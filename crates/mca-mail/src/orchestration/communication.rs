//! Phase 4: reply planning, conversation append and the exit conditions of
//! the automatic dialogue.

use tracing::{info, warn};

use super::pipeline::call_agent;
use super::{AgentContextBuilder, Orchestrator};
use crate::agents::CommunicationAgent;
use crate::domain::{
    AgentKind, ConversationDirection, ConversationEntry, DraftStatus, EmailDraft, LeadId,
    LeadStatus, OutboundState, ProcessingStage, ReplyDisposition, RunId,
};
use crate::error::AppError;
use crate::observability::Correlation;
use crate::persistence::{draft_repo, lead_repo, outbox_repo, outbox_repo::OutboundKind, run_repo};

use super::dialogue::{self, Dialogue};

impl Orchestrator {
    pub(super) async fn phase_communication(
        &self,
        corr: &Correlation,
        ctx_builder: &AgentContextBuilder,
        run_id: &RunId,
        email_id: &uuid::Uuid,
        lead_id: LeadId,
        company_name: Option<String>,
    ) -> Result<(), AppError> {
        if !self.config.agents.communication {
            return Ok(());
        }
        run_repo::record_stage(&self.pool, *run_id, ProcessingStage::ReplyPlanning, false).await?;

        let state = Dialogue::load(&self.pool, lead_id).await?;
        if let Some(reason) = self.silence_reason(&state) {
            info!(reason, "automatic reply suppressed");
            run_repo::record_stage(&self.pool, *run_id, ProcessingStage::ReplyPlanning, true)
                .await?;
            return Ok(());
        }

        let ctx = ctx_builder.build_for(AgentKind::EmailCommunication).await?;
        let agent = CommunicationAgent::new(self.llm.clone());
        let output = call_agent(
            corr,
            AgentKind::EmailCommunication,
            |o| o.disposition.clone(),
            agent.plan_response(&ctx, lead_id, &state),
        )
        .await?;
        let plan = output.into_plan();

        run_repo::record_stage(&self.pool, *run_id, ProcessingStage::ReplyPlanning, true).await?;

        // The customer's address is taken from the message we are answering.
        // The model never names a recipient; it produces a body, and the
        // backend decides where that body may go.
        let recipient = ctx.email.from_address.clone();
        let fresh_questions = state.new_questions(&plan.questions);
        if !plan.questions.is_empty() && fresh_questions.is_empty() {
            info!("every question in the reply was already asked; suppressed");
            return Ok(());
        }
        if state.complete && !plan.questions.is_empty() {
            info!("dialogue already complete; a repeated question is not sent");
            return Ok(());
        }

        // The model asks, policy decides. Two directions, one gate:
        //   auto-send on  -> nothing but an outright `suppress` stays behind.
        //                    There is no reviewer in the loop, so a `draft`
        //                    would sit in the queue forever and the reply the
        //                    pipeline was asked to produce would never go out.
        //   auto-send off -> `send` is demoted to `draft`, because the server,
        //                    never the model, opens the outbound gate.
        // Handoff and suppression keep their meaning either way.
        let disposition = match plan.disposition {
            ReplyDisposition::Suppress => ReplyDisposition::Suppress,
            _ if self.may_autosend() => ReplyDisposition::Send,
            ReplyDisposition::Send => ReplyDisposition::Draft,
            other => other,
        };

        match disposition {
            ReplyDisposition::Suppress => info!("response suppressed"),
            ReplyDisposition::Draft | ReplyDisposition::Send => {
                let draft = EmailDraft {
                    id: uuid::Uuid::new_v4(),
                    lead_id: Some(lead_id),
                    email_id: Some(*email_id),
                    in_reply_to: ctx.email.internet_message_id.clone(),
                    to_addresses: vec![recipient],
                    cc_addresses: vec![],
                    subject: plan.subject.clone(),
                    body: plan.body.clone(),
                    status: DraftStatus::PendingApproval,
                    idempotency_key: draft_repo::idempotency_key(*run_id, lead_id, &plan.body),
                    suppression_reason: None,
                    reviewed_by: None,
                    reviewed_at: None,
                    sent_at: None,
                    provider_message_id: None,
                    created_at: chrono::Utc::now(),
                };
                let (draft_id, created) = draft_repo::create(&self.pool, &draft).await?;
                crate::observability::actions::draft_created(
                    corr,
                    draft_id,
                    lead_id,
                    disposition.as_str(),
                );
                info!(draft_id = %draft_id, created, "draft prepared");

                // Only an outright "send" enters the outbox. A draft waits for
                // a human, and enqueueing it would hand the decision to the
                // send worker's `auto_send` flag instead of to the reviewer.
                if disposition.is_send() {
                    self.enqueue_reply(corr, run_id, lead_id, &draft, draft_id)
                        .await?;
                }

                // The outbound turn becomes part of the lead's history, so the
                // next turn can see what it already said. The unique index on
                // `idempotency_key` makes a replayed run append nothing.
                dialogue::record_outbound(
                    &self.pool,
                    &ConversationEntry {
                        id: uuid::Uuid::new_v4(),
                        lead_id,
                        email_id: Some(*email_id),
                        direction: ConversationDirection::Outbound,
                        state: OutboundState::Draft,
                        subject: plan.subject.clone(),
                        body: plan.body.clone(),
                        sent_at: None,
                        created_at: chrono::Utc::now(),
                        idempotency_key: dialogue::outbound_key(draft_id),
                    },
                )
                .await?;

                self.after_reply(corr, lead_id, &state, &company_name)
                    .await?;
            }
        }

        if plan.handoff_requested && self.config.agents.handoff {
            self.phase_handoff(
                corr,
                &ctx,
                lead_id,
                company_name,
                plan.handoff_reason.as_ref().map(|r| r.as_str()),
            )
            .await?;
        }
        Ok(())
    }

