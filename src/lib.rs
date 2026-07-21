//! `fujin` — the Actions layer of the the host family: the maturity
//! boundary between deliberation and execution (manifest §6.5, §13, §16).
//!
//! A self-contained open-core skeleton: it defines its own primitives,
//! domain output types, and the local Daruma contract types. It has
//! **no** dependency on daruma and **no** dependency on sibling `*_oss`
//! layers. the host supplies concrete daruma adapters and any cross-layer
//! wiring — implementations live only inside the host.
//!
//! # Contract
//! - Domain primitives stay storage-agnostic; the server persists action packets.
//! - The maturity check ([`maturity::assess`]) is **deterministic**:
//!   the same packet always yields the same verdict.
//! - Errors propagate as [`error::ActionsError`].
//!

pub mod agent;
pub mod ai;
pub mod error;
pub mod handoff;
pub mod maturity;
pub mod minimality;
pub mod pack;
pub mod packet;

pub use agent::{Actor, ActorKind, NewPlan, NewTask, ProjectId};
pub use ai::{AiError, AiOutput, AiProvider, AiRequest, AiUsage, ToolCall};
pub use error::ActionsError;
pub use handoff::{into_new_plan, to_handoff, NewPlanWithTasks};
pub use maturity::{assess, Maturity};
pub use minimality::{
    check_minimality, CodeActionMinimality, DebtMarker, DependencyJustification, EvidenceCheck,
    MinimalityCheck, MinimalityPolicy, MinimalityVerdict, ProtectedRequirements,
    M2_NO_MINIMALITY_CHECK, M3_NEW_DEP_UNJUSTIFIED, M4_PROTECTED_REQUIREMENT_CUT,
    M5_NONTRIVIAL_NO_EVIDENCE, M6_DEBT_MARKER_NO_CEILING, W1_DEBT_MARKER_NO_UPGRADE_TRIGGER,
    W2_NEED_NOT_ESTABLISHED,
};
pub use pack::pack_ai;
pub use packet::{
    ActionPacket, ExecutionStep, Gate, HandoffPacket, HandoffProject, LinkedItem, RequiredDocument,
    TargetFiles, TaskCandidate, WorkOrder,
};

/// Map the wire representation of a PlanBrief (§15) into an ActionPacket (§13).
pub fn packet_from_brief(brief: &serde_json::Value) -> ActionPacket {
    fn text(value: &serde_json::Value, field: &str) -> String {
        value[field].as_str().unwrap_or_default().to_owned()
    }
    fn strings(value: &serde_json::Value, field: &str) -> Vec<String> {
        value[field]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect()
    }
    fn linked(ids: Vec<String>) -> Vec<LinkedItem> {
        ids.into_iter()
            .map(|id| LinkedItem {
                label: id.clone(),
                id,
            })
            .collect()
    }

    ActionPacket {
        goal: text(brief, "goal"),
        context: text(brief, "daruma_target"),
        do_items: strings(brief, "in_scope"),
        why: text(brief, "why_now"),
        do_not: strings(brief, "out_of_scope"),
        completion_criteria: strings(brief, "completion_criteria"),
        constraints: strings(brief, "constraints"),
        risks: strings(brief, "risks"),
        dependencies: strings(brief, "dependencies"),
        required_documents: strings(brief, "required_artifacts")
            .into_iter()
            .map(|title| RequiredDocument {
                uri: format!("artifact://{title}"),
                title,
            })
            .collect(),
        linked_decisions: linked(strings(brief, "decisions_made")),
        linked_knowledge: linked(strings(brief, "knowledge_base")),
        linked_rejected: linked(strings(brief, "rejected_alternatives")),
        ..ActionPacket::default()
    }
}

#[cfg(test)]
mod adapter_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn brief_fields_are_preserved_in_packet() {
        let packet = packet_from_brief(&json!({
            "goal": "Ship storage",
            "daruma_target": "one task",
            "in_scope": ["persist artifacts"],
            "why_now": "avoid data loss",
            "decisions_made": ["dec_1"]
        }));
        assert_eq!(packet.goal, "Ship storage");
        assert_eq!(packet.do_items, ["persist artifacts"]);
        assert_eq!(packet.why, "avoid data loss");
        assert_eq!(packet.linked_decisions[0].id, "dec_1");
    }
}
