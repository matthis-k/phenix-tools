use std::collections::HashSet;
use std::path::Path;

use crate::checks;
use crate::planner::PlanItem;

#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub task_id: String,
    pub description: String,
    pub kind: String,
    pub phase: crate::model::Phase,
    pub passed: bool,
    pub skipped: bool,
    pub reason: String,
    pub stdout: String,
    pub stderr: String,
}

pub fn execute_plan(items: &[PlanItem], _root: &Path) -> Vec<ExecutionResult> {
    let mut results = Vec::new();
    let mut failed_chains: HashSet<String> = HashSet::new();

    for item in items {
        let always = item
            .step
            .as_ref()
            .map(|s| s.always)
            .unwrap_or(false);

        if failed_chains.contains(&item.chain_id) && !always {
            results.push(ExecutionResult {
                task_id: item.task_id.clone(),
                description: item.description.clone(),
                kind: item.kind.clone(),
                phase: item.phase.clone(),
                passed: true,
                skipped: true,
                reason: "skipped due to earlier failure in chain".to_string(),
                stdout: String::new(),
                stderr: String::new(),
            });
            continue;
        }

        let step = match &item.step {
            Some(s) => s,
            None => {
                failed_chains.insert(item.chain_id.clone());
                results.push(ExecutionResult {
                    task_id: item.task_id.clone(),
                    description: item.description.clone(),
                    kind: item.kind.clone(),
                    phase: item.phase.clone(),
                    passed: false,
                    skipped: false,
                    reason: "internal error: no step defined".to_string(),
                    stdout: String::new(),
                    stderr: String::new(),
                });
                continue;
            }
        };

        let workdir = effective_workdir(item, _root);
        let env = item.context.env.as_ref();

        let check_result = checks::dispatch_kind(step, &workdir, env);

        let failed = check_result.outcome.is_failure();
        if failed {
            failed_chains.insert(item.chain_id.clone());
        }

        let (passed, skipped, reason) = match &check_result.outcome {
            checks::CheckOutcome::Passed => (true, false, String::new()),
            checks::CheckOutcome::Skipped { reason } => (true, true, reason.clone()),
            checks::CheckOutcome::Failed { reason } => (false, false, reason.clone()),
            checks::CheckOutcome::Errored { reason } => (false, false, reason.clone()),
        };

        results.push(ExecutionResult {
            task_id: item.task_id.clone(),
            description: item.description.clone(),
            kind: item.kind.clone(),
            phase: item.phase,
            passed,
            skipped,
            reason,
            stdout: check_result.stdout,
            stderr: check_result.stderr,
        });
    }

    results
}

fn effective_workdir(item: &PlanItem, fallback: &Path) -> std::path::PathBuf {
    match &item.context.workdir {
        Some(wd) if wd == "configDir" => {
            item.config_path.parent().unwrap_or(fallback).to_path_buf()
        }
        Some(wd) if wd == "programCwd" => {
            std::env::current_dir().unwrap_or_else(|_| fallback.to_path_buf())
        }
        Some(wd) => {
            item.config_path.parent().unwrap_or(fallback).join(wd)
        }
        None => item.config_path.parent().unwrap_or(fallback).to_path_buf(),
    }
}
