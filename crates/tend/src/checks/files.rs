use std::path::Path;

use crate::model::Step;

use super::CheckResult;

pub fn run_exist(step: &Step, workdir: &Path) -> CheckResult {
    let paths = &step.paths;
    if paths.is_empty() {
        return CheckResult::skip();
    }

    let mut missing = Vec::new();

    for pattern in paths {
        let path = if Path::new(pattern).is_absolute() {
            Path::new(pattern).to_path_buf()
        } else {
            workdir.join(pattern)
        };

        if !path.exists() {
            missing.push(pattern.clone());
        }
    }

    if missing.is_empty() {
        CheckResult::pass()
    } else {
        CheckResult::fail(format!("files not found: {}", missing.join(", ")))
    }
}

pub fn run_absent(step: &Step, workdir: &Path) -> CheckResult {
    let paths = &step.paths;
    if paths.is_empty() {
        return CheckResult::skip();
    }

    let mut found = Vec::new();

    for pattern in paths {
        let path = workdir.join(pattern);
        if path.exists() {
            found.push(pattern.clone());
        }
    }

    if found.is_empty() {
        CheckResult::pass()
    } else {
        CheckResult::fail(format!("unexpected files present: {}", found.join(", ")))
    }
}
