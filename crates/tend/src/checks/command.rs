use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use crate::model::Step;

use super::CheckResult;

pub fn run(step: &Step, workdir: &Path, env: Option<&HashMap<String, String>>) -> CheckResult {
    if step.command.is_empty() {
        return CheckResult::skip();
    }

    let program = &step.command[0];
    let args: Vec<&str> = step.command[1..].iter().map(|s| s.as_str()).collect();

    let mut cmd = Command::new(program);
    cmd.args(&args).current_dir(workdir);

    if let Some(env_vars) = env {
        cmd.envs(env_vars);
    }

    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => {
            return CheckResult::error(format!("failed to execute command: {e}"));
        }
    };

    let status = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    let expected = step.expect.as_ref().and_then(|e| e.status).unwrap_or(0);

    if status == expected {
        CheckResult::pass_with(stdout, stderr)
    } else {
        CheckResult {
            passed: false,
            skipped: false,
            reason: format!("command exited with status {status} (expected {expected})"),
            stdout,
            stderr,
        }
    }
}
