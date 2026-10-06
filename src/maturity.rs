//! Maturity gate for the §13 Action Packet.
//!
//! Manifest §13/§16: only a *mature* Action Packet may cross into
//! Daruma. Maturity here is **deterministic** — every required §13
//! field must be present (non-empty). No model call, no heuristics: the
//! same packet always yields the same verdict, and a `NotReady` verdict
//! names exactly which fields are missing so the upper layers know what
//! to finish.
//!
//! Only blocking fields gate readiness. `constraints`, `risks`,
//! `linked_decisions` and (under Soft) the provenance trio are advisory:
//! see [`advisory_warnings`].

use serde::{Deserialize, Serialize};

use crate::packet::{ActionPacket, Gate, LinkedItem, RequiredDocument};

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

/// How strictly the optional §13 provenance trio is gated.
///
/// Under [`FujinStrictness::Soft`] the three provenance fields
/// (`required_documents`, `linked_knowledge`, `linked_rejected`) may stay
/// empty as long as any present values are complete. Under
/// [`FujinStrictness::Strict`] they must additionally be non-empty.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FujinStrictness {
    /// Current behaviour: provenance fields are valid-or-empty.
    #[default]
    Soft,
    /// Provenance fields must be non-empty *and* valid.
    Strict,
}

/// Assess an [`ActionPacket`] against the §13 contract.
///
/// A field counts as "present" if it carries content: string fields must
/// be non-blank (after trimming); required list fields must be non-empty
/// and every element must carry content. Optional provenance fields may
/// be empty, but values that are present must still be complete. The checks
/// run in §13 order so `missing` reads as a checklist.
pub fn assess(packet: &ActionPacket) -> Maturity {
    assess_with(packet, FujinStrictness::default())
}

