//! Actions-layer primitives — the maturity boundary between deliberation
//! and execution (manifest §6.5, §13, §16).
//!
//! The central object is the [`ActionPacket`]: a packet of work that has
//! ripened to the point of execution. Only a *mature* Action Packet
//! (see [`crate::maturity`]) may cross into TaskAgent. Raw material and
//! un-accepted decisions never reach the execution layer.
//!
//! # Note on `from_brief`
//! The adapter `ActionPacket::from_brief(PlanBrief)` that previously
//! consumed `planning_oss::PlanBrief` has been removed from this crate.
//! It moves to mcpbox, which owns the cross-layer wiring and may build an
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
