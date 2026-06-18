//! `actions-oss` — the Actions layer of the MCPBox family: the maturity
//! boundary between deliberation and execution (manifest §6.5, §13, §16).
//!
//! Where `intake-oss` and `planning-oss` are *AI operation* crates built
//! on `taskagent-ai-infra`, Actions is a pure **domain** layer. It owns
//! the primitives that decide whether work has ripened to the point of
//! execution — and only then lets it cross into TaskAgent. Raw material
//! and un-accepted decisions never reach the execution layer.
//!
//! # Primitives (§6.5)
//! - [`ActionPacket`] — the §13 boundary object, with every required
//!   field (goal/context/do/why/do-not/criteria/constraints/risks/
//!   dependencies/required docs/linked decisions+knowledge+rejected/
//!   expected artifacts/before-start+before-complete gates).
//! - [`WorkOrder`], [`TaskCandidate`], [`ExecutionStep`] — the units a
//!   packet fans out into.
//! - [`HandoffPacket`] — the committed crossing into TaskAgent.
//!
//! # Contract
//! - Actions **never** writes to storage. [`handoff::into_new_plan`]
//!   lowers a project onto the core [`taskagent_domain::NewPlan`] /
//!   [`taskagent_domain::NewTask`] contract; the caller dispatches it.
//! - The maturity check ([`maturity::assess`]) is **deterministic**:
//!   the same packet always yields the same verdict.
//! - Errors propagate as [`taskagent_shared::CoreError`].

pub mod handoff;
pub mod maturity;
pub mod packet;

pub use handoff::{into_new_plan, to_handoff, NewPlanWithTasks};
pub use maturity::{assess, Maturity};
pub use packet::{
    ActionPacket, ExecutionStep, Gate, HandoffPacket, HandoffProject, LinkedItem, RequiredDocument,
    TaskCandidate, WorkOrder,
};
