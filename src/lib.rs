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
//! - Actions **never** writes to storage. [`handoff::into_new_plan`]
//!   lowers a project onto the local [`agent::NewPlan`] / [`agent::NewTask`]
//!   contract; the host maps those onto the real daruma types and dispatches.
//! - The maturity check ([`maturity::assess`]) is **deterministic**:
//!   the same packet always yields the same verdict.
//! - Errors propagate as [`error::ActionsError`].
//!
//! # Note on `ActionPacket::from_brief`
//! The Planning→Actions adapter (`from_brief(PlanBrief)`) has moved to
//! the host, which owns cross-layer wiring. It is not part of this skeleton.

pub mod agent;
pub mod error;
pub mod handoff;
pub mod maturity;
pub mod packet;

pub use agent::{Actor, ActorKind, NewPlan, NewTask, ProjectId};
pub use error::ActionsError;
pub use handoff::{into_new_plan, to_handoff, NewPlanWithTasks};
pub use maturity::{assess, Maturity};
pub use packet::{
    ActionPacket, ExecutionStep, Gate, HandoffPacket, HandoffProject, LinkedItem, RequiredDocument,
    TargetFiles, TaskCandidate, WorkOrder,
};
