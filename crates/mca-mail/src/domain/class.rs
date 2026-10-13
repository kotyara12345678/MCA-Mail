use serde::{Deserialize, Serialize};

crate::domain::wire_enum! {
    /// Business classification of an inbound message.
    EmailCategory {
        NewLead => "new_lead",
        ExistingClient => "existing_client",
        Partner => "partner",
        TransportRequest => "transport_request",
        FullImportRequest => "full_import_request",
        CustomsRequest => "customs_request",
        ProcurementRequest => "procurement_request",
        DocumentRequest => "document_request",
        Complaint => "complaint",
        Internal => "internal",
        Advertisement => "advertisement",
        Spam => "spam",
        BusinessInquiry => "business_inquiry",
        Other => "other",
        Uncertain => "uncertain",
    }
}

impl EmailCategory {
    /// Categories that represent genuine commercial demand for MCA services.
    pub const fn is_demand(&self) -> bool {
        matches!(
            self,
            EmailCategory::NewLead
                | EmailCategory::ExistingClient
                | EmailCategory::TransportRequest
                | EmailCategory::FullImportRequest
                | EmailCategory::CustomsRequest
                | EmailCategory::ProcurementRequest
                | EmailCategory::BusinessInquiry
        )
    }

    /// Categories that must never receive an automated reply.
    ///
    /// `Other` sits here deliberately: it is the model's way of saying "this
    /// does not fit any business shape", and answering an email nobody
    /// understood is worse than leaving it for a person.
    pub const fn allows_reply(&self) -> bool {
        !matches!(
            self,
            EmailCategory::Spam
                | EmailCategory::Advertisement
                | EmailCategory::Internal
                | EmailCategory::Other
                | EmailCategory::Uncertain
        )
    }

    /// Categories routed to a human without attempting automation.
    pub const fn requires_human(&self) -> bool {
        matches!(self, EmailCategory::Complaint | EmailCategory::Other)
    }

    /// Whether a lead may be created for this category at all.
    ///
    /// A lead is a commercial record; creating one for spam, internal traffic,
    /// an unrecognised message or an unclassified one would pollute the
    /// pipeline the manager reads.
    pub const fn allows_lead(&self) -> bool {
        self.is_demand()
            || matches!(
                self,
                EmailCategory::DocumentRequest | EmailCategory::Partner | EmailCategory::Complaint
            )
    }
}

crate::domain::wire_enum! {
    /// Spam Agent verdict. Only `NotSpam` and `Uncertain` keep a message in the
    /// normal flow; nothing is ever deleted based on this verdict.
    SpamVerdict {
        Spam => "spam",
        Advertisement => "advertisement",
        PhishingSuspected => "phishing_suspected",
        AutomatedNotification => "automated_notification",
        NotSpam => "not_spam",
        Uncertain => "uncertain",
    }
}

impl SpamVerdict {
    /// Whether the message should leave the active inbox.
    pub const fn quarantine(&self) -> bool {
        matches!(self, SpamVerdict::Spam | SpamVerdict::Advertisement)
    }

    /// Whether a human must look at the message before anything else happens.
    pub const fn needs_human_review(&self) -> bool {
        matches!(
            self,
            SpamVerdict::PhishingSuspected | SpamVerdict::Uncertain
        )
    }
}

/// The red flag rule: does this message look like a potential or running order?
///
/// Deliberately deterministic — it owns no model call and no classification
/// of its own, it only consumes what the pipeline already produced. A
/// manager uses the flag to find work, so it has to stay trustworthy:
/// anything ambiguous stays unflagged, and a category the model could not
/// place never earns the flag from context alone.
///
/// `thread_context` is true when another message in the same thread already
/// carries the flag or a lead — that is what ties a client's follow-up (an
/// invoice, a clarification of weight or dates, a confirmation) back to the
/// order the conversation started with.
pub fn is_order_related(
    category: EmailCategory,
    spam: Option<SpamVerdict>,
    confidence: f32,
    thread_context: bool,
) -> bool {
    // Bulk mail, ads and machine notifications are never order traffic,
    // whatever they managed to get classified as.
    match spam {
        Some(verdict) if verdict.quarantine() => return false,
        Some(SpamVerdict::AutomatedNotification) => return false,
        _ => {}
    }
    // A neutral category — a document, a confirmation, a complaint — still
    // belongs to the order when the thread around it already does.
    if thread_context && category.allows_lead() && !category.is_demand() {
        return true;
    }
    // Genuine commercial demand, but only when the model was sure of it or
    // the thread proves the conversation is about an order.
    category.is_demand() && (confidence >= Confidence::AUTO_ACTION_THRESHOLD || thread_context)
}

crate::domain::wire_enum! {
    /// Per-field certainty for an extracted commercial parameter.
    FieldState {
        /// Stated by the customer in their own words.
        Known => "known",
        /// Not present in the correspondence.
        Unknown => "unknown",
        /// Determined to be irrelevant for this request.
        NotApplicable => "not_applicable",
        /// Present but ambiguous, or inferred and not yet confirmed.
        NeedsConfirmation => "needs_confirmation",
    }
}

impl FieldState {
    /// Whether the value may be relied on without asking the customer.
    pub const fn is_trusted(&self) -> bool {
        matches!(self, FieldState::Known | FieldState::NotApplicable)
    }
}

/// Confidence reported by an agent, normalized to `0.0..=1.0`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Confidence(f32);

impl Confidence {
    pub fn new(value: f32) -> Self {
        Self(value.clamp(0.0, 1.0))
    }

    pub fn value(&self) -> f32 {
        self.0
    }

    /// Below this threshold the orchestrator escalates to a human instead of
    /// acting on the agent's output.
    pub const AUTO_ACTION_THRESHOLD: f32 = 0.75;

    pub fn allows_auto_action(&self) -> bool {
        self.0 >= Self::AUTO_ACTION_THRESHOLD
    }
}

impl Default for Confidence {
    fn default() -> Self {
        Self(0.0)
    }
}

#[cfg(test)]
#[path = "class_test.rs"]
mod tests;
