//! Local domain primitives for the Actions → TaskAgent contract.
//!
//! Replaces `taskagent_domain::{Actor, NewPlan, NewTask}` and
//! `taskagent_shared::ProjectId` so the crate is dependency-free.
//! mcpbox maps these onto the real taskagent types when wiring the layer.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── ProjectId ─────────────────────────────────────────────────────────────────

/// Strongly-typed UUIDv7 identifier for a TaskAgent project.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProjectId(pub Uuid);

impl ProjectId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for ProjectId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ProjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "prj_{}", self.0)
    }
}

// ── Actor ─────────────────────────────────────────────────────────────────────

/// The originating agent or user for a TaskAgent operation.
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

/// Input for creating a plan in TaskAgent (mirrors taskagent_domain::NewPlan).
/// mcpbox maps this onto the real `NewPlan` when dispatching.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NewPlan {
    pub title: String,
    pub project_id: ProjectId,
    pub owner: Actor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success_criteria: Option<Vec<String>>,
}

impl NewPlan {
    pub fn new(title: impl Into<String>, project_id: ProjectId, owner: Actor) -> Self {
        Self {
            title: title.into(),
            project_id,
            owner,
            goal: None,
            success_criteria: None,
        }
    }
}

// ── NewTask ───────────────────────────────────────────────────────────────────

/// Input for creating a task in TaskAgent (mirrors taskagent_domain::NewTask).
/// mcpbox maps this onto the real `NewTask` when dispatching.
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
