use std::path::Path;
use std::process::Command;

use crate::model::Step;

use super::CheckResult;

pub fn run(step: &Step, workdir: &Path) -> CheckResult {
    if step.command.is_empty() {
        return CheckResult::skip();
    }

    let program = &step.command[0];
    let args: Vec<&str> = step.command[1..].iter().map(|s| s.as_str()).collect();

    let output = match Command::new(program).args(&args).current_dir(workdir).output() {
        Ok(o) => o,
        Err(e) => {
            return CheckResult::error(format!("failed to execute command: {e}"));
        }
    };

    let status = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if status == 0 {
        CheckResult::pass()
    } else {
        CheckResult {
            passed: false,
            skipped: false,
            reason: format!("command exited with status {status}"),
            stdout,
            stderr,
        }
    }
}
