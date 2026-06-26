use std::collections::HashSet;
use std::path::Path;

use crate::checks;
use crate::checks::CheckOutcome;
use crate::model::TaskKind;
use crate::planner::PlanItem;

#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub task_id: String,
    pub description: String,
    pub kind: String,
    pub phase: crate::model::Phase,
    pub outcome: CheckOutcome,
    pub stdout: String,
    pub stderr: String,
}

pub fn execute_plan(items: &[PlanItem], _root: &Path) -> Vec<ExecutionResult> {
    let mut results = Vec::new();
    let mut failed_chains: HashSet<String> = HashSet::new();

    for item in items {
        if failed_chains.contains(&item.chain_id) && !item.step.always {
            results.push(ExecutionResult {
                task_id: item.task_id.clone(),
                description: item.description.clone(),
                kind: item.step.kind.description().to_string(),
                phase: item.phase,
                outcome: CheckOutcome::Skipped {
                    reason: "skipped due to earlier failure in chain".to_string(),
                },
                stdout: String::new(),
                stderr: String::new(),
            });
            continue;
        }

        let workdir = effective_workdir(item, _root);
        let env = item.context.env.as_ref();

        let shell = item.context.shell.as_ref();

        let check_result = match &item.step.kind {
            TaskKind::Command { command, expect } => {
                checks::command::run_command(command, expect.as_ref(), &workdir, env, shell)
            }
            TaskKind::FilesExist { paths } => checks::files::run_exist(paths, &workdir),
            TaskKind::FilesAbsent { paths } => checks::files::run_absent(paths, &workdir),
            TaskKind::ForbidText { paths, patterns } => {
                checks::text::run_forbid(paths, patterns, &workdir)
            }
            TaskKind::RequireText { paths, patterns } => {
                checks::text::run_require(paths, patterns, &workdir)
            }
        };

        if check_result.outcome.is_failure() {
            failed_chains.insert(item.chain_id.clone());
        }

        results.push(ExecutionResult {
            task_id: item.task_id.clone(),
            description: item.description.clone(),
            kind: item.step.kind.description().to_string(),
            phase: item.phase,
            outcome: check_result.outcome,
            stdout: check_result.stdout,
            stderr: check_result.stderr,
        });
    }

    results
}

fn effective_workdir(item: &PlanItem, fallback: &Path) -> std::path::PathBuf {
    match &item.context.workdir {
        Some(policy) => {
            let config_dir = item.config_path.parent().unwrap_or(fallback);
            policy.resolve(config_dir, fallback)
        }
        None => item.config_path.parent().unwrap_or(fallback).to_path_buf(),
    }
}
