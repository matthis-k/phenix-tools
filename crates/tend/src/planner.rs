use std::collections::{BTreeMap, BTreeSet, HashSet};

use globset::{GlobBuilder, GlobSetBuilder};

use crate::model::*;
use crate::{config, profiles};

#[derive(Debug)]
pub enum PlanError {
    MutatingRefused(String),
    UnsafePrerequisite { task: String, prerequisite: String },
    UnknownPrerequisite { task: String, prerequisite: String },
    PrerequisiteCycle(String),
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MutatingRefused(id) => {
                write!(f, "mutating task '{id}' refused in non-mutating command")
            }
            Self::UnsafePrerequisite { task, prerequisite } => {
                write!(f, "task '{task}' requires unsafe prerequisite '{prerequisite}'")
            }
            Self::UnknownPrerequisite { task, prerequisite } => {
                write!(f, "task '{task}' requires unknown task '{prerequisite}'")
            }
            Self::PrerequisiteCycle(id) => write!(f, "prerequisite cycle involving task '{id}'"),
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
    pub prerequisite_for: Option<String>,
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
    ExplicitSelection,
    NoWhenCondition,
    BeforeAfter,
    Prerequisite,
}

impl std::fmt::Display for PlanReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlanReason::ChangedFile => write!(f, "matched changed file(s)"),
            PlanReason::Always => write!(f, "always-run"),
            PlanReason::Force => write!(f, "force mode"),
            PlanReason::ExplicitSelection => write!(f, "explicitly selected"),
            PlanReason::NoWhenCondition => write!(f, "no when.changed condition"),
            PlanReason::BeforeAfter => write!(f, "before/after hook"),
            PlanReason::Prerequisite => write!(f, "prerequisite task"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub items: Vec<PlanItem>,
}

pub fn build_plan(nodes: &[ResolvedNode], req: &PlanRequest) -> Result<Plan, PlanError> {
    let phase = req.phase;
    let mode = req.mode;
    let changed_files: Option<Vec<String>> = if !req.files.is_empty() {
        Some(req.files.clone())
    } else {
        None
    };
    let changed_ref = changed_files.as_deref();
    let command_is_mutating = phase.is_mutating();
    let mut roots = BTreeSet::new();
    let mut items = Vec::new();
    let mut by_key: BTreeMap<String, (&ResolvedNode, &ResolvedTask)> = BTreeMap::new();
    let mut by_node_task: BTreeMap<(String, String), String> = BTreeMap::new();

    for node in nodes {
        for task in &node.tasks {
            let key = format!("{}.{}", node.id, task.config.id);
            by_node_task.insert((node.id.clone(), task.config.id.clone()), key.clone());
            by_node_task.insert(
                (
                    node.node_path.to_string_lossy().to_string(),
                    task.config.id.clone(),
                ),
                key.clone(),
            );
            by_key.insert(key, (node, task));
        }
    }

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
                step,
                item_type: PlanItemType::TaskBefore,
                context: node.context.clone(),
                reason: PlanReason::BeforeAfter,
                matched_files: Vec::new(),
                prerequisite_for: None,
            });
        }

        for task in &node.tasks {
            if task.config.phase != phase {
                continue;
            }

            // Profile filtering: skip tasks that don't match the requested profile
            if !task_matches_profile(task, req.profile.as_deref()) {
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

            roots.insert(format!("{}.{}", node.id, task.config.id));
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
                step,
                item_type: PlanItemType::TaskAfter,
                context: node.context.clone(),
                reason: PlanReason::BeforeAfter,
                matched_files: Vec::new(),
                prerequisite_for: None,
            });
        }
    }

    let mut ordered = Vec::new();
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    for root in roots {
        visit_task(
            &root,
            None,
            command_is_mutating,
            &by_key,
            &by_node_task,
            &mut visiting,
            &mut visited,
            &mut ordered,
        )?;
    }

    for (key, prerequisite_for) in ordered {
        let (node, task) = by_key[&key];
        let reason = if prerequisite_for.is_some() {
            PlanReason::Prerequisite
        } else {
            plan_reason(task, mode, changed_ref)
        };
        push_task_items(&mut items, node, task, phase, reason, prerequisite_for, changed_ref);
    }

    Ok(Plan { items })
}

