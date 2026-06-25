pub mod command;
pub mod files;
pub mod text;

pub struct CheckResult {
    pub passed: bool,
    pub skipped: bool,
    pub reason: String,
    pub stdout: String,
    pub stderr: String,
}

impl CheckResult {
    pub fn pass() -> Self {
        Self {
            passed: true,
            skipped: false,
            reason: String::new(),
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    pub fn fail(reason: impl Into<String>) -> Self {
        Self {
            passed: false,
            skipped: false,
            reason: reason.into(),
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    pub fn skip() -> Self {
        Self {
            passed: true,
            skipped: true,
            reason: String::new(),
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            passed: false,
            skipped: false,
            reason: msg.into(),
            stdout: String::new(),
            stderr: String::new(),
        }
    }
}

pub fn dispatch_kind(step: &crate::model::Step, workdir: &std::path::Path) -> CheckResult {
    match step.kind.as_str() {
        "command" => command::run(step, workdir),
        "filesExist" => files::run_exist(step, workdir),
        "filesAbsent" => files::run_absent(step, workdir),
        "forbidText" => text::run_forbid(step, workdir),
        "requireText" => text::run_require(step, workdir),
        _ => CheckResult::error(format!("unknown kind: {}", step.kind)),
    }
}
