use std::collections::HashSet;

use crate::model::{NodeConfig, ResolvedNode, ResolvedTask};

#[derive(Debug)]
pub enum ConfigError {
    InvalidVersion(u32),
    InvalidJson(String),
    DuplicateTaskId(String),
    UnknownKind(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidVersion(v) => write!(f, "unsupported config version: {v}"),
            Self::InvalidJson(msg) => write!(f, "invalid JSON: {msg}"),
            Self::DuplicateTaskId(id) => write!(f, "duplicate task id: {id}"),
            Self::UnknownKind(k) => write!(f, "unknown task kind: {k}"),
        }
    }
}

impl std::error::Error for ConfigError {}

pub fn validate_tasks(tasks: &[crate::model::TaskConfig]) -> Result<(), ConfigError> {
    let mut seen = HashSet::new();
    for task in tasks {
        if !seen.insert(task.id.as_str()) {
            return Err(ConfigError::DuplicateTaskId(task.id.clone()));
        }
    }
    Ok(())
}

pub fn resolve_node(
    config_path: &std::path::Path,
    node_path: &std::path::Path,
    config: NodeConfig,
) -> ResolvedNode {
    let id = config
        .id
        .clone()
        .unwrap_or_else(|| node_path.to_string_lossy().to_string());
    let description = config.description.unwrap_or_default();
    let tags = config.tags.unwrap_or_default();
    let context = config.context.unwrap_or(crate::model::ContextConfig {
        workdir: None,
        env: None,
        shell: None,
    });

    let tasks: Vec<ResolvedTask> = config
        .tasks
        .unwrap_or_default()
        .into_iter()
        .map(|t| ResolvedTask {
            config: t,
            parent_node_path: node_path.to_path_buf(),
        })
        .collect();

    ResolvedNode {
        config_path: config_path.to_path_buf(),
        node_path: node_path.to_path_buf(),
        id,
        description,
        tags,
        when: config.when,
        context,
        before: config.before.unwrap_or_default(),
        after: config.after.unwrap_or_default(),
        tasks,
    }
}

pub fn default_mutates(phase: &crate::model::Phase) -> bool {
    phase.is_mutating()
}