fn visit_task<'a>(
    key: &str,
    prerequisite_for: Option<String>,
    command_is_mutating: bool,
    by_key: &BTreeMap<String, (&'a ResolvedNode, &'a ResolvedTask)>,
    by_node_task: &BTreeMap<(String, String), String>,
    visiting: &mut HashSet<String>,
    visited: &mut HashSet<String>,
    ordered: &mut Vec<(String, Option<String>)>,
) -> Result<(), PlanError> {
    if visited.contains(key) {
        return Ok(());
    }
    if !visiting.insert(key.to_string()) {
        return Err(PlanError::PrerequisiteCycle(key.to_string()));
    }
    let (node, task) = by_key.get(key).ok_or_else(|| PlanError::UnknownPrerequisite {
        task: prerequisite_for.clone().unwrap_or_else(|| key.to_string()),
        prerequisite: key.to_string(),
    })?;
    for req in task.config.requires.iter().flatten() {
        let req_key = resolve_ref(node, req, by_node_task).ok_or_else(|| {
            PlanError::UnknownPrerequisite {
                task: key.to_string(),
                prerequisite: req.task().to_string(),
            }
        })?;
        let (_, prerequisite) = by_key.get(&req_key).ok_or_else(|| {
            PlanError::UnknownPrerequisite {
                task: key.to_string(),
                prerequisite: req.task().to_string(),
            }
        })?;
        if !prerequisite_edge_allowed(command_is_mutating, prerequisite) {
            return Err(PlanError::UnsafePrerequisite {
                task: key.to_string(),
                prerequisite: req_key,
            });
        }
        visit_task(
            &req_key,
            Some(key.to_string()),
            command_is_mutating,
            by_key,
            by_node_task,
            visiting,
            visited,
            ordered,
        )?;
    }
    visiting.remove(key);
    visited.insert(key.to_string());
    ordered.push((key.to_string(), prerequisite_for));
    Ok(())
}

fn prerequisite_edge_allowed(command_is_mutating: bool, task: &ResolvedTask) -> bool {
    let is_mutating = task
        .config
        .mutates
        .unwrap_or_else(|| config::default_mutates(&task.config.phase));

    command_is_mutating || !is_mutating || profiles::is_safe_generated_source_prerequisite(task)
}

fn resolve_ref(
    node: &ResolvedNode,
    req: &TaskRef,
    by_node_task: &BTreeMap<(String, String), String>,
) -> Option<String> {
    let node_id = req.node().unwrap_or(&node.id);
    by_node_task.get(&(node_id.to_string(), req.task().to_string())).cloned()
}

fn plan_reason(task: &ResolvedTask, mode: RunMode, changed_ref: Option<&[String]>) -> PlanReason {
    let matched = compute_matched_files(task, changed_ref);
    let has_when_condition = task.config.when.as_ref().and_then(|w| w.changed.as_ref()).map(|c| !c.paths.is_empty()).unwrap_or(false);
    if mode == RunMode::Force { PlanReason::Force } else if task.config.always.unwrap_or(false) { PlanReason::Always } else if !matched.is_empty() { PlanReason::ChangedFile } else if !has_when_condition { PlanReason::NoWhenCondition } else { PlanReason::ExplicitSelection }
}

