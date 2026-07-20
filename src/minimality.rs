//! Self-contained Ponytail minimality validator for code action packets.
//!
//! This module is intentionally pure: no I/O, no graph access, no packet
//! wiring. It validates the minimality metadata shape that future ActionPacket
//! wiring can carry.

/// Minimality enforcement mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MinimalityPolicy {
    Off,
    Advisory,
    Enforced,
    /// Reserved for stricter future policy. Today it is equivalent to
    /// [`MinimalityPolicy::Enforced`].
    Strict,
}

/// Ponytail ladder answers for a code action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinimalityCheck {
    pub need_exists: bool,
    pub deletion_possible: bool,
    pub stdlib_candidate: Option<String>,
    pub native_candidate: Option<String>,
    pub installed_dependency_candidate: Option<String>,
    pub new_dependency_required: bool,
    pub new_dependency_reason: Option<String>,
    pub one_line_possible: bool,
    pub chosen_smallest_working_approach: String,
}

/// Requirements that minimality must not cut.
///
/// `true` means the requirement is addressed or not affected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectedRequirements {
    pub validation: bool,
    pub data_loss_handling: bool,
    pub security: bool,
    pub accessibility: bool,
    pub explicit_user_requirements: bool,
}

/// Deliberate shortcut marker with a known ceiling and optional upgrade trigger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebtMarker {
    pub id: String,
    pub ceiling: String,
    pub upgrade_trigger: Option<String>,
}

/// Smallest runnable check considered sufficient for this action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceCheck {
    pub required_check: String,
    pub why_sufficient: String,
}

/// Dependency decision attached to a new dependency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyJustification {
    pub dependency: String,
    pub reason: String,
}

/// Minimality metadata carried by a code action packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeActionMinimality {
    pub policy: MinimalityPolicy,
    pub is_nontrivial: bool,
    pub check: Option<MinimalityCheck>,
    pub protected: Option<ProtectedRequirements>,
    pub dependency_justifications: Vec<DependencyJustification>,
    pub debt_markers: Vec<DebtMarker>,
    pub evidence: Option<EvidenceCheck>,
}

/// Result returned by [`check_minimality`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MinimalityVerdict {
    Passed,
    /// Advisory or soft-policy findings. `notes` lists stable W/M codes in
    /// canonical order.
    Warning {
        notes: Vec<String>,
    },
    /// Hard policy failures. `failed` lists stable M-codes in canonical order.
    Blocked {
        failed: Vec<String>,
    },
}

/// M2: enforced code packets must carry a minimality check.
pub const M2_NO_MINIMALITY_CHECK: &str = "M2_no_minimality_check";
/// M3: a new dependency needs both a local reason and a dependency decision.
pub const M3_NEW_DEP_UNJUSTIFIED: &str = "M3_new_dep_unjustified";
/// M4: validation, data-loss handling, security, accessibility, and explicit
/// user requirements are protected from minimality cuts.
pub const M4_PROTECTED_REQUIREMENT_CUT: &str = "M4_protected_requirement_cut";
/// M5: non-trivial logic needs one runnable evidence check.
pub const M5_NONTRIVIAL_NO_EVIDENCE: &str = "M5_nontrivial_no_evidence";
/// M6: a debt marker needs a ceiling that states where the shortcut stops.
pub const M6_DEBT_MARKER_NO_CEILING: &str = "M6_debt_marker_no_ceiling";
/// W1: a debt marker without an upgrade trigger is trackable but incomplete.
pub const W1_DEBT_MARKER_NO_UPGRADE_TRIGGER: &str = "W1_debt_marker_no_upgrade_trigger";
/// W2: ladder rung 1 was not established; the work may not need to exist.
pub const W2_NEED_NOT_ESTABLISHED: &str = "W2_need_not_established";

