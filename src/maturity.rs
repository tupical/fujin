//! Maturity gate for the §13 Action Packet.
//!
//! Manifest §13/§16: only a *mature* Action Packet may cross into
//! TaskAgent. Maturity here is **deterministic** — every required §13
//! field must be present (non-empty). No model call, no heuristics: the
//! same packet always yields the same verdict, and a `NotReady` verdict
//! names exactly which fields are missing so the upper layers know what
//! to finish.

use serde::{Deserialize, Serialize};

use crate::packet::ActionPacket;

/// The deterministic verdict for an [`ActionPacket`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum Maturity {
    /// Every required §13 field is filled — the packet may spawn work in
    /// TaskAgent.
    Ready,
    /// At least one required §13 field is empty. `missing` lists the
    /// field names (in §13 order) that still need filling.
    NotReady { missing: Vec<String> },
}

impl Maturity {
    pub fn is_ready(&self) -> bool {
        matches!(self, Maturity::Ready)
    }
}

/// Assess an [`ActionPacket`] against the §13 contract.
///
/// A field counts as "present" if it carries content: string fields must
/// be non-blank (after trimming), list fields must be non-empty. The
/// checks run in §13 order so `missing` reads as a checklist.
pub fn assess(packet: &ActionPacket) -> Maturity {
    let mut missing = Vec::new();

    let str_present = |s: &str| !s.trim().is_empty();

    if !str_present(&packet.goal) {
        missing.push("goal");
    }
    if !str_present(&packet.context) {
        missing.push("context");
    }
    if packet.do_items.is_empty() {
        missing.push("do_items");
    }
    if !str_present(&packet.why) {
        missing.push("why");
    }
    if packet.do_not.is_empty() {
        missing.push("do_not");
    }
    if packet.completion_criteria.is_empty() {
        missing.push("completion_criteria");
    }
    if packet.constraints.is_empty() {
        missing.push("constraints");
    }
    if packet.risks.is_empty() {
        missing.push("risks");
    }
    if packet.dependencies.is_empty() {
        missing.push("dependencies");
    }
    if packet.required_documents.is_empty() {
        missing.push("required_documents");
    }
    if packet.linked_decisions.is_empty() {
        missing.push("linked_decisions");
    }
    if packet.linked_knowledge.is_empty() {
        missing.push("linked_knowledge");
    }
    if packet.linked_rejected.is_empty() {
        missing.push("linked_rejected");
    }
    if packet.expected_artifacts.is_empty() {
        missing.push("expected_artifacts");
    }
    if packet.before_start.is_empty() {
        missing.push("before_start");
    }
    if packet.before_complete.is_empty() {
        missing.push("before_complete");
    }

    if missing.is_empty() {
        Maturity::Ready
    } else {
        Maturity::NotReady {
            missing: missing.into_iter().map(String::from).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::{Gate, LinkedItem, RequiredDocument};

    /// A packet with every §13 field filled.
    fn full_packet() -> ActionPacket {
        let item = |s: &str| LinkedItem {
            id: s.to_string(),
            label: s.to_string(),
        };
        ActionPacket {
            goal: "Ship the maturity gate".into(),
            context: "Wave-2b Actions layer".into(),
            do_items: vec!["implement assess()".into()],
            why: "Only mature packets may reach TaskAgent".into(),
            do_not: vec!["do not call any model".into()],
            completion_criteria: vec!["cargo test green".into()],
            constraints: vec!["no ai-infra dependency".into()],
            risks: vec!["field drift vs §13".into()],
            dependencies: vec!["domain crate".into()],
            required_documents: vec![RequiredDocument {
                title: "manifest §13".into(),
                uri: "docs/mcpbox/manifest.md".into(),
            }],
            linked_decisions: vec![item("dec-1")],
            linked_knowledge: vec![item("kn-1")],
            linked_rejected: vec![item("rej-1")],
            expected_artifacts: vec!["actions-oss crate".into()],
            before_start: vec![Gate {
                rule: "charter read".into(),
            }],
            before_complete: vec![Gate {
                rule: "tests pass".into(),
            }],
        }
    }

    #[test]
    fn full_packet_is_ready() {
        assert_eq!(assess(&full_packet()), Maturity::Ready);
    }

    #[test]
    fn empty_packet_lists_every_missing_field() {
        match assess(&ActionPacket::default()) {
            Maturity::NotReady { missing } => {
                // All 16 §13 fields are flagged, in declaration order.
                assert_eq!(
                    missing,
                    vec![
                        "goal",
                        "context",
                        "do_items",
                        "why",
                        "do_not",
                        "completion_criteria",
                        "constraints",
                        "risks",
                        "dependencies",
                        "required_documents",
                        "linked_decisions",
                        "linked_knowledge",
                        "linked_rejected",
                        "expected_artifacts",
                        "before_start",
                        "before_complete",
                    ]
                );
            }
            Maturity::Ready => panic!("empty packet must not be ready"),
        }
    }

    #[test]
    fn blank_string_field_is_not_present() {
        let mut p = full_packet();
        p.goal = "   ".into(); // whitespace-only counts as missing
        assert_eq!(
            assess(&p),
            Maturity::NotReady {
                missing: vec!["goal".to_string()]
            }
        );
    }

    #[test]
    fn assessment_is_deterministic() {
        let p = full_packet();
        assert_eq!(assess(&p), assess(&p));
    }
}
