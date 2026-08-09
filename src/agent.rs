//! Local domain primitives for the Actions → Daruma contract.
//!
//! Replaces `daruma_domain::{Actor, NewPlan, NewTask}` and
//! `daruma_shared::ProjectId` so the crate is dependency-free.
//! the host maps these onto the real daruma types when wiring the layer.

use serde::{Deserialize, Serialize};

use crate::packet::LinkedItem;

// ── ProjectId ─────────────────────────────────────────────────────────────────

layer_kit::newtype_id! {
    /// Strongly-typed UUIDv7 identifier for a Daruma project.
    pub struct ProjectId("prj");
}

// ── Actor ─────────────────────────────────────────────────────────────────────

/// The originating agent or user for a Daruma operation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Actor {
    pub kind: ActorKind,
    /// Opaque string identifier (user-id, agent-id, etc.).
    pub id: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    User,
    Agent,
}

impl Actor {
    pub fn user() -> Self {
        Self {
            kind: ActorKind::User,
            id: "user".into(),
        }
    }
    pub fn agent(id: impl Into<String>) -> Self {
        Self {
            kind: ActorKind::Agent,
            id: id.into(),
        }
    }
}

// ── NewPlan ───────────────────────────────────────────────────────────────────

/// Input for creating a plan in Daruma (mirrors daruma_domain::NewPlan).
/// the host maps this onto the real `NewPlan` when dispatching.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NewPlan {
    pub title: String,
    pub project_id: ProjectId,
    pub owner: Actor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success_criteria: Option<Vec<String>>,
    /// Provenance lowered from the Action Packet's §13 `linked_*` fields
    /// via `HandoffProject` (manifest §6, "родословная задач"). The
    /// `linked_*` fields are not part of the Daruma domain form; when lowering
    /// to execution, a host is expected to fold these references into
    /// `daruma_domain::NewPlan::source_brief` (`mcpbox-pipeline` does so).
    /// Actions guarantees only that they survive the actions→execution hop.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub linked_decisions: Vec<LinkedItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub linked_knowledge: Vec<LinkedItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub linked_rejected: Vec<LinkedItem>,
}

impl NewPlan {
    pub fn new(title: impl Into<String>, project_id: ProjectId, owner: Actor) -> Self {
        Self {
            title: title.into(),
            project_id,
            owner,
            goal: None,
            success_criteria: None,
            linked_decisions: Vec::new(),
            linked_knowledge: Vec::new(),
            linked_rejected: Vec::new(),
        }
    }
}

// ── NewTask ───────────────────────────────────────────────────────────────────

/// Input for creating a task in Daruma (mirrors daruma_domain::NewTask).
/// the host maps this onto the real `NewTask` when dispatching.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NewTask {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<ProjectId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl NewTask {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            project_id: None,
            description: None,
        }
    }
}