/// Validate code-action minimality metadata.
///
/// `Off` always passes. `Advisory` never blocks: hard and soft findings are
/// returned as warnings. `Enforced` and `Strict` block on hard M-codes and warn
/// only when soft W-codes remain.
pub fn check_minimality(input: &CodeActionMinimality) -> MinimalityVerdict {
    match input.policy {
        MinimalityPolicy::Off => MinimalityVerdict::Passed,
        MinimalityPolicy::Advisory => warning_or_pass(all_findings(input)),
        MinimalityPolicy::Enforced => enforced_verdict(input),
        MinimalityPolicy::Strict => enforced_verdict(input),
    }
}

fn enforced_verdict(input: &CodeActionMinimality) -> MinimalityVerdict {
    let failed = hard_failures(input);
    if !failed.is_empty() {
        return MinimalityVerdict::Blocked { failed };
    }

    warning_or_pass(soft_warnings(input))
}

fn all_findings(input: &CodeActionMinimality) -> Vec<String> {
    let mut notes = hard_failures(input);
    notes.extend(soft_warnings(input));
    notes
}

fn warning_or_pass(notes: Vec<String>) -> MinimalityVerdict {
    if notes.is_empty() {
        MinimalityVerdict::Passed
    } else {
        MinimalityVerdict::Warning { notes }
    }
}

fn hard_failures(input: &CodeActionMinimality) -> Vec<String> {
    let mut failed = Vec::new();

    match &input.check {
        Some(check) => {
            if check.new_dependency_required
                && (is_blank_option(&check.new_dependency_reason)
                    || !has_dependency_justification(&input.dependency_justifications))
            {
                failed.push(M3_NEW_DEP_UNJUSTIFIED.into());
            }
        }
        None => failed.push(M2_NO_MINIMALITY_CHECK.into()),
    }

    match &input.protected {
        Some(protected) => {
            if !protected.validation
                || !protected.data_loss_handling
                || !protected.security
                || !protected.accessibility
                || !protected.explicit_user_requirements
            {
                failed.push(M4_PROTECTED_REQUIREMENT_CUT.into());
            }
        }
        None => failed.push(M4_PROTECTED_REQUIREMENT_CUT.into()),
    }

    if input.is_nontrivial {
        match &input.evidence {
            Some(evidence) if !is_blank(&evidence.required_check) => {}
            Some(_evidence) => failed.push(M5_NONTRIVIAL_NO_EVIDENCE.into()),
            None => failed.push(M5_NONTRIVIAL_NO_EVIDENCE.into()),
        }
    }

    if input
        .debt_markers
        .iter()
        .any(|marker| is_blank(&marker.ceiling))
    {
        failed.push(M6_DEBT_MARKER_NO_CEILING.into());
    }

    failed
}

fn soft_warnings(input: &CodeActionMinimality) -> Vec<String> {
    let mut notes = Vec::new();

    if input
        .debt_markers
        .iter()
        .any(|marker| is_blank_option(&marker.upgrade_trigger))
    {
        notes.push(W1_DEBT_MARKER_NO_UPGRADE_TRIGGER.into());
    }

    if matches!(&input.check, Some(check) if !check.need_exists) {
        notes.push(W2_NEED_NOT_ESTABLISHED.into());
    }

    notes
}

fn has_dependency_justification(justifications: &[DependencyJustification]) -> bool {
    justifications.iter().any(|justification| {
        !is_blank(&justification.dependency) && !is_blank(&justification.reason)
    })
}

fn is_blank_option(value: &Option<String>) -> bool {
    match value {
        Some(value) => is_blank(value),
        None => true,
    }
}

