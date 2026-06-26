//! The crossing into Daruma (manifest §16).
//!
//! A [`HandoffPacket`] may only be built from a *mature* Action Packet
//! ([`crate::maturity`]), and each [`HandoffProject`] it carries lowers
//! onto Daruma's local intake contract — [`NewPlan`] + [`NewTask`] from
//! the local `agent` module. This is where Actions hands off to execution;
//! raw material never reaches here.
//!
//! the host maps [`NewPlan`] / [`NewTask`] onto the real daruma types when
//! dispatching — Actions itself never writes to storage.

use crate::agent::{Actor, NewPlan, NewTask, ProjectId};
use crate::error::ActionsError;
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
) -> Result<HandoffPacket, ActionsError> {
    match assess(packet) {
        m if m.is_ready() => Ok(HandoffPacket { projects }),
        crate::maturity::Maturity::NotReady { missing } => {
            Err(ActionsError::validation(format!(
                "action packet is not mature; missing §13 fields: {}",
                missing.join(", ")
            )))
        }
        // `is_ready()` already matched the Ready arm above.
        crate::maturity::Maturity::Ready => unreachable!(),
    }
}

/// Lower one [`HandoffProject`] onto the local intake contract: a
/// [`NewPlan`] plus the [`NewTask`] rows it owns. The caller (the host)
/// dispatches these against Daruma; Actions never writes to storage itself.
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

/// A lowered project: the local `NewPlan` and the `NewTask` rows the
/// caller dispatches against Daruma.
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
        assert!(matches!(err, ActionsError::Validation(_)));
    }

    #[test]
    fn mature_packet_with_zero_projects_is_valid() {
        // §16: "одна идея может не породить проект".
        let h = to_handoff(&mature_packet(), vec![]).unwrap();
        assert!(h.projects.is_empty());
    }

    #[test]
    fn brief_plus_execution_gates_hands_off_to_new_plan() {
        // A fully-filled packet hands off and lowers to NewPlan/NewTask.
        let mut packet = mature_packet();
        packet.goal = "Ship the Planning→Actions seam".into();
        packet.context = "fujin project".into();
        packet.why = "Wave-2 wires the pipeline".into();
        packet.do_items = vec!["packet.rs refactor".into()];
        packet.constraints = vec!["no ai-infra in Actions".into()];
        packet.linked_knowledge = vec![LinkedItem { id: "kn_1".into(), label: "kn_1".into() }];
        packet.linked_rejected = vec![LinkedItem { id: "rej_1".into(), label: "rej_1".into() }];
        packet.linked_decisions = vec![
            LinkedItem { id: "dec_aaaa".into(), label: "dec_aaaa".into() },
            LinkedItem { id: "dec_bbbb".into(), label: "dec_bbbb".into() },
        ];
        packet.expected_artifacts = vec!["from_brief() + tests".into()];
        packet.before_start = vec![Gate { rule: "brief is ready".into() }];
        packet.before_complete = vec![Gate { rule: "cargo test green".into() }];

        let ids: Vec<&str> = packet.linked_decisions.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ids, vec!["dec_aaaa", "dec_bbbb"]);

        let project = HandoffProject {
            project_title: "fujin".into(),
            plan_title: "Wire the seam".into(),
            goal: packet.goal.clone(),
            success_criteria: packet.completion_criteria.clone(),
            tasks: vec![TaskCandidate {
                title: "implement from_brief".into(),
                description: "map §15 → §13, keep lineage".into(),
            }],
        };
        let handoff = to_handoff(&packet, vec![project]).expect("mature packet hands off");
        assert_eq!(handoff.projects.len(), 1);

        let lowered = into_new_plan(&handoff.projects[0], ProjectId::new(), Actor::user());
        assert_eq!(lowered.plan.goal.as_deref(), Some("Ship the Planning→Actions seam"));
        assert_eq!(lowered.tasks.len(), 1);
        assert_eq!(lowered.tasks[0].title, "implement from_brief");
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