fn push_task_items(items: &mut Vec<PlanItem>, node: &ResolvedNode, task: &ResolvedTask, phase: Phase, reason: PlanReason, prerequisite_for: Option<String>, changed_ref: Option<&[String]>) {
    let task_chain_id = format!("{}.{}", node.id, task.config.id);
    let task_context = merge_context(&node.context, task.config.context.as_ref());
    for step_cfg in task.config.before.iter().flatten() {
        let step = Step::from(step_cfg);
        items.push(PlanItem { node_path: node.node_path.clone(), config_path: node.config_path.clone(), task_id: format!("{}.before", task.config.id), chain_id: task_chain_id.clone(), description: step.description.clone(), phase, step, item_type: PlanItemType::TaskBefore, context: task_context.clone(), reason: PlanReason::BeforeAfter, matched_files: Vec::new(), prerequisite_for: prerequisite_for.clone() });
    }
    let matched = compute_matched_files(task, changed_ref);
    items.push(PlanItem { node_path: node.node_path.clone(), config_path: node.config_path.clone(), task_id: task.config.id.clone(), chain_id: task_chain_id.clone(), description: task.config.description.clone().unwrap_or_default(), phase: task.config.phase, step: Step { kind: task_step_kind_from_config(&task.config), always: task.config.always.unwrap_or(false), description: task.config.description.clone().unwrap_or_default() }, item_type: PlanItemType::TaskAction, context: task_context.clone(), reason, matched_files: matched, prerequisite_for: prerequisite_for.clone() });
    for step_cfg in task.config.after.iter().flatten() {
        let step = Step::from(step_cfg);
        items.push(PlanItem { node_path: node.node_path.clone(), config_path: node.config_path.clone(), task_id: format!("{}.after", task.config.id), chain_id: task_chain_id.clone(), description: step.description.clone(), phase, step, item_type: PlanItemType::TaskAfter, context: task_context.clone(), reason: PlanReason::BeforeAfter, matched_files: Vec::new(), prerequisite_for: prerequisite_for.clone() });
    }
}

/// Check whether a task matches the requested profile.
///
/// Rules:
/// - If a task has explicit profiles, it must include the requested profile.
/// - If a task has no profiles, it matches only when no profile is requested
///   or when the requested profile is "manual".
/// - When no profile is requested, all tasks match.
pub fn task_matches_profile(task: &ResolvedTask, requested_profile: Option<&str>) -> bool {
    let task_profiles = match &task.config.profiles {
        Some(p) => p,
        None => {
            // No profiles declared: match only for manual or no profile
            return requested_profile.is_none_or(|p| p == "manual");
        }
    };

    match requested_profile {
        Some(p) => task_profiles.iter().any(|tp| tp == p),
        None => true,
    }
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
        TaskKind::ForbidText { paths, .. } | TaskKind::RequireText { paths, .. } => {
            !paths.is_empty()
        }
    }
}

pub fn merge_context(node: &ContextConfig, task: Option<&ContextConfig>) -> ContextConfig {
    let Some(task) = task else {
        return node.clone();
    };

    let mut env = node.env.clone().unwrap_or_default();
    if let Some(task_env) = &task.env {
        for (k, v) in task_env {
            env.insert(k.clone(), v.clone());
        }
    }

    ContextConfig {
        workdir: task.workdir.clone().or_else(|| node.workdir.clone()),
        env: if env.is_empty() { None } else { Some(env) },
        shell: task.shell.clone().or_else(|| node.shell.clone()),
    }
}

fn task_step_kind_from_config(cfg: &TaskConfig) -> crate::model::TaskKind {
    cfg.kind.clone()
}

