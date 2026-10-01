//! Agent Orchestrator — manages the agent pipeline for each inbound email.

use std::sync::Arc;

use tracing::{error, info, warn};

use crate::agents::{
    ClassificationAgent, CommunicationAgent, HandoffAgent, LeadQualificationAgent, SpamAgent,
};
use crate::config::AppConfig;
use crate::domain::{
    AgentKind, EmailCategory, EmailStatus, EmailThread, Handoff, HandoffReason, HandoffState,
    LeadStatus, Priority, ProcessingStage, RequirementScope, RunId, RunState, RunTrigger,
    SpamVerdict,
};
use crate::error::AppError;
use crate::llm::LlmProvider;
use crate::persistence::{draft_repo, email_repo, handoff_repo, lead_repo, run_repo, thread_repo};
use crate::tools::ToolRegistry;

pub mod context;
pub mod loop_guard;

pub use context::{AgentContext, AgentContextBuilder};
pub use loop_guard::{BudgetTracker, IterationGuard, ToolCallGuard};

pub struct Orchestrator {
    llm: Arc<dyn LlmProvider>,
    tools: ToolRegistry,
    pool: sqlx::PgPool,
    config: AppConfig,
}

impl Orchestrator {
    pub fn new(
        llm: Arc<dyn LlmProvider>,
        tools: ToolRegistry,
        config: &AppConfig,
        pool: sqlx::PgPool,
    ) -> Self {
        Orchestrator {
            llm,
            tools,
            pool,
            config: config.clone(),
        }
    }

    /// Process a single email through the full pipeline.
    ///
    /// Accepts both `pending` (fresh poll) and `processing` (claimed by the
    /// queue worker via `claim_batch`, which is atomic); anything already
    /// finished is skipped.
    pub async fn process_email(&self, email_id: uuid::Uuid) -> Result<(), AppError> {
        let email = email_repo::get(&self.pool, email_id).await?;

        if !matches!(email.status, EmailStatus::Pending | EmailStatus::Processing) {
            info!(status = %email.status.as_str(), "skipping non-pending email");
            return Ok(());
        }

        email_repo::set_status(&self.pool, email_id, EmailStatus::Processing, None).await?;

        let thread = thread_repo::get(&self.pool, email.thread_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("thread {}", email.thread_id)))?;

        let run = run_repo::start(&self.pool, email_id, email.thread_id, RunTrigger::Poll).await?;

        let run_id = run.id;
        let result = self.run_pipeline(&run_id, &email_id, &thread).await;

        match &result {
            Ok(status) => {
                info!(status = %status.as_str(), "pipeline completed");
                email_repo::set_status(&self.pool, email_id, *status, None).await?;
                run_repo::finish(&self.pool, run_id, RunState::Succeeded, None).await?;
            }
            Err(e) if e.is_retryable() => {
                warn!(error = %e, "retryable error");
                email_repo::record_error(&self.pool, email_id, &e.to_string()).await?;
                run_repo::finish(&self.pool, run_id, RunState::Failed, Some(&e.to_string()))
                    .await?;
            }
            Err(e) => {
                // A malformed agent output is a model glitch, not a broken
                // message: flag it for human review instead of killing it.
                let needs_review = matches!(&e, AppError::Agent(_));
                error!(error = %e, needs_review, "pipeline failed");
                email_repo::record_error(&self.pool, email_id, &e.to_string()).await?;
                if needs_review {
                    email_repo::set_status(&self.pool, email_id, EmailStatus::NeedsReview, None)
                        .await?;
                } else {
                    email_repo::set_status(&self.pool, email_id, EmailStatus::Dead, None).await?;
                }
                run_repo::finish(&self.pool, run_id, RunState::Failed, Some(&e.to_string()))
                    .await?;
            }
        }

