//! Actions-layer primitives — the maturity boundary between deliberation
//! and execution (manifest §6.5, §13, §16).
//!
//! The central object is the [`ActionPacket`]: a packet of work that has
//! ripened to the point of execution. Only a *mature* Action Packet
//! (see [`crate::maturity`]) may cross into Daruma. Raw material and
//! un-accepted decisions never reach the execution layer.
//!
//! # Note on `from_brief`
//! The adapter `ActionPacket::from_brief(PlanBrief)` that previously
//! consumed `planning_oss::PlanBrief` has been removed from this crate.
//! It moves to the host, which owns the cross-layer wiring and may build an
//! `ActionPacket` from a `PlanBrief` using the public fields below.

use serde::{Deserialize, Serialize};

/// A document the executor must have on hand before/while working
/// ("обязательные документы", §13). The `uri` points at the source of
/// truth; `title` is a human label.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequiredDocument {
    pub title: String,
    pub uri: String,
}

/// A typed back-reference into the upper layers — the provenance of this
/// packet (manifest §6, "родословная задач"). Kept as opaque ids/labels
/// so Actions never has to model Decisions/Sensemaking internals.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkedItem {
    /// Stable id in the originating layer (decision id, knowledge id, …).
    pub id: String,
    /// Human label for the link.
    pub label: String,
}

/// A guard that must hold before a step may start or be considered
/// complete (§13 "правила перед стартом / перед завершением"; maps onto
/// Daruma's `can_start` / `before_complete`). Free-form text — Actions
/// states the rule, Daruma enforces it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gate {
    pub rule: String,
}

/// Scoped write authority handed to the executor: which paths it owns,
/// which it may only read, and which are explicitly off-limits. Part of
/// the bounded execution envelope, not the §13 required contract — an
/// empty [`TargetFiles`] means the envelope was not set, and unscoped
/// packets are unaffected.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TargetFiles {
    /// Paths the executor owns and may write.
    pub owned: Vec<String>,
    /// Paths the executor may read but not write.
    pub read_only: Vec<String>,
    /// Paths explicitly out of bounds for this packet.
    pub forbidden: Vec<String>,
}

impl TargetFiles {
    fn is_empty(&self) -> bool {
        self.owned.is_empty() && self.read_only.is_empty() && self.forbidden.is_empty()
    }
}

/// The §13 Action Packet — the boundary object between the thinking
/// layers and execution.
///
/// Every field below is part of the §13 contract. Maturity
/// ([`crate::maturity::assess`]) is what decides whether a packet is
/// allowed to spawn work in Daruma; an under-filled packet is still a
/// valid value, just `not_ready`.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ActionPacket {
    /// цель — what outcome this packet exists to produce.
    pub goal: String,
    /// контекст — situational background the executor needs.
    pub context: String,
    /// что нужно сделать — the concrete work to perform.
    pub do_items: Vec<String>,
    /// почему это нужно — the rationale.
    pub why: String,
    /// что не нужно делать — explicit non-goals / out-of-scope.
    pub do_not: Vec<String>,
    /// критерии завершения — verifiable completion criteria.
    pub completion_criteria: Vec<String>,
    /// ограничения — constraints the work must respect.
    pub constraints: Vec<String>,
    /// риски — known risks.
    pub risks: Vec<String>,
    /// зависимости — prerequisites / blockers.
    pub dependencies: Vec<String>,
    /// обязательные документы — required reference material.
    pub required_documents: Vec<RequiredDocument>,
    /// связанные решения — decisions this packet implements.
    pub linked_decisions: Vec<LinkedItem>,
    /// связанные знания — knowledge this packet draws on.
    pub linked_knowledge: Vec<LinkedItem>,
    /// связанные отбракованные варианты — rejected alternatives, kept as
    /// first-class objects (§ "Отбракованные идеи как первый класс").
    pub linked_rejected: Vec<LinkedItem>,
    /// ожидаемые артефакты — what the work should produce.
    pub expected_artifacts: Vec<String>,
    /// правила перед стартом — gates checked before work begins.
    pub before_start: Vec<Gate>,
    /// правила перед завершением — gates checked before completion.
    pub before_complete: Vec<Gate>,

    // ── Bounded execution envelope ──────────────────────────────────────
    // Scoping added on top of the §13 contract above. Not required fields:
    // `maturity::assess` does not check them, and an unset envelope keeps
    // old JSON payloads and old consumers working unchanged.
    /// границы правки — which paths the executor owns, may read, or must
    /// not touch.
    #[serde(default, skip_serializing_if = "TargetFiles::is_empty")]
    pub target_files: TargetFiles,
    /// политика при конфликте — how to resolve a conflicting concurrent
    /// edit to `target_files.owned`. Free-form text — Actions states the
    /// policy, the host/Daruma enforces it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflict_policy: Option<String>,
    /// обязательные проверки — checks (tests, lints, gates) that must
    /// pass before this packet's work counts as complete.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_checks: Vec<String>,
    /// профиль ревьюера — the reviewer profile/persona prescribed for
    /// this packet's output, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer_profile: Option<String>,
}

