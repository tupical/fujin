//! Actions-layer primitives — the maturity boundary between deliberation
//! and execution (manifest §6.5, §13, §16).
//!
//! The central object is the [`ActionPacket`]: a packet of work that has
//! ripened to the point of execution. Only a *mature* Action Packet
//! (see [`crate::maturity`]) may cross into TaskAgent. Raw material and
//! un-accepted decisions never reach the execution layer.

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
/// TaskAgent's `can_start` / `before_complete`). Free-form text — Actions
/// states the rule, TaskAgent enforces it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gate {
    pub rule: String,
}

/// The §13 Action Packet — the boundary object between the thinking
/// layers and execution.
///
/// Every field below is part of the §13 contract. Maturity
/// ([`crate::maturity::assess`]) is what decides whether a packet is
/// allowed to spawn work in TaskAgent; an under-filled packet is still a
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
}

impl ActionPacket {
    /// Pack a finished §15 [`PlanBrief`](planning_oss::PlanBrief) into a §13
    /// Action Packet, **preserving the plan's lineage**.
    ///
    /// This is the Planning→Actions seam of the MCPBox pipeline
    /// (Intake → Sensemaking → Decisions → Planning → **Actions** →
    /// TaskAgent). Its load-bearing job is provenance: every entry in
    /// `PlanBrief.decisions_made` is the `Display` id of a source
    /// [`Decision`](decisions_oss::Decision) (e.g. `dec_<uuid>`, set upstream
    /// by `PlanBrief::from_decisions`). Those ids are carried verbatim into
    /// [`linked_decisions`](ActionPacket::linked_decisions), so the full
    /// chain RawItem → SensingItem → Decision → PlanBrief → ActionPacket →
    /// TaskAgent stays auditable. A packet that forgot which decisions it
    /// realises could never be traced back, and lineage is the core value
    /// MCPBox preserves.
    ///
    /// ## Field mapping (§15 → §13)
    /// - `goal` → `goal`; `taskagent_target` → `context`;
    ///   `why_now` → `why`;
    /// - `in_scope` → `do_items`; `out_of_scope` → `do_not`;
    /// - `completion_criteria`, `constraints`, `risks`, `dependencies` →
    ///   their like-named fields;
    /// - `required_artifacts` → `required_documents`;
    /// - lineage links: `decisions_made` → `linked_decisions`,
    ///   `knowledge_base` → `linked_knowledge`,
    ///   `rejected_alternatives` → `linked_rejected`.
    ///
    /// ## What is *not* mapped
    /// §13 has three execution-only fields with no §15 counterpart —
    /// [`expected_artifacts`](ActionPacket::expected_artifacts),
    /// [`before_start`](ActionPacket::before_start), and
    /// [`before_complete`](ActionPacket::before_complete). They are left
    /// empty here: a plan brief states *what* to do and why, but the
    /// before/after execution gates and the concrete artifact list belong to
    /// Actions, not Planning. A brief-derived packet is therefore deliberately
    /// **not yet mature** (see [`crate::maturity::assess`]); the Actions layer
    /// fills those gates before the packet may cross into TaskAgent. This is
    /// the maturity boundary doing its job — converting a plan does not by
    /// itself license execution.
    pub fn from_brief(brief: &planning_oss::PlanBrief) -> ActionPacket {
        // Lineage helper: a brief carries only the *id* of each upstream
        // item (string form), which is exactly what we need for an auditable
        // back-reference. We use it as both id and label — the brief has no
        // richer label to offer, and Actions keeps links opaque (see
        // [`LinkedItem`]).
        let link = |id: &String| LinkedItem {
            id: id.clone(),
            label: id.clone(),
        };

        ActionPacket {
            goal: brief.goal.clone(),
            context: brief.taskagent_target.clone(),
            do_items: brief.in_scope.clone(),
            why: brief.why_now.clone().unwrap_or_default(),
            do_not: brief.out_of_scope.clone(),
            completion_criteria: brief.completion_criteria.clone(),
            constraints: brief.constraints.clone(),
            risks: brief.risks.clone(),
            dependencies: brief.dependencies.clone(),
            required_documents: brief
                .required_artifacts
                .iter()
                .map(|a| RequiredDocument {
                    title: a.clone(),
                    uri: a.clone(),
                })
                .collect(),
            // Provenance: the source Decision ids, kept verbatim so the
            // packet can always be traced back to the choices it realises.
            linked_decisions: brief.decisions_made.iter().map(link).collect(),
            linked_knowledge: brief.knowledge_base.iter().map(link).collect(),
            linked_rejected: brief.rejected_alternatives.iter().map(link).collect(),
            // Execution-only §13 fields — no §15 source, filled by Actions.
            expected_artifacts: Vec::new(),
            before_start: Vec::new(),
            before_complete: Vec::new(),
        }
    }
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
/// TaskAgent task: it lives in Actions until a [`HandoffPacket`] commits
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

/// The committed crossing into TaskAgent (§6.5, §16). Produced only from
/// a mature Action Packet; carries the project/plan/task shape the
/// execution layer will materialize. A packet may yield one, several, or
/// zero projects (§16) — hence `projects` is a list and may be empty
/// (the "не породить проект" outcome).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffPacket {
    /// Projects to create in TaskAgent (0, 1, or many — §16).
    pub projects: Vec<HandoffProject>,
}

/// A single project's worth of execution work, shaped to TaskAgent's
/// `NewPlan` / `NewTask` contract (title + goal + success_criteria; tasks
/// with title + description).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffProject {
    pub project_title: String,
    pub plan_title: String,
    pub goal: String,
    pub success_criteria: Vec<String>,
    pub tasks: Vec<TaskCandidate>,
}