        result.map(|_| ())
    }

    async fn run_pipeline(
        &self,
        run_id: &RunId,
        email_id: &uuid::Uuid,
        thread: &EmailThread,
    ) -> Result<EmailStatus, AppError> {
        let ctx_builder = AgentContextBuilder::new(
            self.pool.clone(),
            *run_id,
            *email_id,
            self.llm.clone(),
            self.tools.clone(),
            self.config.agents.clone(),
            self.config.security.email_mode,
        );

        // Phase 1: Spam detection
        if self.config.agents.spam {
            info!("Spam Agent");
            run_repo::record_stage(&self.pool, *run_id, ProcessingStage::Spam, false).await?;

            let ctx = ctx_builder.build_for(AgentKind::Spam).await?;
            let agent = SpamAgent::new(self.llm.clone());
            let assessment = agent.assess(&ctx).await?;

            run_repo::record_stage(&self.pool, *run_id, ProcessingStage::Spam, true).await?;
            email_repo::set_spam_verdict(&self.pool, *email_id, assessment.verdict).await?;

            match assessment.verdict {
                SpamVerdict::Spam | SpamVerdict::Advertisement => {
                    info!(verdict = ?assessment.verdict, "spam detected");
                    return Ok(EmailStatus::Quarantined);
                }
                SpamVerdict::PhishingSuspected => {
                    info!("phishing suspected, needs review");
                    return Ok(EmailStatus::NeedsReview);
                }
                _ => {}
            }
        }

        // Phase 2: Classification
        let mut email_category = EmailCategory::Uncertain;
        if self.config.agents.classification {
            info!("Classification Agent");
            run_repo::record_stage(&self.pool, *run_id, ProcessingStage::Classification, false)
                .await?;

            let ctx = ctx_builder.build_for(AgentKind::Classification).await?;
            let agent = ClassificationAgent::new(self.llm.clone());
            let outcome = agent.classify(&ctx).await?;
            email_category = outcome.category;

            run_repo::record_stage(&self.pool, *run_id, ProcessingStage::Classification, true)
                .await?;
            email_repo::set_category(&self.pool, *email_id, email_category).await?;

            match email_category {
                EmailCategory::Spam => {
                    return Ok(EmailStatus::Quarantined);
                }
                EmailCategory::Advertisement | EmailCategory::Internal => {
                    return Ok(EmailStatus::Processed);
                }
                _ => {}
            }
        }

        // Phase 3: Lead Qualification (for commercial emails)
        if self.config.agents.qualification {
            info!("Lead Qualification Agent");
            run_repo::record_stage(&self.pool, *run_id, ProcessingStage::Qualification, false)
                .await?;

            let ctx = ctx_builder.build_for(AgentKind::LeadQualification).await?;
            let agent = LeadQualificationAgent::new(self.llm.clone());
            let qual_result = agent.qualify(&ctx).await?;

            run_repo::record_stage(&self.pool, *run_id, ProcessingStage::Qualification, true)
                .await?;

            // Create or find lead
            let scope = RequirementScope::from_category(email_category);
            let (lead_id, _) = lead_repo::ensure(
                &self.pool,
                &thread.conversation_key,
                &ctx.email.from_address,
                Some(ctx.thread_id),
                Some(*email_id),
                scope,
            )
            .await?;

            run_repo::set_lead(&self.pool, *run_id, lead_id).await?;
            email_repo::attach_lead(&self.pool, *email_id, lead_id).await?;

            // Phase 4: Communication
            if self.config.agents.communication {
                info!("Communication Agent");
                run_repo::record_stage(&self.pool, *run_id, ProcessingStage::ReplyPlanning, false)
                    .await?;

                let ctx = ctx_builder.build_for(AgentKind::EmailCommunication).await?;
                let agent = CommunicationAgent::new(self.llm.clone());
                let plan = agent.plan_response(&ctx, lead_id).await?;

                run_repo::record_stage(&self.pool, *run_id, ProcessingStage::ReplyPlanning, true)
                    .await?;

                match plan.disposition.as_str() {
                    "suppress" => info!("response suppressed"),
                    "draft" | "send" => {
                        let draft = crate::domain::EmailDraft {
                            id: uuid::Uuid::new_v4(),
                            lead_id: Some(lead_id),
                            email_id: Some(*email_id),
                            in_reply_to: ctx.email.internet_message_id.clone(),
                            to_addresses: vec![ctx.email.from_address.clone()],
                            cc_addresses: vec![],
                            subject: plan.subject.clone(),
                            body: plan.body.clone(),
                            status: crate::domain::DraftStatus::PendingApproval,
                            idempotency_key: draft_repo::idempotency_key(
                                *run_id, lead_id, &plan.body,
                            ),
                            suppression_reason: None,
                            reviewed_by: None,
                            reviewed_at: None,
                            sent_at: None,
                            provider_message_id: None,
                            created_at: chrono::Utc::now(),
                        };
                        let (draft_id, created) = draft_repo::create(&self.pool, &draft).await?;
                        info!(
                            draft_id = %draft_id,
                            created,
                            disposition = %plan.disposition,
                            "draft prepared"
                        );
                    }
                    _ => {}
                }

                // Phase 5: Handoff if requested
                if plan.handoff_requested {
                    info!("Handoff Agent triggered");
                    let handoff_agent = crate::agents::HandoffAgent::new(self.llm.clone());
                    let ho = handoff_agent.handoff(&ctx, lead_id).await?;

                    let reason = match plan.handoff_reason.as_deref() {
                        Some("callback_requested") => HandoffReason::CallbackRequested,
                        Some("pricing_or_legal") => HandoffReason::PricingOrLegal,
                        Some("compliance_review") => HandoffReason::ComplianceReview,
                        _ => HandoffReason::ReadyForManager,
                    };

                    let handoff = Handoff {
                        id: uuid::Uuid::new_v4(),
                        lead_id,
                        thread_id: Some(ctx.thread_id),
                        run_id: Some(*run_id),
                        email_id: Some(*email_id),
                        reason,
                        priority: Priority::Normal,
                        state: HandoffState::Open,
                        contact_email: ctx.email.from_address.clone(),
                        contact_name: ctx.email.from_name.clone(),
                        contact_phone: None,
                        company_name: qual_result.company_name,
                        company_inn: None,
                        original_request: ho.original_request,
                        category: None,
                        spam_verdict: None,
                        cargo_summary: ho.cargo_summary,
                        route_summary: ho.route_summary,
                        requested_service: ho.requested_service,
                        missing_information: ho.missing_information,
                        open_questions: ho.open_questions,
                        conversation_digest: ho.conversation_digest,
                        research_digest: None,
                        checks_performed: ho.checks_performed,
                        unresolved_topics: ho.unresolved_topics,
                        assigned_to: None,
                        acknowledged_at: None,
                        created_at: chrono::Utc::now(),
                    };

                    handoff_repo::upsert_open(&self.pool, &handoff).await?;
                    lead_repo::lock_automation(&self.pool, lead_id).await?;
                    lead_repo::update_status(&self.pool, lead_id, LeadStatus::HandedOff).await?;
                }
            }
        }

        Ok(EmailStatus::Processed)
    }
}

