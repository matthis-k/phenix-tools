use std::path::PathBuf;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Verify,
    Fix,
    Generate,
    Setup,
    Cleanup,
}

impl Phase {
    pub fn is_mutating(self) -> bool {
        matches!(
            self,
            Phase::Fix | Phase::Generate | Phase::Setup | Phase::Cleanup
        )
    }
}

impl std::fmt::Display for Phase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Phase::Verify => write!(f, "verify"),
            Phase::Fix => write!(f, "fix"),
            Phase::Generate => write!(f, "generate"),
            Phase::Setup => write!(f, "setup"),
            Phase::Cleanup => write!(f, "cleanup"),
        }
    }
}

impl FromStr for Phase {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "verify" => Ok(Phase::Verify),
            "fix" => Ok(Phase::Fix),
            "generate" => Ok(Phase::Generate),
            "setup" => Ok(Phase::Setup),
            "cleanup" => Ok(Phase::Cleanup),
            _ => Err(format!("Unknown phase: {s}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RunMode {
    Changed,
    Staged,
    #[serde(alias = "all")]
    Full,
    Force,
    Selected,
}

impl std::fmt::Display for RunMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunMode::Changed => write!(f, "changed"),
            RunMode::Staged => write!(f, "staged"),
            RunMode::Full => write!(f, "full"),
            RunMode::Force => write!(f, "force"),
            RunMode::Selected => write!(f, "selected"),
        }
    }
}

impl FromStr for RunMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "changed" => Ok(RunMode::Changed),
            "staged" => Ok(RunMode::Staged),
            "full" | "all" => Ok(RunMode::Full),
            "force" => Ok(RunMode::Force),
            "selected" => Ok(RunMode::Selected),
            _ => Err(format!("Unknown mode: {s}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TendConfig {
    pub version: u32,
    pub node: NodeConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub id: Option<String>,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    pub when: Option<WhenConfig>,
    pub context: Option<ContextConfig>,
    pub before: Option<Vec<StepConfig>>,
    pub tasks: Option<Vec<TaskConfig>>,
    pub after: Option<Vec<StepConfig>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhenConfig {
    pub changed: Option<ChangedConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangedConfig {
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WorkdirPolicy {
    ConfigDir,
    ProgramCwd,
    Relative(String),
}

impl WorkdirPolicy {
    pub fn resolve(
        &self,
        config_dir: &std::path::Path,
        fallback: &std::path::Path,
    ) -> std::path::PathBuf {
        match self {
            WorkdirPolicy::ConfigDir => config_dir.to_path_buf(),
            WorkdirPolicy::ProgramCwd => {
                std::env::current_dir().unwrap_or_else(|_| fallback.to_path_buf())
            }
            WorkdirPolicy::Relative(suffix) => config_dir.join(suffix),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ShellConfig {
    pub flake: Option<String>,
    pub name: Option<String>,
    pub impure: Option<bool>,
    pub accept_flake_config: Option<bool>,
    pub extra_args: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContextConfig {
    pub workdir: Option<WorkdirPolicy>,
    pub env: Option<std::collections::HashMap<String, String>>,
    pub shell: Option<ShellConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepConfig {
    pub kind: Option<String>,
    pub command: Option<Vec<String>>,
    pub paths: Option<Vec<String>>,
    pub patterns: Option<Vec<String>>,
    pub always: Option<bool>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskConfig {
    pub id: String,
    pub description: Option<String>,
    pub phase: Phase,
    #[serde(flatten)]
    pub kind: TaskKind,
    pub context: Option<ContextConfig>,
    pub tags: Option<Vec<String>>,
    pub profiles: Option<Vec<String>>,
    pub mutates: Option<bool>,
    pub interactive: Option<bool>,
    pub network: Option<bool>,
    pub sandbox_safe: Option<bool>,
    pub when: Option<WhenConfig>,
    pub always: Option<bool>,
    pub before: Option<Vec<StepConfig>>,
    pub after: Option<Vec<StepConfig>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectConfig {
    pub status: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TaskKind {
    Command {
        command: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        expect: Option<ExpectConfig>,
    },
    #[serde(rename = "filesExist")]
    FilesExist { paths: Vec<String> },
    #[serde(rename = "filesAbsent")]
    FilesAbsent { paths: Vec<String> },
    #[serde(rename = "forbidText")]
    ForbidText {
        paths: Vec<String>,
        patterns: Vec<String>,
    },
    #[serde(rename = "requireText")]
    RequireText {
        paths: Vec<String>,
        patterns: Vec<String>,
    },
}

impl TaskKind {
    pub fn description(&self) -> &str {
        match self {
            TaskKind::Command { .. } => "command",
            TaskKind::FilesExist { .. } => "filesExist",
            TaskKind::FilesAbsent { .. } => "filesAbsent",
            TaskKind::ForbidText { .. } => "forbidText",
            TaskKind::RequireText { .. } => "requireText",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedNode {
    pub config_path: PathBuf,
    pub node_path: PathBuf,
    pub id: String,
    pub description: String,
    pub tags: Vec<String>,
    pub when: Option<WhenConfig>,
    pub context: ContextConfig,
    pub before: Vec<StepConfig>,
    pub after: Vec<StepConfig>,
    pub tasks: Vec<ResolvedTask>,
}

#[derive(Debug, Clone)]
pub struct ResolvedTask {
    pub config: TaskConfig,
    pub parent_node_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct PlanRequest {
    pub phase: Phase,
    pub mode: RunMode,
    pub profile: Option<String>,
    pub group: Option<String>,
    pub target: Option<String>,
    pub files: Vec<String>,
    pub offline: bool,
    pub locked: bool,
}

#[derive(Debug, Clone)]
pub struct Step {
    pub kind: TaskKind,
    pub always: bool,
    pub description: String,
}

impl From<&StepConfig> for Step {
    fn from(s: &StepConfig) -> Self {
        let kind = match s.kind.as_deref() {
            Some("filesExist") => TaskKind::FilesExist {
                paths: s.paths.clone().unwrap_or_default(),
            },
            Some("filesAbsent") => TaskKind::FilesAbsent {
                paths: s.paths.clone().unwrap_or_default(),
            },
            Some("forbidText") => TaskKind::ForbidText {
                paths: s.paths.clone().unwrap_or_default(),
                patterns: s.patterns.clone().unwrap_or_default(),
            },
            Some("requireText") => TaskKind::RequireText {
                paths: s.paths.clone().unwrap_or_default(),
                patterns: s.patterns.clone().unwrap_or_default(),
            },
            _ => TaskKind::Command {
                command: s.command.clone().unwrap_or_default(),
                expect: None,
            },
        };
        Self {
            kind,
            always: s.always.unwrap_or(false),
            description: s.description.clone().unwrap_or_default(),
        }
    }
}
