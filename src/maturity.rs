//! Maturity gate for the §13 Action Packet.
//!
//! Manifest §13/§16: only a *mature* Action Packet may cross into
//! Daruma. Maturity here is **deterministic** — every required §13
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
    /// Daruma.
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
/// be non-blank (after trimming); list fields must be non-empty *and*
/// every element in them must itself carry content (no blank strings, no
/// structs with blank required sub-fields) — a list padded with empty
/// placeholders is not a filled field. The checks run in §13 order so
/// `missing` reads as a checklist.
pub fn assess(packet: &ActionPacket) -> Maturity {
    let mut missing = Vec::new();

    fn str_present(s: &str) -> bool {
        !s.trim().is_empty()
    }
    fn list_present(items: &[String]) -> bool {
        !items.is_empty() && items.iter().all(|s| str_present(s))
    }
    fn structs_present<T>(items: &[T], all_ok: impl Fn(&T) -> bool) -> bool {
        !items.is_empty() && items.iter().all(all_ok)
    }

    if !str_present(&packet.goal) {
        missing.push("goal");
    }
    if !str_present(&packet.context) {
        missing.push("context");
    }
    if !list_present(&packet.do_items) {
        missing.push("do_items");
    }
    if !str_present(&packet.why) {
        missing.push("why");
    }
    if !list_present(&packet.do_not) {
        missing.push("do_not");
    }
    if !list_present(&packet.completion_criteria) {
        missing.push("completion_criteria");
    }
    if !list_present(&packet.constraints) {
        missing.push("constraints");
    }
    if !list_present(&packet.risks) {
        missing.push("risks");
    }
    if !list_present(&packet.dependencies) {
        missing.push("dependencies");
    }
    if !structs_present(&packet.required_documents, |d| {
        str_present(&d.title) && str_present(&d.uri)
    }) {
        missing.push("required_documents");
    }
    if !structs_present(&packet.linked_decisions, |i| {
        str_present(&i.id) && str_present(&i.label)
    }) {
        missing.push("linked_decisions");
    }
    if !structs_present(&packet.linked_knowledge, |i| {
        str_present(&i.id) && str_present(&i.label)
    }) {
        missing.push("linked_knowledge");
    }
    if !structs_present(&packet.linked_rejected, |i| {
        str_present(&i.id) && str_present(&i.label)
    }) {
        missing.push("linked_rejected");
    }
    if !list_present(&packet.expected_artifacts) {
        missing.push("expected_artifacts");
    }
    if !structs_present(&packet.before_start, |g| str_present(&g.rule)) {
        missing.push("before_start");
    }
    if !structs_present(&packet.before_complete, |g| str_present(&g.rule)) {
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
            why: "Only mature packets may reach Daruma".into(),
            do_not: vec!["do not call any model".into()],
            completion_criteria: vec!["cargo test green".into()],
            constraints: vec!["no ai-infra dependency".into()],
            risks: vec!["field drift vs §13".into()],
            dependencies: vec!["domain crate".into()],
            required_documents: vec![RequiredDocument {
                title: "manifest §13".into(),
                uri: "docs/the host/manifest.md".into(),
            }],
            linked_decisions: vec![item("dec-1")],
            linked_knowledge: vec![item("kn-1")],
            linked_rejected: vec![item("rej-1")],
            expected_artifacts: vec!["fujin crate".into()],
            before_start: vec![Gate {
                rule: "charter read".into(),
            }],
            before_complete: vec![Gate {
                rule: "tests pass".into(),
            }],
            ..ActionPacket::default()
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
    fn blank_item_in_string_list_is_not_present() {
        let mut p = full_packet();
        p.do_items = vec!["   ".into()]; // non-empty Vec, but the one item is blank
        assert_eq!(
            assess(&p),
            Maturity::NotReady {
                missing: vec!["do_items".to_string()]
            }
        );
    }

    #[test]
    fn blank_gate_rule_is_not_present() {
        let mut p = full_packet();
        p.before_start = vec![Gate { rule: "".into() }];
        assert_eq!(
            assess(&p),
            Maturity::NotReady {
                missing: vec!["before_start".to_string()]
            }
        );
    }

    #[test]
    fn blank_required_document_uri_is_not_present() {
        let mut p = full_packet();
        p.required_documents = vec![RequiredDocument {
            title: "manifest §13".into(),
            uri: "  ".into(),
        }];
        assert_eq!(
            assess(&p),
            Maturity::NotReady {
                missing: vec!["required_documents".to_string()]
            }
        );
    }

    #[test]
    fn assessment_is_deterministic() {
        let p = full_packet();
        assert_eq!(assess(&p), assess(&p));
    }

    // ── Per-field NotReady tests ──────────────────────────────────────────────
    //
    // For each of the 16 §13 required fields: start from a fully-filled packet,
    // clear exactly one field, and assert that assess() returns NotReady with
    // exactly that field name in `missing`.

    fn clear_field(field: &str) -> ActionPacket {
        let mut p = full_packet();
        match field {
            "goal" => p.goal = String::new(),
            "context" => p.context = String::new(),
            "do_items" => p.do_items.clear(),
            "why" => p.why = String::new(),
            "do_not" => p.do_not.clear(),
            "completion_criteria" => p.completion_criteria.clear(),
            "constraints" => p.constraints.clear(),
            "risks" => p.risks.clear(),
            "dependencies" => p.dependencies.clear(),
            "required_documents" => p.required_documents.clear(),
            "linked_decisions" => p.linked_decisions.clear(),
            "linked_knowledge" => p.linked_knowledge.clear(),
            "linked_rejected" => p.linked_rejected.clear(),
            "expected_artifacts" => p.expected_artifacts.clear(),
            "before_start" => p.before_start.clear(),
            "before_complete" => p.before_complete.clear(),
            other => panic!("unknown field: {other}"),
        }
        p
    }

    #[test]
    fn each_missing_field_produces_not_ready_with_that_field_name() {
        let fields = [
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
        ];

        for field in fields {
            let p = clear_field(field);
            match assess(&p) {
                Maturity::NotReady { missing } => {
                    assert!(
                        missing.contains(&field.to_string()),
                        "field `{field}` cleared but not listed in missing; got: {missing:?}"
                    );
                    // Only this one field should appear as missing.
                    assert_eq!(
                        missing.len(),
                        1,
                        "expected only `{field}` in missing but got: {missing:?}"
                    );
                }
                Maturity::Ready => {
                    panic!("clearing `{field}` should yield NotReady but got Ready");
                }
            }
        }
    }
}