pub fn task_matches_paths(patterns: &[String], changed_files: &[String]) -> bool {
    let mut builder = GlobSetBuilder::new();
    for p in patterns {
        let glob = match GlobBuilder::new(p).literal_separator(true).build() {
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
            profile: None,
            group: None,
            target: None,
            files: Vec::new(),
            offline: false,
            locked: false,
        }
    }

    fn make_command_task(
        id: &str,
        phase: Phase,
        mutates: bool,
        command: Vec<String>,
    ) -> ResolvedTask {
        ResolvedTask {
            config: TaskConfig {
                id: id.to_string(),
                description: None,
                phase,
                kind: TaskKind::Command {
                    command,
                    expect: None,
                },
                context: None,
                tags: None,
                profiles: None,
                mutates: Some(mutates),
                interactive: None,
                network: None,
                sandbox_safe: None,
                when: None,
                always: None,
                requires: None,
                before: None,
                after: None,
            },
            parent_node_path: Path::new(".").to_path_buf(),
        }
    }

    fn make_command_task_requiring(id: &str, requires: Vec<TaskRef>) -> ResolvedTask {
        let mut task = make_command_task(
            id,
            Phase::Verify,
            false,
            vec!["echo".to_string(), id.to_string()],
        );
        task.config.requires = Some(requires);
        task
    }

    fn make_safe_generated_task(id: &str) -> ResolvedTask {
        let mut task = make_command_task(
            id,
            Phase::Generate,
            true,
            vec!["tend".to_string(), "flake".to_string(), "write".to_string()],
        );
        task.config.tags = Some(vec!["generated-source".to_string()]);
        task.config.sandbox_safe = Some(true);
        task.config.interactive = Some(false);
        task.config.network = Some(false);
        task
    }

    fn make_command_task_with_context(
        id: &str,
        phase: Phase,
        mutates: bool,
        command: Vec<String>,
        context: Option<ContextConfig>,
    ) -> ResolvedTask {
        ResolvedTask {
            config: TaskConfig {
                id: id.to_string(),
                description: None,
                phase,
                kind: TaskKind::Command {
                    command,
                    expect: None,
                },
                context,
                tags: None,
                profiles: None,
                mutates: Some(mutates),
                interactive: None,
                network: None,
                sandbox_safe: None,
                when: None,
                always: None,
                requires: None,
                before: None,
                after: None,
            },
            parent_node_path: Path::new(".").to_path_buf(),
        }
    }

    fn make_node(id: &str, tasks: Vec<ResolvedTask>) -> ResolvedNode {
        ResolvedNode {
            config_path: Path::new(".tend.json").to_path_buf(),
            node_path: Path::new(".").to_path_buf(),
            id: id.to_string(),
            description: String::new(),
            tags: vec![],
            when: None,
            context: ContextConfig {
                workdir: None,
                env: None,
                shell: None,
            },
            before: vec![],
            after: vec![],
            tasks,
        }
    }

    #[test]
    fn test_mutating_task_refused_in_verify() {
        let node = make_node(
            "root",
            vec![make_command_task(
                "bad-task",
                Phase::Verify,
                true,
                vec!["touch".to_string(), "/tmp/evil".to_string()],
            )],
        );
        let result = build_plan(&[node], &req(Phase::Verify, RunMode::Full));
        assert!(result.is_err());
        match result {
            Err(PlanError::MutatingRefused(id)) => assert_eq!(id, "bad-task"),
            _ => panic!("expected MutatingRefused error"),
        }
    }

    #[test]
    fn test_mutating_task_allowed_in_fix() {
        let node = make_node(
            "root",
            vec![make_command_task(
                "ok-task",
                Phase::Fix,
                true,
                vec!["touch".to_string(), "/tmp/test".to_string()],
            )],
        );
        let result = build_plan(&[node], &req(Phase::Fix, RunMode::Full));
        assert!(result.is_ok());
    }

    #[test]
    fn test_prerequisite_runs_before_dependent() {
        let node = make_node(
            "root",
            vec![
                make_command_task_requiring("dependent", vec![TaskRef::Id("setup".to_string())]),
                make_command_task("setup", Phase::Verify, false, vec!["echo".to_string()]),
            ],
        );
        let plan = build_plan(&[node], &req(Phase::Verify, RunMode::Full)).unwrap();
        let actions: Vec<_> = plan
            .items
            .iter()
            .filter(|item| item.item_type == PlanItemType::TaskAction)
            .map(|item| item.task_id.as_str())
            .collect();
        assert_eq!(actions, vec!["setup", "dependent"]);
        assert_eq!(plan.items[0].reason, PlanReason::Prerequisite);
    }

    #[test]
    fn test_safe_generated_prerequisite_allowed_in_verify() {
        let node = make_node(
            "root",
            vec![
                make_command_task_requiring("dependent", vec![TaskRef::Id("generate".to_string())]),
                make_safe_generated_task("generate"),
            ],
        );
        let plan = build_plan(&[node], &req(Phase::Verify, RunMode::Full)).unwrap();
        let actions: Vec<_> = plan
            .items
            .iter()
            .filter(|item| item.item_type == PlanItemType::TaskAction)
            .map(|item| item.task_id.as_str())
            .collect();
        assert_eq!(actions, vec!["generate", "dependent"]);
    }

    #[test]
    fn test_unsafe_mutating_prerequisite_refused_in_verify() {
        let node = make_node(
            "root",
            vec![
                make_command_task_requiring("dependent", vec![TaskRef::Id("setup".to_string())]),
                make_command_task("setup", Phase::Generate, true, vec!["touch".to_string(), "x".to_string()]),
            ],
        );
        assert!(matches!(
            build_plan(&[node], &req(Phase::Verify, RunMode::Full)),
            Err(PlanError::UnsafePrerequisite { .. })
        ));
    }

    #[test]
    fn test_unknown_prerequisite_errors() {
        let node = make_node(
            "root",
            vec![make_command_task_requiring(
                "dependent",
                vec![TaskRef::Object {
                    node: None,
                    task: "missing".to_string(),
                }],
            )],
        );
        assert!(matches!(
            build_plan(&[node], &req(Phase::Verify, RunMode::Full)),
            Err(PlanError::UnknownPrerequisite { .. })
        ));
    }

    #[test]
    fn test_prerequisite_cycle_errors() {
        let node = make_node(
            "root",
            vec![
                make_command_task_requiring("a", vec![TaskRef::Id("b".to_string())]),
                make_command_task_requiring("b", vec![TaskRef::Id("a".to_string())]),
            ],
        );
        assert!(matches!(
            build_plan(&[node], &req(Phase::Verify, RunMode::Full)),
            Err(PlanError::PrerequisiteCycle(_))
        ));
    }

    fn make_task_with_profiles(id: &str, profiles: Vec<&str>) -> ResolvedTask {
        ResolvedTask {
            config: TaskConfig {
                id: id.to_string(),
                description: None,
                phase: Phase::Verify,
                kind: TaskKind::Command {
                    command: vec!["echo".to_string(), "hello".to_string()],
                    expect: None,
                },
                context: None,
                tags: None,
                profiles: Some(profiles.iter().map(|s| s.to_string()).collect()),
                mutates: Some(false),
                interactive: None,
                network: None,
                sandbox_safe: None,
                when: None,
                always: None,
                requires: None,
                before: None,
                after: None,
            },
            parent_node_path: Path::new(".").to_path_buf(),
        }
    }

    fn make_task_with_profiles_and_phase(
        id: &str,
        profiles: Vec<&str>,
        phase: Phase,
        mutates: bool,
    ) -> ResolvedTask {
        ResolvedTask {
            config: TaskConfig {
                id: id.to_string(),
                description: None,
                phase,
                kind: TaskKind::Command {
                    command: vec!["echo".to_string(), "hello".to_string()],
                    expect: None,
                },
                context: None,
                tags: None,
                profiles: Some(profiles.iter().map(|s| s.to_string()).collect()),
                mutates: Some(mutates),
                interactive: None,
                network: None,
                sandbox_safe: None,
                when: None,
                always: None,
                requires: None,
                before: None,
                after: None,
            },
            parent_node_path: Path::new(".").to_path_buf(),
        }
    }

    fn req_with_profile(phase: Phase, mode: RunMode, profile: Option<&str>) -> PlanRequest {
        PlanRequest {
            phase,
            mode,
            profile: profile.map(|s| s.to_string()),
            group: None,
            target: None,
            files: Vec::new(),
            offline: false,
            locked: false,
        }
    }

    #[test]
    fn test_git_hook_includes_format_config_checks() {
        let task = make_task_with_profiles("tend-validate", vec!["git-hook", "manual"]);
        let node = make_node("root", vec![task]);
        let plan = build_plan(
            &[node],
            &req_with_profile(Phase::Verify, RunMode::Full, Some("git-hook")),
        )
        .unwrap();
        assert_eq!(plan.items.len(), 1);
        assert_eq!(plan.items[0].task_id, "tend-validate");
    }

    #[test]
    fn test_git_hook_excludes_cargo_test() {
        let task = make_task_with_profiles("cargo-test", vec!["manual", "nix-check"]);
        let node = make_node("root", vec![task]);
        let plan = build_plan(
            &[node],
            &req_with_profile(Phase::Verify, RunMode::Full, Some("git-hook")),
        )
        .unwrap();
        assert_eq!(plan.items.len(), 0);
    }

    #[test]
    fn test_git_hook_excludes_nix_flake_check() {
        let task = make_task_with_profiles("nix-flake-check", vec!["manual"]);
        let node = make_node("root", vec![task]);
        let plan = build_plan(
            &[node],
            &req_with_profile(Phase::Verify, RunMode::Full, Some("git-hook")),
        )
        .unwrap();
        assert_eq!(plan.items.len(), 0);
    }

    #[test]
    fn test_pre_push_includes_cargo_check() {
        let task = make_task_with_profiles("cargo-check", vec!["pre-push", "manual"]);
        let node = make_node("root", vec![task]);
        let plan = build_plan(
            &[node],
            &req_with_profile(Phase::Verify, RunMode::Full, Some("pre-push")),
        )
        .unwrap();
        assert_eq!(plan.items.len(), 1);
    }

    #[test]
    fn test_pre_push_includes_cargo_clippy() {
        let task = make_task_with_profiles("cargo-clippy", vec!["pre-push", "manual"]);
        let node = make_node("root", vec![task]);
        let plan = build_plan(
            &[node],
            &req_with_profile(Phase::Verify, RunMode::Full, Some("pre-push")),
        )
        .unwrap();
        assert_eq!(plan.items.len(), 1);
    }

    #[test]
    fn test_nix_check_includes_cargo_test() {
        let task = make_task_with_profiles("cargo-test", vec!["nix-check", "manual"]);
        let node = make_node("root", vec![task]);
        let plan = build_plan(
            &[node],
            &req_with_profile(Phase::Verify, RunMode::Full, Some("nix-check")),
        )
        .unwrap();
        assert_eq!(plan.items.len(), 1);
    }

    #[test]
    fn test_nix_check_excludes_nix_flake_check() {
        let task = make_task_with_profiles("nix-flake-check", vec!["manual"]);
        let node = make_node("root", vec![task]);
        let plan = build_plan(
            &[node],
            &req_with_profile(Phase::Verify, RunMode::Full, Some("nix-check")),
        )
        .unwrap();
        assert_eq!(plan.items.len(), 0);
    }

    #[test]
    fn test_manual_includes_nix_flake_check() {
        let task = make_task_with_profiles("nix-flake-check", vec!["manual"]);
        let node = make_node("root", vec![task]);
        let plan = build_plan(
            &[node],
            &req_with_profile(Phase::Verify, RunMode::Full, Some("manual")),
        )
        .unwrap();
        assert_eq!(plan.items.len(), 1);
    }

    #[test]
    fn test_fix_includes_mutating_formatters() {
        // Fix profile tasks use Phase::Fix and allow mutating
        let task = make_task_with_profiles_and_phase("rustfmt-fix", vec!["fix"], Phase::Fix, true);
        let node = make_node("root", vec![task]);
        let plan = build_plan(
            &[node],
            &req_with_profile(Phase::Fix, RunMode::Full, Some("fix")),
        )
        .unwrap();
        assert_eq!(plan.items.len(), 1);
    }

    #[test]
    fn test_mutating_task_refused_in_verify_with_profile() {
        // Even with a profile, mutating tasks are refused in verify phase
        let task = make_task_with_profiles_and_phase(
            "mutating-task",
            vec!["nix-check"],
            Phase::Verify,
            true,
        );
        let node = make_node("root", vec![task]);
        let result = build_plan(
            &[node],
            &req_with_profile(Phase::Verify, RunMode::Full, Some("nix-check")),
        );
        assert!(result.is_err());
        match result {
            Err(PlanError::MutatingRefused(id)) => assert_eq!(id, "mutating-task"),
            _ => panic!("expected MutatingRefused error"),
        }
    }

    #[test]
    fn test_default_profile_manual_for_no_profiles() {
        let task = make_command_task(
            "legacy-task",
            Phase::Verify,
            false,
            vec!["echo".to_string(), "hi".to_string()],
        );
        let node = make_node("root", vec![task]);
        // Task with no profiles should match when profile is "manual"
        let plan = build_plan(
            std::slice::from_ref(&node),
            &req_with_profile(Phase::Verify, RunMode::Full, Some("manual")),
        )
        .unwrap();
        assert_eq!(plan.items.len(), 1);
        // Task with no profiles should NOT match when profile is "git-hook"
        let plan2 = build_plan(
            &[node],
            &req_with_profile(Phase::Verify, RunMode::Full, Some("git-hook")),
        )
        .unwrap();
        assert_eq!(plan2.items.len(), 0);
    }

    #[test]
    fn test_unknown_profile_results_in_empty_plan() {
        let task = make_task_with_profiles("cargo-check", vec!["manual"]);
        let node = make_node("root", vec![task]);
        let plan = build_plan(
            &[node],
            &req_with_profile(Phase::Verify, RunMode::Full, Some("non-existent")),
        )
        .unwrap();
        assert_eq!(plan.items.len(), 0);
    }

    #[test]
    fn test_merge_context_workdir_task_overrides() {
        let node = ContextConfig {
            workdir: Some(WorkdirPolicy::ConfigDir),
            env: None,
            shell: None,
        };
        let task = ContextConfig {
            workdir: Some(WorkdirPolicy::Relative("sub".to_string())),
            env: None,
            shell: None,
        };
        let merged = merge_context(&node, Some(&task));
        assert_eq!(
            format!("{:?}", merged.workdir),
            format!("{:?}", Some(WorkdirPolicy::Relative("sub".to_string())))
        );
    }

    #[test]
    fn test_merge_context_shell_task_overrides() {
        let node = ContextConfig {
            workdir: None,
            env: None,
            shell: Some(ShellConfig {
                flake: Some(".".to_string()),
                name: Some("default".to_string()),
                file: None,
                impure: None,
                accept_flake_config: None,
                extra_args: None,
                auto: None,
            }),
        };
        let task = ContextConfig {
            workdir: None,
            env: None,
            shell: Some(ShellConfig {
                flake: Some(".".to_string()),
                name: Some("test".to_string()),
                file: None,
                impure: None,
                accept_flake_config: None,
                extra_args: None,
                auto: None,
            }),
        };
        let merged = merge_context(&node, Some(&task));
        assert_eq!(merged.shell.as_ref().unwrap().name.as_deref(), Some("test"));
    }

    #[test]
    fn test_merge_context_env_overlaid() {
        let node = ContextConfig {
            workdir: None,
            env: Some([("A".to_string(), "1".to_string())].into()),
            shell: None,
        };
        let task = ContextConfig {
            workdir: None,
            env: Some([("B".to_string(), "2".to_string())].into()),
            shell: None,
        };
        let merged = merge_context(&node, Some(&task));
        let env = merged.env.unwrap();
        assert_eq!(env.get("A").unwrap(), "1");
        assert_eq!(env.get("B").unwrap(), "2");
    }

    #[test]
    fn test_merge_context_env_task_overrides_node() {
        let node = ContextConfig {
            workdir: None,
            env: Some([("KEY".to_string(), "node".to_string())].into()),
            shell: None,
        };
        let task = ContextConfig {
            workdir: None,
            env: Some([("KEY".to_string(), "task".to_string())].into()),
            shell: None,
        };
        let merged = merge_context(&node, Some(&task));
        assert_eq!(merged.env.unwrap().get("KEY").unwrap(), "task");
    }

    #[test]
    fn test_merge_context_shell_task_full_override() {
        let node = ContextConfig {
            workdir: None,
            env: None,
            shell: Some(ShellConfig {
                flake: Some(".".to_string()),
                name: Some("default".to_string()),
                file: None,
                impure: None,
                accept_flake_config: None,
                extra_args: None,
                auto: None,
            }),
        };
        let merged = merge_context(&node, None);
        assert_eq!(
            merged.shell.as_ref().unwrap().name.as_deref(),
            Some("default")
        );
    }

    #[test]
    fn test_task_context_shell_appears_in_plan() {
        let task_context = ContextConfig {
            workdir: None,
            env: None,
            shell: Some(ShellConfig {
                flake: Some(".".to_string()),
                name: Some("test".to_string()),
                file: None,
                impure: None,
                accept_flake_config: None,
                extra_args: None,
                auto: None,
            }),
        };
        let task = make_command_task_with_context(
            "shell-task",
            Phase::Verify,
            false,
            vec!["echo".to_string(), "hi".to_string()],
            Some(task_context),
        );
        let node = make_node("root", vec![task]);
        let plan = build_plan(&[node], &req(Phase::Verify, RunMode::Full)).unwrap();
        assert_eq!(plan.items.len(), 1);
        assert_eq!(
            plan.items[0]
                .context
                .shell
                .as_ref()
                .unwrap()
                .name
                .as_deref(),
            Some("test")
        );
    }
}
