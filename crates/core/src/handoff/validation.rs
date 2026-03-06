use std::collections::{HashMap, HashSet};

use super::execution::{is_material_work_package, unresolved_failed_commands};
use super::{HandoffSummary, OrderedStep, WorkPackage};

#[derive(Debug, Clone, serde::Serialize)]
pub struct ValidationFinding {
    pub code: String,
    pub severity: String,
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct HandoffValidationReport {
    pub session_id: String,
    pub passed: bool,
    pub findings: Vec<ValidationFinding>,
}

pub fn validate_handoff_summary(summary: &HandoffSummary) -> HandoffValidationReport {
    let mut findings = Vec::new();

    if summary.objective.trim().is_empty() || summary.objective == "(objective unavailable)" {
        findings.push(ValidationFinding {
            code: "objective_missing".to_string(),
            severity: "warning".to_string(),
            message: "Objective is unavailable.".to_string(),
        });
    }

    let unresolved_failures = unresolved_failed_commands(&summary.verification.checks_run);
    if !unresolved_failures.is_empty() && summary.execution_contract.next_actions.is_empty() {
        findings.push(ValidationFinding {
            code: "next_actions_missing".to_string(),
            severity: "warning".to_string(),
            message: "Unresolved failed checks exist but no next action was generated.".to_string(),
        });
    }

    if !summary.files_modified.is_empty() && summary.verification.checks_run.is_empty() {
        findings.push(ValidationFinding {
            code: "verification_missing".to_string(),
            severity: "warning".to_string(),
            message: "Files were modified but no verification check was recorded.".to_string(),
        });
    }

    if summary.evidence.is_empty() {
        findings.push(ValidationFinding {
            code: "evidence_missing".to_string(),
            severity: "warning".to_string(),
            message: "No evidence references were generated.".to_string(),
        });
    } else if !summary
        .evidence
        .iter()
        .any(|evidence| evidence.claim.starts_with("objective:"))
    {
        findings.push(ValidationFinding {
            code: "objective_evidence_missing".to_string(),
            severity: "warning".to_string(),
            message: "Objective exists but objective evidence is missing.".to_string(),
        });
    }

    if has_work_package_cycle(&summary.work_packages) {
        findings.push(ValidationFinding {
            code: "work_package_cycle".to_string(),
            severity: "error".to_string(),
            message: "work_packages.depends_on contains a cycle.".to_string(),
        });
    }

    let has_material_packages = summary.work_packages.iter().any(is_material_work_package);
    if has_material_packages && summary.execution_contract.ordered_steps.is_empty() {
        findings.push(ValidationFinding {
            code: "ordered_steps_missing".to_string(),
            severity: "warning".to_string(),
            message: "Material work packages exist but execution_contract.ordered_steps is empty."
                .to_string(),
        });
    } else if !ordered_steps_are_consistent(
        &summary.execution_contract.ordered_steps,
        &summary.work_packages,
    ) {
        findings.push(ValidationFinding {
            code: "ordered_steps_inconsistent".to_string(),
            severity: "error".to_string(),
            message:
                "execution_contract.ordered_steps is not temporally or referentially consistent."
                    .to_string(),
        });
    }

    HandoffValidationReport {
        session_id: summary.source_session_id.clone(),
        passed: findings.is_empty(),
        findings,
    }
}

pub fn validate_handoff_summaries(summaries: &[HandoffSummary]) -> Vec<HandoffValidationReport> {
    summaries.iter().map(validate_handoff_summary).collect()
}

fn has_work_package_cycle(packages: &[WorkPackage]) -> bool {
    let mut state: HashMap<&str, u8> = HashMap::new();
    let deps: HashMap<&str, Vec<&str>> = packages
        .iter()
        .map(|pkg| {
            (
                pkg.id.as_str(),
                pkg.depends_on
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
            )
        })
        .collect();

    fn dfs<'a>(
        node: &'a str,
        state: &mut HashMap<&'a str, u8>,
        deps: &HashMap<&'a str, Vec<&'a str>>,
    ) -> bool {
        match state.get(node).copied() {
            Some(1) => return true,
            Some(2) => return false,
            _ => {}
        }
        state.insert(node, 1);
        if let Some(children) = deps.get(node) {
            for child in children {
                if !deps.contains_key(child) {
                    continue;
                }
                if dfs(child, state, deps) {
                    return true;
                }
            }
        }
        state.insert(node, 2);
        false
    }

    for node in deps.keys().copied() {
        if dfs(node, &mut state, &deps) {
            return true;
        }
    }
    false
}

fn ordered_steps_are_consistent(steps: &[OrderedStep], work_packages: &[WorkPackage]) -> bool {
    if steps.is_empty() {
        return true;
    }

    if !steps
        .windows(2)
        .all(|pair| pair[0].sequence < pair[1].sequence)
    {
        return false;
    }

    let known_ids = work_packages
        .iter()
        .map(|pkg| pkg.id.as_str())
        .collect::<HashSet<_>>();
    if !steps
        .iter()
        .all(|step| known_ids.contains(step.work_package_id.as_str()))
    {
        return false;
    }

    let is_monotonic_time = |left: Option<&str>, right: Option<&str>| -> bool {
        match (left, right) {
            (Some(left), Some(right)) => {
                let left = chrono::DateTime::parse_from_rfc3339(left).ok();
                let right = chrono::DateTime::parse_from_rfc3339(right).ok();
                match (left, right) {
                    (Some(left), Some(right)) => left <= right,
                    _ => false,
                }
            }
            _ => true,
        }
    };

    steps
        .windows(2)
        .all(|pair| is_monotonic_time(pair[0].started_at.as_deref(), pair[1].started_at.as_deref()))
}