/// Builder pattern for constructing Orchestrator with explicit dependencies.
pub struct OrchestratorBuilder {
    config: Option<AppConfig>,
    pool: Option<sqlx::PgPool>,
    tools: Option<ToolRegistry>,
    llm: Option<Arc<dyn LlmProvider>>,
}

impl OrchestratorBuilder {
    pub fn new() -> Self {
        OrchestratorBuilder {
            config: None,
            pool: None,
            tools: None,
            llm: None,
        }
    }

    pub fn config(mut self, config: AppConfig) -> Self {
        self.config = Some(config);
        self
    }
    pub fn pool(mut self, pool: sqlx::PgPool) -> Self {
        self.pool = Some(pool);
        self
    }
    pub fn tools(mut self, tools: ToolRegistry) -> Self {
        self.tools = Some(tools);
        self
    }
    pub fn llm(mut self, llm: Arc<dyn LlmProvider>) -> Self {
        self.llm = Some(llm);
        self
    }

    pub fn build(self) -> Result<Orchestrator, AppError> {
        let config = self
            .config
            .ok_or_else(|| AppError::internal("config required"))?;
        let pool = self
            .pool
            .ok_or_else(|| AppError::internal("pool required"))?;
        let tools = self.tools.unwrap_or_default();
        let llm = self
            .llm
            .unwrap_or_else(|| Arc::new(crate::llm::mock::MockLlmProvider::new(&config.llm)));
        Ok(Orchestrator::new(llm, tools, &config, pool))
    }
}

impl Default for OrchestratorBuilder {
    fn default() -> Self {
        Self::new()
    }
}
