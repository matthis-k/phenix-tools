use globset::{GlobBuilder, GlobSetBuilder};

use crate::config;
use crate::model::*;

#[derive(Debug)]
pub enum PlanError {
    MutatingRefused(String),
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MutatingRefused(id) => {
                write!(f, "mutating task '{id}' refused in non-mutating command")
            }
        }
    }
}

impl std::error::Error for PlanError {}

#[derive(Debug, Clone)]
pub struct PlanItem {
    pub node_path: std::path::PathBuf,
    pub config_path: std::path::PathBuf,
    pub task_id: String,
    pub chain_id: String,
    pub description: String,
    pub kind: String,
    pub phase: Phase,
    pub step: Option<Step>,
    pub item_type: PlanItemType,
    pub context: ContextConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanItemType {
    TaskAction,
    TaskBefore,
    TaskAfter,
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub items: Vec<PlanItem>,
}

pub fn build_plan(
    nodes: &[ResolvedNode],
    phase: Phase,
    mode: RunMode,
    changed_files: Option<&[String]>,
) -> Result<Plan, PlanError> {
    let command_is_mutating = phase.is_mutating();
    let mut items = Vec::new();

    for node in nodes {
        let node_applies = node_applies(node, mode, changed_files);
        if !node_applies && mode == RunMode::Changed {
            continue;
        }

        for step_cfg in &node.before {
            let step = Step::from(step_cfg);
            let description = if step.description.is_empty() {
                format!("node before ({})", step.kind)
            } else {
                step.description.clone()
            };

            if !should_run_step(&step) {
                continue;
            }

            items.push(PlanItem {
                node_path: node.node_path.clone(),
                config_path: node.config_path.clone(),
                task_id: format!("{}.before", node.id),
                chain_id: node.id.clone(),
                description,
                kind: step.kind.clone(),
                phase,
                step: Some(step),
                item_type: PlanItemType::TaskBefore,
                context: node.context.clone(),
            });
        }

        for task in &node.tasks {
            if task.config.phase != phase {
                continue;
            }

            let applies = task_applies(task, mode, changed_files);

            if mode == RunMode::Changed && !applies {
                continue;
            }

            if !applies && mode == RunMode::Full && !task.config.always.unwrap_or(false) {
                continue;
            }

            let is_mutating = task
                .config
                .mutates
                .unwrap_or_else(|| config::default_mutates(&task.config.phase));

            if !command_is_mutating && is_mutating {
                return Err(PlanError::MutatingRefused(task.config.id.clone()));
            }

            let task_chain_id = format!("{}.{}", node.id, task.config.id);

            for step_cfg in task.config.before.iter().flatten() {
                let step = Step::from(step_cfg);
                items.push(PlanItem {
                    node_path: node.node_path.clone(),
                    config_path: node.config_path.clone(),
                    task_id: format!("{}.before", task.config.id),
                    chain_id: task_chain_id.clone(),
                    description: step.description.clone(),
                    kind: step.kind.clone(),
                    phase,
                    step: Some(step),
                    item_type: PlanItemType::TaskBefore,
                    context: node.context.clone(),
                });
            }

            items.push(PlanItem {
                node_path: node.node_path.clone(),
                config_path: node.config_path.clone(),
                task_id: task.config.id.clone(),
                chain_id: task_chain_id.clone(),
                description: task
                    .config
                    .description
                    .clone()
                    .unwrap_or_default(),
                kind: task.config.kind.clone(),
                phase,
                step: Some(Step {
                    kind: task.config.kind.clone(),
                    command: task.config.command.clone().unwrap_or_default(),
                    paths: task.config.paths.clone().unwrap_or_default(),
                    patterns: task.config.patterns.clone().unwrap_or_default(),
                    always: task.config.always.unwrap_or(false),
                    description: task
                        .config
                        .description
                        .clone()
                        .unwrap_or_default(),
                    expect: task.config.expect.clone(),
                }),
                item_type: PlanItemType::TaskAction,
                context: node.context.clone(),
            });

            for step_cfg in task.config.after.iter().flatten() {
                let step = Step::from(step_cfg);
                items.push(PlanItem {
                    node_path: node.node_path.clone(),
                    config_path: node.config_path.clone(),
                    task_id: format!("{}.after", task.config.id),
                    chain_id: task_chain_id.clone(),
                    description: step.description.clone(),
                    kind: step.kind.clone(),
                    phase,
                    step: Some(step),
                    item_type: PlanItemType::TaskAfter,
                    context: node.context.clone(),
                });
            }
        }

        for step_cfg in &node.after {
            let step = Step::from(step_cfg);
            let description = if step.description.is_empty() {
                format!("node after ({})", step.kind)
            } else {
                step.description.clone()
            };

            if !should_run_step(&step) {
                continue;
            }

            items.push(PlanItem {
                node_path: node.node_path.clone(),
                config_path: node.config_path.clone(),
                task_id: format!("{}.after", node.id),
                chain_id: node.id.clone(),
                description,
                kind: step.kind.clone(),
                    phase,
                    step: Some(step),
                item_type: PlanItemType::TaskAfter,
                context: node.context.clone(),
            });
        }
    }

    Ok(Plan { items })
}

fn node_applies(node: &ResolvedNode, mode: RunMode, changed_files: Option<&[String]>) -> bool {
    if mode == RunMode::Force {
        return true;
    }

    let when = match &node.when {
        Some(w) => w,
        None => return true,
    };

    let changed = match &when.changed {
        Some(c) => c,
        None => return true,
    };

    let changed_files = match changed_files {
        Some(f) => f,
        None => return changed.paths.is_empty(),
    };

    if changed.paths.is_empty() {
        return true;
    }

    task_matches_paths(&changed.paths, changed_files)
}

fn task_applies(task: &ResolvedTask, mode: RunMode, changed_files: Option<&[String]>) -> bool {
    if task.config.always.unwrap_or(false) {
        return true;
    }

    if mode == RunMode::Force {
        return true;
    }

    let when = match &task.config.when {
        Some(w) => w,
        None => return true,
    };

    let changed = match &when.changed {
        Some(c) => c,
        None => return true,
    };

    let changed_files = match changed_files {
        Some(f) => f,
        None => return changed.paths.is_empty(),
    };

    if changed.paths.is_empty() {
        return true;
    }

    task_matches_paths(&changed.paths, changed_files)
}

fn should_run_step(step: &Step) -> bool {
    !step.command.is_empty() || !step.paths.is_empty() || !step.patterns.is_empty()
}

pub fn task_matches_paths(patterns: &[String], changed_files: &[String]) -> bool {
    let mut builder = GlobSetBuilder::new();
    for p in patterns {
        let glob = match GlobBuilder::new(p)
            .literal_separator(true)
            .build()
        {
            Ok(g) => g,
            Err(_) => continue,
        };
        builder.add(glob);
    }
    let glob_set = match builder.build() {
        Ok(g) => g,
        Err(_) => return false,
    };

    for f in changed_files {
        if glob_set.is_match(f.as_str()) {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_changed_file_glob_matches() {
        let patterns = vec!["docs/**/*.md".to_string()];
        let changed = vec!["docs/readme.md".to_string(), "src/main.rs".to_string()];
        assert!(task_matches_paths(&patterns, &changed));
    }

    #[test]
    fn test_changed_file_glob_no_match() {
        let patterns = vec!["docs/**/*.md".to_string()];
        let changed = vec!["src/main.rs".to_string()];
        assert!(!task_matches_paths(&patterns, &changed));
    }

    #[test]
    fn test_mutating_task_refused_in_verify() {
        let task = ResolvedTask {
            config: TaskConfig {
                id: "bad-task".to_string(),
                description: None,
                phase: crate::model::Phase::Verify,
                kind: "command".to_string(),
                tags: None,
                mutates: Some(true),
                when: None,
                always: None,
                before: None,
                after: None,
                command: Some(vec!["touch".to_string(), "/tmp/evil".to_string()]),
                expect: None,
                paths: None,
                patterns: None,
            },
            parent_node_path: Path::new(".").to_path_buf(),
        };

        let node = ResolvedNode {
            config_path: Path::new(".tend.json").to_path_buf(),
            node_path: Path::new(".").to_path_buf(),
            id: "root".to_string(),
            description: String::new(),
            tags: vec![],
            when: None,
            context: ContextConfig {
                workdir: None,
                env: None,
            },
            before: vec![],
            after: vec![],
            tasks: vec![task],
        };

        let result = build_plan(&[node], Phase::Verify, RunMode::Full, None);
        assert!(result.is_err());
        match result {
            Err(PlanError::MutatingRefused(id)) => assert_eq!(id, "bad-task"),
            _ => panic!("expected MutatingRefused error"),
        }
    }

    #[test]
    fn test_mutating_task_allowed_in_fix() {
        let task = ResolvedTask {
            config: TaskConfig {
                id: "ok-task".to_string(),
                description: None,
                phase: crate::model::Phase::Fix,
                kind: "command".to_string(),
                tags: None,
                mutates: Some(true),
                when: None,
                always: None,
                before: None,
                after: None,
                command: Some(vec!["touch".to_string(), "/tmp/test".to_string()]),
                expect: None,
                paths: None,
                patterns: None,
            },
            parent_node_path: Path::new(".").to_path_buf(),
        };

        let node = ResolvedNode {
            config_path: Path::new(".tend.json").to_path_buf(),
            node_path: Path::new(".").to_path_buf(),
            id: "root".to_string(),
            description: String::new(),
            tags: vec![],
            when: None,
            context: ContextConfig {
                workdir: None,
                env: None,
            },
            before: vec![],
            after: vec![],
            tasks: vec![task],
        };

        let result = build_plan(&[node], Phase::Fix, RunMode::Full, None);
        assert!(result.is_ok());
    }
}
