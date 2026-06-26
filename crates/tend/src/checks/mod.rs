pub mod command;
pub mod files;
pub mod text;

#[derive(Debug, Clone)]
pub enum CheckOutcome {
    Passed,
    Failed { reason: String },
    Skipped { reason: String },
    Errored { reason: String },
}

impl CheckOutcome {
    pub fn is_pass(&self) -> bool { matches!(self, Self::Passed) }
    pub fn is_skip(&self) -> bool { matches!(self, Self::Skipped { .. }) }
    pub fn is_failure(&self) -> bool { matches!(self, Self::Failed { .. } | Self::Errored { .. }) }
}

pub struct CheckResult {
    pub outcome: CheckOutcome,
    pub stdout: String,
    pub stderr: String,
}

impl CheckResult {
    pub fn pass_with(stdout: String, stderr: String) -> Self {
        Self { outcome: CheckOutcome::Passed, stdout, stderr }
    }

    pub fn pass() -> Self {
        Self::pass_with(String::new(), String::new())
    }

    pub fn fail(reason: impl Into<String>) -> Self {
        Self { outcome: CheckOutcome::Failed { reason: reason.into() }, stdout: String::new(), stderr: String::new() }
    }

    pub fn skip() -> Self {
        Self { outcome: CheckOutcome::Skipped { reason: String::new() }, stdout: String::new(), stderr: String::new() }
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Self { outcome: CheckOutcome::Errored { reason: msg.into() }, stdout: String::new(), stderr: String::new() }
    }
}

pub fn dispatch_kind(
    step: &crate::model::Step,
    workdir: &std::path::Path,
    env: Option<&std::collections::HashMap<String, String>>,
) -> CheckResult {
    match step.kind.as_str() {
        "command" => command::run(step, workdir, env),
        "filesExist" => files::run_exist(step, workdir),
        "filesAbsent" => files::run_absent(step, workdir),
        "forbidText" => text::run_forbid(step, workdir),
        "requireText" => text::run_require(step, workdir),
        _ => CheckResult::error(format!("unknown kind: {}", step.kind)),
    }
}
