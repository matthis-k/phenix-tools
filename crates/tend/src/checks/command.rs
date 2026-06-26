use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use crate::model::ExpectConfig;

use super::CheckResult;

pub fn run_command(
    command: &[String],
    expect: Option<&ExpectConfig>,
    workdir: &Path,
    env: Option<&HashMap<String, String>>,
) -> CheckResult {
    if command.is_empty() {
        return CheckResult::skip();
    }

    let program = &command[0];
    let args: Vec<&str> = command[1..].iter().map(|s| s.as_str()).collect();

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

    let expected = expect.and_then(|e| e.status).unwrap_or(0);

    if status == expected {
        CheckResult::pass_with(stdout, stderr)
    } else {
        CheckResult {
            outcome: crate::checks::CheckOutcome::Failed {
                reason: format!("command exited with status {status} (expected {expected})"),
            },
            stdout,
            stderr,
        }
    }
}
