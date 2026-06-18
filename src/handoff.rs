//! The crossing into TaskAgent (manifest §16).
//!
//! A [`HandoffPacket`] may only be built from a *mature* Action Packet
//! ([`crate::maturity`]), and each [`HandoffProject`] it carries lowers
//! onto TaskAgent's real intake contract — [`NewPlan`] + [`NewTask`] from
//! the `domain` crate. This is where Actions hands off to execution; raw
//! material never reaches here.

use taskagent_domain::{Actor, NewPlan, NewTask};
use taskagent_shared::{CoreError, ProjectId};

use crate::maturity::assess;
use crate::packet::{ActionPacket, HandoffPacket, HandoffProject};

/// Build a [`HandoffPacket`] from a packet's projects, refusing if the
/// packet is not yet mature (§16: "хотя бы один готовый action packet").
///
/// `projects` may be empty — that is the legitimate "одна идея может не
/// породить проект" outcome (§16). Maturity is still required: even the
/// zero-project handoff is a decision that the packet has ripened.
pub fn to_handoff(
    packet: &ActionPacket,
    projects: Vec<HandoffProject>,
) -> Result<HandoffPacket, CoreError> {
    match assess(packet) {
        m if m.is_ready() => Ok(HandoffPacket { projects }),
        crate::maturity::Maturity::NotReady { missing } => Err(CoreError::Validation(format!(
            "action packet is not mature; missing §13 fields: {}",
            missing.join(", ")
        ))),
        // `is_ready()` already matched the Ready arm above.
        crate::maturity::Maturity::Ready => unreachable!(),
    }
}

/// Lower one [`HandoffProject`] onto the core intake contract: a
/// [`NewPlan`] plus the [`NewTask`] rows it owns. The caller dispatches
/// these against TaskAgent (project create → plan create → add tasks);
/// Actions never writes to storage itself.
pub fn into_new_plan(project: &HandoffProject, project_id: ProjectId, owner: Actor) -> NewPlanWithTasks {
    let mut plan = NewPlan::new(project.plan_title.clone(), project_id, owner);
    plan.goal = Some(project.goal.clone());
    plan.success_criteria = Some(project.success_criteria.clone());

    let tasks = project
        .tasks
        .iter()
        .map(|c| {
            let mut t = NewTask::new(c.title.clone());
            t.project_id = Some(project_id);
            t.description = Some(c.description.clone());
            t
        })
        .collect();

    NewPlanWithTasks { plan, tasks }
}

/// A lowered project: the core `NewPlan` and the `NewTask` rows the
/// caller dispatches against TaskAgent.
pub struct NewPlanWithTasks {
    pub plan: NewPlan,
    pub tasks: Vec<NewTask>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::{Gate, LinkedItem, RequiredDocument, TaskCandidate};

    fn mature_packet() -> ActionPacket {
        let item = |s: &str| LinkedItem {
            id: s.into(),
            label: s.into(),
        };
        ActionPacket {
            goal: "g".into(),
            context: "c".into(),
            do_items: vec!["d".into()],
            why: "w".into(),
            do_not: vec!["n".into()],
            completion_criteria: vec!["cc".into()],
            constraints: vec!["co".into()],
            risks: vec!["r".into()],
            dependencies: vec!["dep".into()],
            required_documents: vec![RequiredDocument {
                title: "t".into(),
                uri: "u".into(),
            }],
            linked_decisions: vec![item("dec")],
            linked_knowledge: vec![item("kn")],
            linked_rejected: vec![item("rej")],
            expected_artifacts: vec!["a".into()],
            before_start: vec![Gate { rule: "bs".into() }],
            before_complete: vec![Gate { rule: "bc".into() }],
        }
    }

    #[test]
    fn immature_packet_is_refused() {
        let err = to_handoff(&ActionPacket::default(), vec![]).unwrap_err();
        assert!(matches!(err, CoreError::Validation(_)));
    }

    #[test]
    fn mature_packet_with_zero_projects_is_valid() {
        // §16: "одна идея может не породить проект".
        let h = to_handoff(&mature_packet(), vec![]).unwrap();
        assert!(h.projects.is_empty());
    }

    #[test]
    fn project_lowers_onto_core_contract() {
        let project = HandoffProject {
            project_title: "Core".into(),
            plan_title: "Build it".into(),
            goal: "ship".into(),
            success_criteria: vec!["green".into()],
            tasks: vec![TaskCandidate {
                title: "task A".into(),
                description: "do A".into(),
            }],
        };
        let pid = ProjectId::new();
        let lowered = into_new_plan(&project, pid, Actor::user());
        assert_eq!(lowered.plan.goal.as_deref(), Some("ship"));
        assert_eq!(lowered.plan.success_criteria, Some(vec!["green".into()]));
        assert_eq!(lowered.tasks.len(), 1);
        assert_eq!(lowered.tasks[0].title, "task A");
        assert_eq!(lowered.tasks[0].project_id, Some(pid));
    }
}
