use std::path::Path;

use crate::checks;
use crate::planner::PlanItem;

#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub task_id: String,
    pub description: String,
    pub kind: String,
    pub phase: String,
    pub passed: bool,
    pub skipped: bool,
    pub reason: String,
    pub stdout: String,
    pub stderr: String,
}

pub fn execute_plan(items: &[PlanItem], _root: &Path) -> Vec<ExecutionResult> {
    let mut results = Vec::new();
    let mut chain_failed = false;

    for item in items {
        let always = item
            .step
            .as_ref()
            .map(|s| s.always)
            .unwrap_or(false);

        if chain_failed && !always {
            results.push(ExecutionResult {
                task_id: item.task_id.clone(),
                description: item.description.clone(),
                kind: item.kind.clone(),
                phase: item.phase.clone(),
                passed: true,
                skipped: true,
                reason: "skipped due to earlier failure".to_string(),
                stdout: String::new(),
                stderr: String::new(),
            });
            continue;
        }

        let step = match &item.step {
            Some(s) => s,
            None => {
                chain_failed = true;
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

        let config_dir = item.config_path.parent().unwrap_or(_root);
        let check_result = checks::dispatch_kind(step, config_dir);

        let failed = !check_result.passed && !check_result.skipped;
        if failed {
            chain_failed = true;
        }

        results.push(ExecutionResult {
            task_id: item.task_id.clone(),
            description: item.description.clone(),
            kind: item.kind.clone(),
            phase: item.phase.clone(),
            passed: check_result.passed,
            skipped: check_result.skipped,
            reason: check_result.reason,
            stdout: check_result.stdout,
            stderr: check_result.stderr,
        });
    }

    results
}
