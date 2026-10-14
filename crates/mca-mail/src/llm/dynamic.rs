//! Runtime-switchable LLM provider.
//!
//! The environment configuration remains the bootstrap source of truth. When
//! an *active* profile exists in `llm_profiles` (edited from the admin
//! panel), it overrides model, endpoint, key and pricing for every call —
//! with a short-lived cached snapshot so the hot path never blocks on the
//! database and a model switch takes effect within seconds without restart.
//!
//! The snapshot is read under a plain `RwLock`, the guard is dropped before
//! any `.await`, so the future stays `Send`.

use std::sync::RwLock;
use std::time::{Duration, Instant};

use async_trait::async_trait;

use super::openai::OpenAiProvider;
use super::{LlmProvider, LlmRequest, LlmResponse};
use crate::config::{LlmProviderKind, LlmSettings, ModelPrice, ModelRouting, Secret};
use crate::domain::ModelTier;
use crate::error::LlmError;
use crate::persistence::llm_profile_repo::{self, ProfileFull};

/// How long a snapshot (including "no active profile") is trusted.
const SNAPSHOT_TTL: Duration = Duration::from_secs(5);

/// The active profile compiled into a ready-to-call provider.
#[derive(Clone)]
struct ActiveProfile {
    id: uuid::Uuid,
    updated_at: chrono::DateTime<chrono::Utc>,
    provider: OpenAiProviderHandle,
    model: String,
    price: ModelPrice,
}

type OpenAiProviderHandle = std::sync::Arc<OpenAiProvider>;

#[derive(Default)]
struct Snapshot {
    profile: Option<ActiveProfile>,
    fetched_at: Option<Instant>,
}

pub struct DynamicProvider {
    /// The provider built from environment settings; used verbatim while no
    /// profile is active, and for all default/temperature/name behaviour.
    base: LlmProviderHandle,
    /// Settings the base was built from; profile providers reuse its
    /// timeouts, retries and JSON mode.
    base_settings: LlmSettings,
    pool: sqlx::PgPool,
    snapshot: RwLock<Snapshot>,
}

type LlmProviderHandle = super::ProviderHandle;

impl DynamicProvider {
    pub fn new(base: LlmProviderHandle, base_settings: LlmSettings, pool: sqlx::PgPool) -> Self {
        DynamicProvider {
            base,
            base_settings,
            pool,
            snapshot: RwLock::new(Snapshot::default()),
        }
    }

    /// Load the active profile into the snapshot if the TTL has expired.
    /// Errors keep the previous snapshot (and its TTL), so a database hiccup
    /// neither switches models nor stalls the pipeline.
    async fn refresh(&self) {
        let fresh = self
            .snapshot
            .read()
            .map(|snap| {
                snap.fetched_at
                    .is_some_and(|at| at.elapsed() < SNAPSHOT_TTL)
            })
            .unwrap_or(false);
        if fresh {
            return;
        }

        let queried = llm_profile_repo::active(&self.pool).await;
        let next = match queried {
            Ok(row) => row.map(|row| self.resolve_profile(row)),
            Err(e) => {
                tracing::warn!(error = %e, "llm profile refresh failed; keeping previous snapshot");
                self.touch();
                return;
            }
        };

        if let Ok(mut snap) = self.snapshot.write() {
            snap.profile = next;
            snap.fetched_at = Some(Instant::now());
        }
    }

    /// Reuse the compiled provider when the row has not changed since the
    /// last snapshot; rebuild it otherwise (model/key/price edits bump
    /// `updated_at`).
    fn resolve_profile(&self, row: ProfileFull) -> ActiveProfile {
        let unchanged = self.snapshot.read().ok().and_then(|snap| {
            let current = snap.profile.as_ref()?;
            (current.id == row.id && current.updated_at == row.updated_at).then(|| current.clone())
        });
        if let Some(profile) = unchanged {
            return profile;
        }
        ActiveProfile::compile(&self.base_settings, row)
    }

    fn touch(&self) {
        if let Ok(mut snap) = self.snapshot.write() {
            snap.fetched_at = Some(Instant::now());
        }
    }

    /// Snapshot copy of the active profile, if one is selected. Read-only;
    /// the caller must not hold the guard across `.await`.
    fn active_profile(&self) -> Option<ActiveProfile> {
        self.snapshot
            .read()
            .ok()
            .and_then(|snap| snap.profile.clone())
    }
}

impl ActiveProfile {
    /// Build an OpenAI-compatible provider for the profile row, inheriting
    /// transport behaviour (timeouts, retries, JSON mode) from the base
    /// settings but overriding everything the panel controls.
    fn compile(base: &LlmSettings, row: ProfileFull) -> ActiveProfile {
        let settings = LlmSettings {
            provider: LlmProviderKind::OpenAiCompatible,
            disabled: false,
            base_url: row.base_url,
            api_key: Secret::new(row.api_key),
            model: row.model.clone(),
            // One model for every tier: the panel switches the whole pipeline
            // to a single chosen model.
            routing: ModelRouting::default(),
            price: ModelPrice {
                prompt_per_million_usd: row.price_prompt_per_million_usd,
                completion_per_million_usd: row.price_completion_per_million_usd,
            },
            ..base.clone()
        };
        ActiveProfile {
            id: row.id,
            updated_at: row.updated_at,
            provider: std::sync::Arc::new(OpenAiProvider::new(&settings)),
            model: row.model,
            price: settings.price,
        }
    }
}

#[async_trait]
impl LlmProvider for DynamicProvider {
    async fn chat(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        self.refresh().await;
        // Provider cloned out of the guard, then the guard is dropped, so no
        // lock is held across the network call.
        if let Some(profile) = self.active_profile() {
            return profile.provider.chat(req).await;
        }
        self.base.chat(req).await
    }

    async fn chat_for_tier(
        &self,
        tier: ModelTier,
        system: &str,
        messages: Vec<super::LlmMessage>,
        max_tokens: u32,
    ) -> Result<LlmResponse, LlmError> {
        // Refresh first so the model resolved for logging and the request
        // itself come from the same snapshot.
        self.refresh().await;
        let model = self.resolve_model(tier);
        let req = LlmRequest {
            model,
            system_prompt: system.to_string(),
            messages,
            max_tokens,
            temperature: self.default_temperature(),
            top_p: 0.9,
            response_schema: None,
        };
        self.chat(req).await
    }

    fn resolve_model(&self, tier: ModelTier) -> String {
        if let Some(profile) = self.active_profile() {
            return profile.model;
        }
        self.base.resolve_model(tier)
    }

    fn default_temperature(&self) -> f32 {
        self.base.default_temperature()
    }

    fn provider_name(&self) -> &'static str {
        self.base.provider_name()
    }

    fn estimate_cost(&self, prompt: i64, completion: i64) -> i64 {
        if let Some(profile) = self.active_profile() {
            return profile.price.cost_micros(prompt, completion);
        }
        self.base.estimate_cost(prompt, completion)
    }
}