/// Like [`assess`], but with an explicit provenance gate strictness.
///
/// The only difference is how the three provenance fields
/// (`required_documents`, `linked_knowledge`, `linked_rejected`) are
/// checked: [`FujinStrictness::Soft`] keeps the current valid-or-empty
/// semantics, [`FujinStrictness::Strict`] demands they be non-empty too.
/// Every other §13 check is unchanged.
pub fn assess_with(packet: &ActionPacket, strictness: FujinStrictness) -> Maturity {
    let mut missing = Vec::new();

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
    if !list_present(&packet.dependencies) {
        missing.push("dependencies");
    }
    if !list_present(&packet.target_files.owned) {
        missing.push("target_files.owned");
    }
    // Provenance trio: a half-filled entry is always blocking; an empty list
    // is blocking only under Strict (under Soft it is an advisory warning).
    let doc_ok = |d: &RequiredDocument| str_present(&d.title) && str_present(&d.uri);
    let item_ok = |i: &LinkedItem| str_present(&i.id) && str_present(&i.label);
    let strict = strictness == FujinStrictness::Strict;
    let trio_ok = |len: usize, all_ok: bool| all_ok && (len > 0 || !strict);
    if !trio_ok(
        packet.required_documents.len(),
        packet.required_documents.iter().all(doc_ok),
    ) {
        missing.push("required_documents");
    }
    if !trio_ok(
        packet.linked_knowledge.len(),
        packet.linked_knowledge.iter().all(item_ok),
    ) {
        missing.push("linked_knowledge");
    }
    if !trio_ok(
        packet.linked_rejected.len(),
        packet.linked_rejected.iter().all(item_ok),
    ) {
        missing.push("linked_rejected");
    }
    if !list_present(&packet.expected_artifacts) {
        missing.push("expected_artifacts");
    }
    if !gates_present(&packet.before_start) {
        missing.push("before_start");
    }
    if !gates_present(&packet.before_complete) {
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

fn str_present(s: &str) -> bool {
    !s.trim().is_empty()
}
fn list_present(items: &[String]) -> bool {
    !items.is_empty() && items.iter().all(|s| str_present(s))
}
fn gates_present(items: &[Gate]) -> bool {
    !items.is_empty() && items.iter().all(|g| str_present(&g.rule))
}

/// Advisory fields that are empty (or carry a blank entry). They never make a
/// packet `NotReady`; callers surface them as warnings next to a `Ready`
/// verdict. Fields that are blocking under `strictness` are not repeated here.
pub fn advisory_warnings(packet: &ActionPacket, strictness: FujinStrictness) -> Vec<String> {
    let item_ok = |i: &LinkedItem| str_present(&i.id) && str_present(&i.label);
    let mut w = Vec::new();
    if !list_present(&packet.constraints) {
        w.push("constraints");
    }
    if !list_present(&packet.risks) {
        w.push("risks");
    }
    if packet.linked_decisions.is_empty() || !packet.linked_decisions.iter().all(item_ok) {
        w.push("linked_decisions");
    }
    if strictness == FujinStrictness::Soft {
        if packet.required_documents.is_empty() {
            w.push("required_documents");
        }
        if packet.linked_knowledge.is_empty() {
            w.push("linked_knowledge");
        }
        if packet.linked_rejected.is_empty() {
            w.push("linked_rejected");
        }
    }
    w.into_iter().map(String::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::{Gate, LinkedItem, RequiredDocument, TargetFiles};

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
            target_files: TargetFiles {
                owned: vec!["src/maturity.rs".into()],
                ..TargetFiles::default()
            },
            ..ActionPacket::default()
        }
    }

    #[test]
    fn full_packet_is_ready() {
        assert_eq!(assess(&full_packet()), Maturity::Ready);
    }

    #[test]
    fn empty_packet_lists_every_blocking_field() {
        match assess(&ActionPacket::default()) {
            Maturity::NotReady { missing } => {
                assert_eq!(
                    missing,
                    vec![
                        "goal",
                        "context",
                        "do_items",
                        "why",
                        "do_not",
                        "completion_criteria",
                        "dependencies",
                        "target_files.owned",
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
    fn malformed_optional_lineage_is_not_ready() {
        let mut p = full_packet();
        p.required_documents[0].uri = " ".into();
        p.linked_knowledge[0].id = " ".into();
        p.linked_rejected[0].label = " ".into();

        assert_eq!(
            assess(&p),
            Maturity::NotReady {
                missing: vec![
                    "required_documents".to_string(),
                    "linked_knowledge".to_string(),
                    "linked_rejected".to_string(),
                ]
            }
        );
    }

    #[test]
    fn optional_lineage_fields_may_be_empty() {
        let mut p = full_packet();
        p.required_documents.clear();
        p.linked_rejected.clear();

        assert_eq!(assess(&p), Maturity::Ready);
    }

    #[test]
    fn knowledge_lineage_may_be_empty_for_non_knowledge_sensing() {
        let mut p = full_packet();
        p.required_documents.clear();
        p.linked_knowledge.clear();
        p.linked_rejected.clear();

        assert_eq!(assess(&p), Maturity::Ready);
    }

    #[test]
    fn advisory_fields_warn_but_stay_ready() {
        let mut p = full_packet();
        p.constraints.clear();
        p.risks.clear();
        p.linked_decisions.clear();
        p.required_documents.clear();
        p.linked_knowledge.clear();
        p.linked_rejected.clear();

        assert_eq!(assess(&p), Maturity::Ready);
        assert_eq!(
            advisory_warnings(&p, FujinStrictness::Soft),
            vec![
                "constraints",
                "risks",
                "linked_decisions",
                "required_documents",
                "linked_knowledge",
                "linked_rejected"
            ]
        );
        // Strict makes the provenance trio blocking, so it is not a warning.
        assert_eq!(
            advisory_warnings(&p, FujinStrictness::Strict),
            vec!["constraints", "risks", "linked_decisions"]
        );
        assert!(advisory_warnings(&full_packet(), FujinStrictness::Soft).is_empty());
    }

    #[test]
    fn empty_target_files_owned_is_blocking() {
        let mut p = full_packet();
        p.target_files.owned.clear();
        assert_eq!(
            assess(&p),
            Maturity::NotReady {
                missing: vec!["target_files.owned".to_string()]
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
    // For each required §13 field: start from a fully-filled packet,
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
            "dependencies" => p.dependencies.clear(),
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
            "dependencies",
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

    // ── Strictness (provenance gate) ──────────────────────────────────────────

    fn packet_without_provenance() -> ActionPacket {
        let mut p = full_packet();
        p.required_documents.clear();
        p.linked_knowledge.clear();
        p.linked_rejected.clear();
        p
    }

    #[test]
    fn strict_requires_non_empty_required_documents() {
        let p = packet_without_provenance();
        assert_eq!(
            assess_with(&p, FujinStrictness::Strict),
            Maturity::NotReady {
                missing: vec![
                    "required_documents".to_string(),
                    "linked_knowledge".to_string(),
                    "linked_rejected".to_string(),
                ]
            }
        );
    }

    #[test]
    fn strict_names_each_empty_provenance_field_individually() {
        let mut p = full_packet();
        p.linked_knowledge.clear();
        assert_eq!(
            assess_with(&p, FujinStrictness::Strict),
            Maturity::NotReady {
                missing: vec!["linked_knowledge".to_string()]
            }
        );

        let mut p = full_packet();
        p.linked_rejected.clear();
        assert_eq!(
            assess_with(&p, FujinStrictness::Strict),
            Maturity::NotReady {
                missing: vec!["linked_rejected".to_string()]
            }
        );

        // A malformed provenance value is still reported under Strict.
        let mut p = full_packet();
        p.required_documents[0].uri = " ".into();
        assert_eq!(
            assess_with(&p, FujinStrictness::Strict),
            Maturity::NotReady {
                missing: vec!["required_documents".to_string()]
            }
        );
    }

    #[test]
    fn soft_keeps_provenance_optional() {
        assert_eq!(
            assess_with(&packet_without_provenance(), FujinStrictness::Soft),
            Maturity::Ready
        );
    }

    #[test]
    fn assess_defaults_to_soft_strictness() {
        assert_eq!(assess(&packet_without_provenance()), Maturity::Ready);
    }

    #[test]
    fn strict_full_packet_is_ready() {
        assert_eq!(
            assess_with(&full_packet(), FujinStrictness::Strict),
            Maturity::Ready
        );
    }
}