    /// Hand a reviewed-for-auto-send reply to the outbox.
    ///
    /// The outbox key is the conversation entry's key, so the history, the
    /// draft and the send all collapse onto one identity: replaying this run
    /// enqueues nothing new, and the send worker later knows exactly which
    /// history row to flip to `sent`.
    async fn enqueue_reply(
        &self,
        corr: &Correlation,
        run_id: &RunId,
        lead_id: LeadId,
        draft: &EmailDraft,
        draft_id: uuid::Uuid,
    ) -> Result<(), AppError> {
        let recipient = match draft.to_addresses.first() {
            Some(to) => to.clone(),
            None => {
                warn!("reply draft has no recipient; nothing queued");
                return Ok(());
            }
        };
        let intent = outbox_repo::OutboundIntent {
            message_type: OutboundKind::CustomerReply,
            lead_id: Some(lead_id),
            email_id: draft.email_id,
            run_id: Some(*run_id),
            recipient,
            subject: draft.subject.clone(),
            body_text: draft.body.clone(),
            body_html: String::new(),
            in_reply_to: draft.in_reply_to.clone(),
            ref_headers: Vec::new(),
            idempotency_key: dialogue::outbound_key(draft_id),
            correlation_id: Some(corr.processing_id.clone()),
        };
        match outbox_repo::enqueue_send(&self.pool, &intent).await? {
            Some(outbox_id) => {
                crate::observability::actions::reply_queued(corr, outbox_id, lead_id);
                info!(outbox_id = %outbox_id, "reply queued for delivery");
            }
            None => info!("reply already queued for this draft"),
        }
        Ok(())
    }

    /// Why this lead must not be spoken to right now, if any.
    ///
    /// Order matters: a lock is permanent, a pacing limit is temporary.
    fn silence_reason(&self, state: &Dialogue) -> Option<&'static str> {
        if state.locked {
            return Some("automation locked for this lead");
        }
        if state.unanswered >= i64::from(self.config.security.outbound.max_consecutive_replies) {
            return Some("too many replies without a customer answer");
        }
        if state.outbound_count >= i64::from(self.config.security.outbound.max_consecutive_replies)
        {
            return Some("reply budget for this lead exhausted");
        }
        None
    }

    fn may_autosend(&self) -> bool {
        self.config.security.email_mode.allows_sending() && self.config.security.outbound.auto_send
    }

    /// The dialogue is over: either the data is complete, or nothing more is
    /// going to be said. That is the moment the lead becomes a manager's job.
    async fn after_reply(
        &self,
        corr: &Correlation,
        lead_id: LeadId,
        state: &Dialogue,
        _company_name: &Option<String>,
    ) -> Result<(), AppError> {
        if !state.complete {
            return Ok(());
        }
        lead_repo::update_status(&self.pool, lead_id, LeadStatus::Qualified).await?;
        info!(lead_id = %lead_id, "requirements complete; lead ready for a manager");
        crate::observability::actions::lead_qualified(corr, lead_id);
        self.enqueue_manager_card(corr, lead_id).await
    }
}
