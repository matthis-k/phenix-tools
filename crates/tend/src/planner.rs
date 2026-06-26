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
    pub phase: Phase,
    pub step: Step,
    pub item_type: PlanItemType,
    pub context: ContextConfig,
    pub reason: PlanReason,
    pub matched_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanItemType {
    TaskAction,
    TaskBefore,
    TaskAfter,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanReason {
    ChangedFile,
    Always,
    Force,
    Explicit,
    BeforeAfter,
}

impl std::fmt::Display for PlanReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlanReason::ChangedFile => write!(f, "matched changed file(s)"),
            PlanReason::Always => write!(f, "always-run"),
            PlanReason::Force => write!(f, "force mode"),
            PlanReason::Explicit => write!(f, "explicit selection"),
            PlanReason::BeforeAfter => write!(f, "before/after hook"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub items: Vec<PlanItem>,
}

pub fn build_plan(
    nodes: &[ResolvedNode],
    req: &PlanRequest,
) -> Result<Plan, PlanError> {
    let phase = req.phase;
    let mode = req.mode;
    let changed_files: Option<Vec<String>> = if !req.files.is_empty() {
        Some(req.files.clone())
    } else {
        None
    };
    let changed_ref = changed_files.as_deref();
    let command_is_mutating = phase.is_mutating();
    let mut items = Vec::new();

    for node in nodes {
        if let Some(ref g) = req.group {
            if node.id != *g {
                continue;
            }
        }

        let node_applies = node_applies(node, mode, changed_ref);
        if !node_applies && mode == RunMode::Changed {
            continue;
        }

        for step_cfg in &node.before {
            let step = Step::from(step_cfg);
            let description = if step.description.is_empty() {
                format!("node before ({})", step.kind.description())
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
                phase,
                step: step,
                item_type: PlanItemType::TaskBefore,
                context: node.context.clone(),
                reason: PlanReason::BeforeAfter,
                matched_files: Vec::new(),
            });
        }

        for task in &node.tasks {
            if task.config.phase != phase {
                continue;
            }

            if let Some(ref t) = req.target {
                if task.config.id != *t {
                    continue;
                }
            }

            let applies = task_applies(task, mode, changed_ref);

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

            // Compute matched files for this task
            let matched = compute_matched_files(task, changed_ref);

            let reason = if mode == RunMode::Force {
                PlanReason::Force
            } else if task.config.always.unwrap_or(false) {
                PlanReason::Always
            } else if !matched.is_empty() {
                PlanReason::ChangedFile
            } else {
                PlanReason::Explicit
            };

            for step_cfg in task.config.before.iter().flatten() {
                let step = Step::from(step_cfg);
                items.push(PlanItem {
                    node_path: node.node_path.clone(),
                    config_path: node.config_path.clone(),
                    task_id: format!("{}.before", task.config.id),
                    chain_id: task_chain_id.clone(),
                    description: step.description.clone(),
                    phase,
                    step: step,
                    item_type: PlanItemType::TaskBefore,
                    context: node.context.clone(),
                    reason: PlanReason::BeforeAfter,
                    matched_files: Vec::new(),
                });
            }

            let task_step_kind = task_step_kind_from_config(&task.config);
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
                phase,
                step: Step {
                    kind: task_step_kind,
                    always: task.config.always.unwrap_or(false),
                    description: task
                        .config
                        .description
                        .clone()
                        .unwrap_or_default(),
                },
                item_type: PlanItemType::TaskAction,
                context: node.context.clone(),
                reason,
                matched_files: matched,
            });

            for step_cfg in task.config.after.iter().flatten() {
                let step = Step::from(step_cfg);
                items.push(PlanItem {
                    node_path: node.node_path.clone(),
                    config_path: node.config_path.clone(),
                    task_id: format!("{}.after", task.config.id),
                    chain_id: task_chain_id.clone(),
                    description: step.description.clone(),
                    phase,
                    step: step,
                    item_type: PlanItemType::TaskAfter,
                    context: node.context.clone(),
                    reason: PlanReason::BeforeAfter,
                    matched_files: Vec::new(),
                });
            }
        }

        for step_cfg in &node.after {
            let step = Step::from(step_cfg);
            let description = if step.description.is_empty() {
                format!("node after ({})", step.kind.description())
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
                phase,
                step: step,
                item_type: PlanItemType::TaskAfter,
                context: node.context.clone(),
                reason: PlanReason::BeforeAfter,
                matched_files: Vec::new(),
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

fn compute_matched_files(task: &ResolvedTask, changed_files: Option<&[String]>) -> Vec<String> {
    let changed_files = match changed_files {
        Some(f) => f,
        None => return Vec::new(),
    };
    let when = match &task.config.when {
        Some(w) => w,
        None => return Vec::new(),
    };
    let changed = match &when.changed {
        Some(c) => c,
        None => return Vec::new(),
    };
    if changed.paths.is_empty() {
        return Vec::new();
    }
    changed_files
        .iter()
        .filter(|f| task_matches_paths(&changed.paths, &[(*f).clone()]))
        .cloned()
        .collect()
}

fn should_run_step(step: &Step) -> bool {
    use crate::model::TaskKind;
    match &step.kind {
        TaskKind::Command { command, .. } => !command.is_empty(),
        TaskKind::FilesExist { paths } | TaskKind::FilesAbsent { paths } => !paths.is_empty(),
        TaskKind::ForbidText { paths, .. } | TaskKind::RequireText { paths, .. } => !paths.is_empty(),
    }
}

fn task_step_kind_from_config(cfg: &TaskConfig) -> crate::model::TaskKind {
    use crate::model::TaskKind;
    match cfg.kind.as_str() {
        "filesExist" => TaskKind::FilesExist {
            paths: cfg.paths.clone().unwrap_or_default(),
        },
        "filesAbsent" => TaskKind::FilesAbsent {
            paths: cfg.paths.clone().unwrap_or_default(),
        },
        "forbidText" => TaskKind::ForbidText {
            paths: cfg.paths.clone().unwrap_or_default(),
            patterns: cfg.patterns.clone().unwrap_or_default(),
        },
        "requireText" => TaskKind::RequireText {
            paths: cfg.paths.clone().unwrap_or_default(),
            patterns: cfg.patterns.clone().unwrap_or_default(),
        },
        _ => TaskKind::Command {
            command: cfg.command.clone().unwrap_or_default(),
            expect: cfg.expect.clone(),
        },
    }
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

    fn req(phase: Phase, mode: RunMode) -> PlanRequest {
        PlanRequest {
            phase,
            mode,
            group: None,
            target: None,
            files: Vec::new(),
        }
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

        let result = build_plan(&[node], &req(Phase::Verify, RunMode::Full));
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

        let result = build_plan(&[node], &req(Phase::Fix, RunMode::Full));
        assert!(result.is_ok());
    }
}