fn is_blank(value: &str) -> bool {
    value.trim().is_empty()
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;

    fn valid_minimality() -> CodeActionMinimality {
        CodeActionMinimality {
            policy: MinimalityPolicy::Enforced,
            is_nontrivial: true,
            check: Some(MinimalityCheck {
                need_exists: true,
                deletion_possible: false,
                stdlib_candidate: None,
                native_candidate: None,
                installed_dependency_candidate: None,
                new_dependency_required: false,
                new_dependency_reason: None,
                one_line_possible: false,
                chosen_smallest_working_approach: "small direct implementation".into(),
            }),
            protected: Some(ProtectedRequirements {
                validation: true,
                data_loss_handling: true,
                security: true,
                accessibility: true,
                explicit_user_requirements: true,
            }),
            dependency_justifications: Vec::new(),
            debt_markers: vec![DebtMarker {
                id: "shortcut-1".into(),
                ceiling: "fine under 1k records".into(),
                upgrade_trigger: Some("add index past 1k records".into()),
            }],
            evidence: Some(EvidenceCheck {
                required_check: "cargo test -p mcpbox-pipeline".into(),
                why_sufficient: "covers the validator branch table".into(),
            }),
        }
    }

    #[test]
    fn passed_when_all_good_enforced() {
        assert_eq!(
            check_minimality(&valid_minimality()),
            MinimalityVerdict::Passed
        );
    }

    #[test]
    fn advisory_never_blocks() {
        let mut input = valid_minimality();
        input.policy = MinimalityPolicy::Advisory;
        input.check = None;

        assert_eq!(
            check_minimality(&input),
            MinimalityVerdict::Warning {
                notes: vec![M2_NO_MINIMALITY_CHECK.into()]
            }
        );
    }

    #[test]
    fn blocked_M2_missing_check_enforced() {
        let mut input = valid_minimality();
        input.check = None;

        assert_eq!(
            check_minimality(&input),
            MinimalityVerdict::Blocked {
                failed: vec![M2_NO_MINIMALITY_CHECK.into()]
            }
        );
    }

    #[test]
    fn blocked_M3_new_dep_without_justification() {
        let mut input = valid_minimality();
        input.check.as_mut().unwrap().new_dependency_required = true;
        input.check.as_mut().unwrap().new_dependency_reason = Some("needed for parsing".into());

        assert_eq!(
            check_minimality(&input),
            MinimalityVerdict::Blocked {
                failed: vec![M3_NEW_DEP_UNJUSTIFIED.into()]
            }
        );
    }

    #[test]
    fn blocked_M4_protected_requirement_cut() {
        let mut input = valid_minimality();
        input.protected.as_mut().unwrap().security = false;

        assert_eq!(
            check_minimality(&input),
            MinimalityVerdict::Blocked {
                failed: vec![M4_PROTECTED_REQUIREMENT_CUT.into()]
            }
        );
    }

    #[test]
    fn blocked_M5_nontrivial_without_evidence() {
        let mut input = valid_minimality();
        input.evidence = None;

        assert_eq!(
            check_minimality(&input),
            MinimalityVerdict::Blocked {
                failed: vec![M5_NONTRIVIAL_NO_EVIDENCE.into()]
            }
        );
    }

    #[test]
    fn blocked_M6_debt_marker_empty_ceiling() {
        let mut input = valid_minimality();
        input.debt_markers[0].ceiling.clear();

        assert_eq!(
            check_minimality(&input),
            MinimalityVerdict::Blocked {
                failed: vec![M6_DEBT_MARKER_NO_CEILING.into()]
            }
        );
    }

    #[test]
    fn warning_W1_debt_marker_without_upgrade_trigger() {
        let mut input = valid_minimality();
        input.debt_markers[0].upgrade_trigger = None;

        assert_eq!(
            check_minimality(&input),
            MinimalityVerdict::Warning {
                notes: vec![W1_DEBT_MARKER_NO_UPGRADE_TRIGGER.into()]
            }
        );
    }

    #[test]
    fn off_policy_always_passed() {
        let mut input = valid_minimality();
        input.policy = MinimalityPolicy::Off;
        input.check = None;
        input.protected = None;
        input.evidence = None;
        input.debt_markers[0].ceiling.clear();

        assert_eq!(check_minimality(&input), MinimalityVerdict::Passed);
    }
}
