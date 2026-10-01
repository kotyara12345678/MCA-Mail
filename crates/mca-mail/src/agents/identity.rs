//! Agent identity metadata (name and self-description shown to the model).

use crate::domain::AgentKind;

/// Display identity of one agent.
pub struct AgentIdentity {
    pub name: &'static str,
    pub description: &'static str,
}

/// Resolve the identity for an agent kind.
pub fn identity_of(kind: AgentKind) -> AgentIdentity {
    match kind {
        AgentKind::Spam => AgentIdentity {
            name: "Spam Agent",
            description: "I identify spam, advertisements, phishing, and automated notifications.",
        },
        AgentKind::Classification => AgentIdentity {
            name: "Classification Agent",
            description: "I classify inbound emails into categories like new lead, existing client, or transport request.",
        },
        AgentKind::LeadQualification => AgentIdentity {
            name: "Lead Qualification Agent",
            description: "I extract commercial details from potential client emails.",
        },
        AgentKind::LogisticsExpert => AgentIdentity {
            name: "Logistics Expert Agent",
            description: "I advise on international logistics, customs, and trade processes.",
        },
        AgentKind::CompanyResearch => AgentIdentity {
            name: "Company Research Agent",
            description: "I check company details from public registries.",
        },
        AgentKind::EmailCommunication => AgentIdentity {
            name: "Email Communication Agent",
            description: "I draft professional replies to potential clients.",
        },
        AgentKind::Handoff => AgentIdentity {
            name: "Handoff Agent",
            description: "I prepare structured lead handoffs for human managers.",
        },
    }
}
