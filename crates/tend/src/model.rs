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
        matches!(self, Phase::Fix | Phase::Generate | Phase::Setup | Phase::Cleanup)
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
pub struct ContextConfig {
    pub workdir: Option<String>,
    pub env: Option<std::collections::HashMap<String, String>>,
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
    pub kind: String,
    pub tags: Option<Vec<String>>,
    pub mutates: Option<bool>,
    pub when: Option<WhenConfig>,
    pub always: Option<bool>,
    pub before: Option<Vec<StepConfig>>,
    pub after: Option<Vec<StepConfig>>,
    pub command: Option<Vec<String>>,
    pub expect: Option<ExpectConfig>,
    pub paths: Option<Vec<String>>,
    pub patterns: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectConfig {
    pub status: Option<i32>,
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
pub struct Step {
    pub kind: String,
    pub command: Vec<String>,
    pub paths: Vec<String>,
    pub patterns: Vec<String>,
    pub always: bool,
    pub description: String,
    pub expect: Option<ExpectConfig>,
}

impl From<&StepConfig> for Step {
    fn from(s: &StepConfig) -> Self {
        Self {
            kind: s.kind.clone().unwrap_or_else(|| "command".to_string()),
            command: s.command.clone().unwrap_or_default(),
            paths: s.paths.clone().unwrap_or_default(),
            patterns: s.patterns.clone().unwrap_or_default(),
            always: s.always.unwrap_or(false),
            description: s.description.clone().unwrap_or_default(),
            expect: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum PlanItem {
    TaskAction {
        node_path: PathBuf,
        config_path: PathBuf,
        task: ResolvedTask,
    },
}
