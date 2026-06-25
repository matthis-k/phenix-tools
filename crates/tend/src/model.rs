use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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
    pub phase: String,
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