/// A single executable unit carved out of an Action Packet (§6.5). One
/// packet fans out into one or more work orders.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkOrder {
    pub title: String,
    pub instructions: String,
    pub completion_criteria: Vec<String>,
}

/// A proposed-but-not-yet-committed task (§6.5). Distinct from a
/// Daruma task: it lives in Actions until a [`HandoffPacket`] commits
/// it, so a candidate can still be dropped without polluting execution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskCandidate {
    pub title: String,
    pub description: String,
}

/// One ordered step within a [`WorkOrder`] (§6.5).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionStep {
    pub order: u32,
    pub action: String,
}

/// The committed crossing into Daruma (§6.5, §16). Produced only from
/// a mature Action Packet; carries the project/plan/task shape the
/// execution layer will materialize. A packet may yield one, several, or
/// zero projects (§16) — hence `projects` is a list and may be empty
/// (the "не породить проект" outcome).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffPacket {
    /// Projects to create in Daruma (0, 1, or many — §16).
    pub projects: Vec<HandoffProject>,
}

/// A single project's worth of execution work, shaped to Daruma's
/// `NewPlan` / `NewTask` contract (title + goal + success_criteria; tasks
/// with title + description).
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HandoffProject {
    pub project_title: String,
    pub plan_title: String,
    pub goal: String,
    pub success_criteria: Vec<String>,
    pub tasks: Vec<TaskCandidate>,
    /// Provenance carried over from the Action Packet's §13 `linked_*`
    /// fields (manifest §6, "родословная задач"), scoped to this project.
    /// Not part of the maturity contract — the packet is already required
    /// to be mature before `to_handoff` runs — but dropping it here would
    /// sever lineage at the actions→execution boundary, which is the whole
    /// point of carrying it this far.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub linked_decisions: Vec<LinkedItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub linked_knowledge: Vec<LinkedItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub linked_rejected: Vec<LinkedItem>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pre-envelope JSON (no `target_files`/`conflict_policy`/
    /// `required_checks`/`reviewer_profile` keys at all) must still
    /// deserialize, filling the envelope with its empty defaults.
    #[test]
    fn old_format_json_deserializes_with_empty_envelope_defaults() {
        let old_json = r#"{
            "goal": "g",
            "context": "c",
            "do_items": ["d"],
            "why": "w",
            "do_not": ["n"],
            "completion_criteria": ["cc"],
            "constraints": ["co"],
            "risks": ["r"],
            "dependencies": ["dep"],
            "required_documents": [],
            "linked_decisions": [],
            "linked_knowledge": [],
            "linked_rejected": [],
            "expected_artifacts": [],
            "before_start": [],
            "before_complete": []
        }"#;

        let packet: ActionPacket = serde_json::from_str(old_json).expect("old-format JSON parses");
        assert_eq!(packet.target_files, TargetFiles::default());
        assert_eq!(packet.conflict_policy, None);
        assert!(packet.required_checks.is_empty());
        assert_eq!(packet.reviewer_profile, None);
    }

    /// An envelope-less packet serializes without the new keys at all,
    /// so old consumers parsing the JSON see exactly the old shape.
    #[test]
    fn empty_envelope_is_omitted_from_serialized_json() {
        let packet = ActionPacket::default();
        let value = serde_json::to_value(&packet).expect("serializes");
        let obj = value.as_object().expect("packet serializes to an object");
        assert!(!obj.contains_key("target_files"));
        assert!(!obj.contains_key("conflict_policy"));
        assert!(!obj.contains_key("required_checks"));
        assert!(!obj.contains_key("reviewer_profile"));
    }

    /// Round-trip with the envelope fully populated.
    #[test]
    fn populated_envelope_round_trips() {
        let packet = ActionPacket {
            target_files: TargetFiles {
                owned: vec!["src/packet.rs".into()],
                read_only: vec!["src/lib.rs".into()],
                forbidden: vec!["Cargo.toml".into()],
            },
            conflict_policy: Some("last-writer-wins".into()),
            required_checks: vec!["cargo test".into()],
            reviewer_profile: Some("rust-reviewer".into()),
            ..ActionPacket::default()
        };

        let json = serde_json::to_string(&packet).expect("serializes");
        let round_tripped: ActionPacket = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(round_tripped, packet);
    }

    /// Pre-lineage JSON (no `linked_decisions`/`linked_knowledge`/
    /// `linked_rejected` keys) must still deserialize, filling the
    /// provenance with empty defaults — same back-compat contract as the
    /// bounded execution envelope above.
    #[test]
    fn old_format_handoff_project_json_deserializes_with_empty_links() {
        let old_json = r#"{
            "project_title": "Core",
            "plan_title": "Build it",
            "goal": "ship",
            "success_criteria": ["green"],
            "tasks": []
        }"#;

        let project: HandoffProject =
            serde_json::from_str(old_json).expect("old-format JSON parses");
        assert!(project.linked_decisions.is_empty());
        assert!(project.linked_knowledge.is_empty());
        assert!(project.linked_rejected.is_empty());
    }
}
