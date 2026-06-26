use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::git;
use crate::graph::{FlakeNode, WorkspaceGraph, NodeId};
#[allow(unused_imports)]
use crate::model::{RepoAvailability, RepoStatus, WorkspaceConfig};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncCommitPlan {
    pub transaction_id: String,
    pub root: NodeId,
    pub affected_nodes: BTreeSet<NodeId>,
    pub commit_order: Vec<NodeId>,
    pub push_order: Vec<NodeId>,
    pub node_plans: BTreeMap<NodeId, NodeCommitPlan>,
    pub blocked_reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeCommitPlan {
    pub node: NodeId,
    pub dirty: bool,
    pub needs_code_commit: bool,
    pub needs_input_sync: bool,
    pub dependencies_to_update: Vec<InputUpdate>,
    pub message: String,
    pub validation_commands: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputUpdate {
    pub input_name: String,
    pub dependency_node: NodeId,
    pub target_rev: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionJournal {
    pub transaction_id: String,
    pub started_at: String,
    pub root: NodeId,
    pub phase: JournalPhase,
    pub nodes: BTreeMap<NodeId, NodeJournalEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum JournalPhase {
    Planned,
    Committed,
    Validated,
    Pushing,
    Completed,
    Failed,
}

impl std::fmt::Display for JournalPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JournalPhase::Planned => write!(f, "planned"),
            JournalPhase::Committed => write!(f, "committed"),
            JournalPhase::Validated => write!(f, "validated"),
            JournalPhase::Pushing => write!(f, "pushing"),
            JournalPhase::Completed => write!(f, "completed"),
            JournalPhase::Failed => write!(f, "failed"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeJournalEntry {
    pub path: String,
    pub commit_sha: Option<String>,
    pub pushed: bool,
}

fn generate_transaction_id() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let rand_suffix: u32 = rand_byte();
    format!("sync-{:x}-{:04x}", secs, rand_suffix)
}

fn rand_byte() -> u32 {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    (t & 0xFFFF) as u32
}

fn timestamp_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;
    let y = 1970_f64 + (days as f64 - 1.0) / 365.25;
    let year = y as u64;
    let remaining = days as u64 - ((year - 1970) * 365 + (year - 1969) / 4);
    let month_days = [31, if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 1;
    let mut day = remaining;
    for &md in &month_days {
        if day <= md { break; }
        day -= md;
        month += 1;
    }
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", year, month, day + 1, hours, minutes, seconds)
}

fn default_message(node_name: &str, sha: Option<&str>) -> String {
    match sha {
        Some(s) => format!("chore(stitch): sync DAG inputs\n\nIncludes updates from:\n- {}: {}", node_name, s),
        None => format!("chore(stitch): commit workspace changes"),
    }
}

fn input_sync_message(node_name: &str, updates: &[InputUpdate]) -> String {
    let mut msg = format!("chore(inputs): sync {} inputs", node_name);
    for u in updates {
        if let Some(rev) = &u.target_rev {
            msg.push_str(&format!("\n- {}: {}", u.dependency_node, rev));
        }
    }
    msg
}

pub fn plan_sync(
    graph: &WorkspaceGraph,
    statuses: &[RepoStatus],
    _cfg: &WorkspaceConfig,
) -> Result<SyncCommitPlan, String> {
    let mut blocked_reasons = Vec::new();
    let transaction_id = generate_transaction_id();
    let commit_order = graph.topological_order()?;
    let push_order = commit_order.clone();

    let dirty_nodes: BTreeSet<NodeId> = statuses
        .iter()
        .filter(|s| s.staged_count > 0 || s.unstaged_count > 0 || s.untracked_count > 0)
        .map(|s| s.name.clone())
        .collect();

    let mut affected_nodes: BTreeSet<NodeId> = dirty_nodes.clone();
    let mut plan_nodes: BTreeMap<NodeId, NodeCommitPlan> = BTreeMap::new();

    for node_id in &commit_order {
        let is_dirty = dirty_nodes.contains(node_id);
        let node = match graph.get_node(node_id) {
            Some(n) => n,
            None => continue,
        };

        let mut deps_to_update = Vec::new();
        for edge in graph.dependencies_of(node_id) {
            if affected_nodes.contains(&edge.to) {
                deps_to_update.push(InputUpdate {
                    input_name: edge.input_name.clone(),
                    dependency_node: edge.to.clone(),
                    target_rev: None,
                });
            }
        }

        let needs_code_commit = is_dirty;
        let needs_input_sync = !deps_to_update.is_empty();

        if needs_code_commit || needs_input_sync {
            affected_nodes.insert(node_id.clone());
        }

        let message = if needs_code_commit {
            default_message(node_id, None)
        } else if needs_input_sync {
            let short_updates: Vec<InputUpdate> = deps_to_update
                .iter()
                .map(|u| InputUpdate {
                    input_name: u.input_name.clone(),
                    dependency_node: u.dependency_node.clone(),
                    target_rev: u.target_rev.clone(),
                })
                .collect();
            input_sync_message(node_id, &short_updates)
        } else {
            String::new()
        };

        let validation_commands = load_validation_commands(node, graph)?;

        plan_nodes.insert(
            node_id.clone(),
            NodeCommitPlan {
                node: node_id.clone(),
                dirty: is_dirty,
                needs_code_commit,
                needs_input_sync,
                dependencies_to_update: deps_to_update,
                message,
                validation_commands,
            },
        );
    }

    for node_id in &commit_order {
        let node = match graph.get_node(node_id) {
            Some(n) => n,
            None => continue,
        };
        let plan = match plan_nodes.get(node_id) {
            Some(p) => p,
            None => continue,
        };
        if !plan.needs_code_commit && !plan.needs_input_sync {
            continue;
        }

        if node.branch == "HEAD" {
            blocked_reasons.push(format!("{}: detached HEAD (use --force)", node.name));
        }
    }

    let node_plans: BTreeMap<NodeId, NodeCommitPlan> = plan_nodes
        .into_iter()
        .filter(|(id, _)| affected_nodes.contains(id))
        .collect();

    let filtered_commit_order: Vec<NodeId> = commit_order
        .into_iter()
        .filter(|id| affected_nodes.contains(id))
        .collect();

    let filtered_push_order: Vec<NodeId> = push_order
        .into_iter()
        .filter(|id| affected_nodes.contains(id))
        .collect();

    Ok(SyncCommitPlan {
        transaction_id,
        root: graph.root.clone(),
        affected_nodes,
        commit_order: filtered_commit_order,
        push_order: filtered_push_order,
        node_plans,
        blocked_reasons,
    })
}

fn load_validation_commands(node: &FlakeNode, _graph: &WorkspaceGraph) -> Result<Vec<Vec<String>>, String> {
    let sync_path = node.path.join("sync.json");
    if sync_path.exists() {
        let content = std::fs::read_to_string(&sync_path)
            .map_err(|e| format!("Read sync.json: {}", e))?;
        let sync: crate::graph::SyncJson = serde_json::from_str(&content)
            .map_err(|e| format!("Parse sync.json: {}", e))?;
        let commands: Vec<Vec<String>> = sync
            .checks
            .iter()
            .map(|c| shlex_split(c))
            .collect();
        return Ok(commands);
    }
    Ok(Vec::new())
}

fn shlex_split(cmd: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    for c in cmd.chars() {
        match c {
            '"' => in_quote = !in_quote,
            ' ' if !in_quote => {
                if !current.is_empty() {
                    args.push(current.clone());
                    current.clear();
                }
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

pub struct SyncExecutionResult {
    pub transaction_id: String,
    pub created_commits: BTreeMap<String, String>,
    pub push_results: BTreeMap<String, Result<(), String>>,
    pub phase: JournalPhase,
}

pub fn execute_sync(
    plan: &SyncCommitPlan,
    graph: &WorkspaceGraph,
    cfg: &WorkspaceConfig,
    no_push: bool,
    messages: Option<&BTreeMap<String, String>>,
    force: bool,
) -> Result<SyncExecutionResult, String> {
    let mut journal = TransactionJournal {
        transaction_id: plan.transaction_id.clone(),
        started_at: timestamp_now(),
        root: plan.root.clone(),
        phase: JournalPhase::Planned,
        nodes: BTreeMap::new(),
    };

    for node_id in &plan.commit_order {
        let node = graph.get_node(node_id).ok_or_else(|| format!("Node '{}' not found", node_id))?;
        journal.nodes.insert(
            node_id.clone(),
            NodeJournalEntry {
                path: node.path.to_string_lossy().to_string(),
                commit_sha: None,
                pushed: false,
            },
        );
    }

    write_journal(&journal, cfg)?;

    if !force {
        for reason in &plan.blocked_reasons {
            if reason.contains("detached HEAD") {
                return Err(format!("Blocked: {}. Use --force to proceed.", reason));
            }
        }
    }

    for node_id in &plan.commit_order {
        let plan_node = plan.node_plans.get(node_id).ok_or_else(|| format!("No plan for node '{}'", node_id))?;
        if !plan_node.needs_code_commit && !plan_node.needs_input_sync {
            continue;
        }
        let node = graph.get_node(node_id).ok_or_else(|| format!("Node '{}' not found", node_id))?;
        let porcelain = git::git_porcelain(&node.path)?;
        for line in porcelain.lines() {
            if line.starts_with("?? ") {
                // Untracked files exist - warn but don't block
            }
        }
    }

    let mut commit_shas: BTreeMap<NodeId, String> = BTreeMap::new();

    journal.phase = JournalPhase::Committed;
    write_journal(&journal, cfg)?;

    for node_id in &plan.commit_order {
        let plan_node = match plan.node_plans.get(node_id) {
            Some(p) => p,
            None => continue,
        };

        let node = match graph.get_node(node_id) {
            Some(n) => n,
            None => continue,
        };

        let updated_deps: Vec<InputUpdate> = plan_node
            .dependencies_to_update
            .iter()
            .map(|u| {
                let rev = commit_shas.get(&u.dependency_node).cloned();
                InputUpdate {
                    input_name: u.input_name.clone(),
                    dependency_node: u.dependency_node.clone(),
                    target_rev: rev,
                }
            })
            .collect();

        if plan_node.needs_code_commit {
            let files = collect_all_changed_files(&node.path)?;
            if files.is_empty() {
                return Err(format!("{} marked dirty but no changes found", node.name));
            }

            let msg = messages
                .and_then(|m| m.get(node_id))
                .cloned()
                .unwrap_or_else(|| plan_node.message.clone());

            git::git_add(&node.path, &files)?;
            let trailed = crate::model::add_trailers(&msg, &plan.transaction_id, &cfg.workspace);
            git::git_commit(&node.path, &trailed)?;
            let sha = git::git_head(&node.path)?;
            commit_shas.insert(node_id.clone(), sha.clone());

            if let Some(entry) = journal.nodes.get_mut(node_id) {
                entry.commit_sha = Some(sha);
            }
            write_journal(&journal, cfg)?;
        }

        if plan_node.needs_input_sync && !updated_deps.is_empty() {
            for update in &updated_deps {
                let target_rev = update.target_rev.as_deref().unwrap_or("HEAD");
                update_flake_lock_input(&node.path, &update.input_name, target_rev)?;
            }

            let lock_path = node.path.join("flake.lock");
            if lock_path.exists() {
                let msg = messages
                    .and_then(|m| m.get(node_id))
                    .cloned()
                    .unwrap_or_else(|| plan_node.message.clone());

                git::git_add(&node.path, &[lock_path.to_string_lossy().to_string()])?;
                let trailed = crate::model::add_trailers(&msg, &plan.transaction_id, &cfg.workspace);
                git::git_commit(&node.path, &trailed)?;
                let sha = git::git_head(&node.path)?;
                commit_shas.insert(node_id.clone(), sha.clone());

                if let Some(entry) = journal.nodes.get_mut(node_id) {
                    entry.commit_sha = Some(sha);
                }
                write_journal(&journal, cfg)?;
            }
        }
    }

    journal.phase = JournalPhase::Validated;
    write_journal(&journal, cfg)?;

    for node_id in &plan.commit_order {
        let plan_node = match plan.node_plans.get(node_id) {
            Some(p) => p,
            None => continue,
        };
        if !plan_node.needs_code_commit && !plan_node.needs_input_sync {
            continue;
        }

        let node = match graph.get_node(node_id) {
            Some(n) => n,
            None => continue,
        };

        if !node.path.join(".git").exists() {
            continue;
        }
        let porcelain = git::git_porcelain(&node.path)?;
        let has_tracked_changes = porcelain.lines().any(|line| {
            if line.len() < 2 { return false; }
            let idx = line.as_bytes()[0] as char;
            let wt = line.as_bytes()[1] as char;
            // Only count staged changes (idx != space/?/!) as real tracked changes.
            // Worktree-only modifications (space + M) for submodules are expected
            // after sync commits that update submodule pointers.
            idx != ' ' && idx != '?' && idx != '!'
        });
        if has_tracked_changes {
            return Err(format!("Repo '{}' has uncommitted tracked changes after sync commit", node.name));
        }

        let lock_path = node.path.join("flake.lock");
        if lock_path.exists() && plan_node.needs_input_sync {
            for update in &plan_node.dependencies_to_update {
                let expected_rev = commit_shas.get(&update.dependency_node);
                verify_lockfile_rev(&lock_path, &update.input_name, expected_rev.map(|s| s.as_str()))?;
            }
        }

        for cmd_parts in &plan_node.validation_commands {
            if cmd_parts.is_empty() {
                continue;
            }
            let output = std::process::Command::new(&cmd_parts[0])
                .args(&cmd_parts[1..])
                .current_dir(&node.path)
                .output()
                .map_err(|e| format!("Validation command failed: {}", e))?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!(
                    "Validation failed in '{}': {} {}",
                    node.name,
                    cmd_parts.join(" "),
                    stderr.trim()
                ));
            }
        }
    }

    if no_push {
        return Ok(SyncExecutionResult {
            transaction_id: plan.transaction_id.clone(),
            created_commits: commit_shas,
            push_results: BTreeMap::new(),
            phase: JournalPhase::Committed,
        });
    }

    journal.phase = JournalPhase::Pushing;
    write_journal(&journal, cfg)?;

    let mut push_results: BTreeMap<String, Result<(), String>> = BTreeMap::new();
    for node_id in &plan.push_order {
        let plan_node = match plan.node_plans.get(node_id) {
            Some(p) => p,
            None => continue,
        };
        if !plan_node.needs_code_commit && !plan_node.needs_input_sync {
            continue;
        }

        let node = match graph.get_node(node_id) {
            Some(n) => n,
            None => continue,
        };

        let result = git_push(&node.path, &node.branch);
        if let Err(ref e) = result {
            push_results.insert(node.name.clone(), Err(e.clone()));
            journal.phase = JournalPhase::Failed;
            write_journal(&journal, cfg)?;
            let pushed_nodes: Vec<String> = push_results
                .iter()
                .filter(|(_, r)| r.is_ok())
                .map(|(name, _)| name.clone())
                .collect();
            let failed_nodes: Vec<String> = push_results
                .iter()
                .filter(|(_, r)| r.is_err())
                .map(|(name, _)| name.clone())
                .collect();
            return Err(format!(
                "Push failed for '{}': {}\nPushed: {}\nFailed: {}\nResume: stitch commit --sync --resume {}",
                node.name, e, pushed_nodes.join(", "), failed_nodes.join(", "), plan.transaction_id
            ));
        }

        push_results.insert(node.name.clone(), Ok(()));
        if let Some(entry) = journal.nodes.get_mut(node_id) {
            entry.pushed = true;
        }
        write_journal(&journal, cfg)?;
    }

    journal.phase = JournalPhase::Completed;
    write_journal(&journal, cfg)?;

    Ok(SyncExecutionResult {
        transaction_id: plan.transaction_id.clone(),
        created_commits: commit_shas,
        push_results,
        phase: JournalPhase::Completed,
    })
}

fn collect_all_changed_files(repo: &Path) -> Result<Vec<String>, String> {
    let mut files: Vec<String> = Vec::new();
    if let Ok(diff) = git::git_diff_names(repo) {
        for f in diff {
            if !files.contains(&f) {
                files.push(f);
            }
        }
    }
    if let Ok(staged) = git::git_diff_cached_names(repo) {
        for f in staged {
            if !files.contains(&f) {
                files.push(f);
            }
        }
    }
    let porcelain = git::git_porcelain(repo)?;
    for line in porcelain.lines() {
        let line = line.trim();
        if line.starts_with("?? ") {
            let f = line[3..].trim().to_string();
            if !files.contains(&f) {
                files.push(f);
            }
        }
    }
    files.sort();
    Ok(files)
}

fn update_flake_lock_input(repo_path: &Path, input_name: &str, rev: &str) -> Result<(), String> {
    let lock_path = repo_path.join("flake.lock");
    if !lock_path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(&lock_path)
        .map_err(|e| format!("Read flake.lock: {}", e))?;
    let lock: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Parse flake.lock: {}", e))?;

    let input_node = lock
        .get("nodes")
        .and_then(|n| n.get(input_name));

    let input_node = match input_node {
        Some(node) => node,
        None => {
            eprintln!("warning: input '{}' not found in flake.lock (dependency may be declared in sync.json but not as a flake input)", input_name);
            return Ok(());
        }
    };

    let original_url = input_node
        .get("original")
        .and_then(|o| o.get("url"))
        .and_then(|u| u.as_str());

    let locked_url = input_node
        .get("locked")
        .and_then(|l| l.get("url"))
        .and_then(|u| u.as_str());

    let output = std::process::Command::new("nix")
        .args(["flake", "update", input_name])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("nix flake update {}: {}", input_name, e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("warning: nix flake update failed, falling back to direct lock editing: {}", stderr.trim());
        return update_flake_lock_input_direct(repo_path, input_name, rev, original_url, locked_url);
    }

    let new_content = std::fs::read_to_string(&lock_path)
        .map_err(|e| format!("Read updated flake.lock: {}", e))?;
    let new_lock: serde_json::Value = serde_json::from_str(&new_content)
        .map_err(|e| format!("Parse updated flake.lock: {}", e))?;

    let new_rev = new_lock
        .get("nodes")
        .and_then(|n| n.get(input_name))
        .and_then(|n| n.get("locked"))
        .and_then(|l| l.get("rev"))
        .and_then(|r| r.as_str());

    match new_rev {
        Some(r) if r == rev => Ok(()),
        Some(r) => {
            eprintln!("note: input '{}' resolved to {} (expected {}), accepting", input_name, r, rev);
            Ok(())
        }
        None => {
            eprintln!("warning: nix flake update didn't set a rev for '{}', falling back", input_name);
            update_flake_lock_input_direct(repo_path, input_name, rev, original_url, locked_url)
        }
    }
}

fn update_flake_lock_input_direct(
    repo_path: &Path,
    input_name: &str,
    rev: &str,
    _original_url: Option<&str>,
    _locked_url: Option<&str>,
) -> Result<(), String> {
    let lock_path = repo_path.join("flake.lock");
    let content = std::fs::read_to_string(&lock_path)
        .map_err(|e| format!("Read flake.lock: {}", e))?;
    let mut lock: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Parse flake.lock: {}", e))?;

    let nodes = lock
        .get_mut("nodes")
        .and_then(|n| n.as_object_mut())
        .ok_or_else(|| "flake.lock missing 'nodes' section".to_string())?;

    let node = nodes
        .get_mut(input_name)
        .ok_or_else(|| format!("Input '{}' not found in flake.lock", input_name))?;

    if let Some(locked) = node.get_mut("locked").and_then(|l| l.as_object_mut()) {
        locked.insert("rev".to_string(), serde_json::Value::String(rev.to_string()));
        locked.remove("narHash");
        locked.remove("lastModified");
        locked.remove("revCount");
    }

    std::fs::write(&lock_path, serde_json::to_string_pretty(&lock).map_err(|e| format!("Serialize flake.lock: {}", e))?)
        .map_err(|e| format!("Write flake.lock: {}", e))?;

    Ok(())
}

fn verify_lockfile_rev(
    lock_path: &Path,
    input_name: &str,
    expected_rev: Option<&str>,
) -> Result<(), String> {
    if !lock_path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(lock_path)
        .map_err(|e| format!("Read flake.lock: {}", e))?;
    let lock: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Parse flake.lock: {}", e))?;

    let rev = lock
        .get("nodes")
        .and_then(|n| n.get(input_name))
        .and_then(|n| n.get("locked"))
        .and_then(|l| l.get("rev"))
        .and_then(|r| r.as_str());

    match (rev, expected_rev) {
        (Some(actual), Some(expected)) => {
            if actual != expected {
                return Err(format!(
                    "flake.lock input '{}' has rev '{}', expected '{}'",
                    input_name, actual, expected
                ));
            }
        }
        (Some(_), None) => {}
        (None, Some(expected)) => {
            return Err(format!(
                "flake.lock input '{}' has no 'rev' field, expected '{}'",
                input_name, expected
            ));
        }
        (None, None) => {}
    }

    Ok(())
}

fn git_push(repo: &Path, branch: &str) -> Result<(), String> {
    crate::git::git_push(repo, branch)
}

pub fn resume_sync(
    transaction_id: &str,
    cfg: &WorkspaceConfig,
    no_push: bool,
) -> Result<SyncExecutionResult, String> {
    let journal = load_journal(transaction_id, cfg)?
        .ok_or_else(|| format!("Transaction '{}' not found", transaction_id))?;

    let graph = crate::graph::discover_graph(cfg)?;
    let statuses = crate::status::collect_all(cfg)?;

    let plan = plan_sync(&graph, &statuses, cfg)?;

    if journal.phase == JournalPhase::Pushing || journal.phase == JournalPhase::Committed || journal.phase == JournalPhase::Validated {
        let mut push_results: BTreeMap<String, Result<(), String>> = BTreeMap::new();
        let mut any_failed = false;
        for node_id in &plan.push_order {
            let entry = match journal.nodes.get(node_id) {
                Some(e) => e,
                None => continue,
            };
            if entry.pushed {
                push_results.insert(node_id.clone(), Ok(()));
                continue;
            }

            if no_push {
                push_results.insert(node_id.clone(), Err("Skipped (--no-push)".to_string()));
                continue;
            }

            let node = match graph.get_node(node_id) {
                Some(n) => n,
                None => continue,
            };
            let result = git_push(&node.path, &node.branch);
            if let Err(ref e) = result {
                push_results.insert(node_id.clone(), Err(e.clone()));
                any_failed = true;
                break;
            }
            push_results.insert(node_id.clone(), Ok(()));
        }

        if any_failed {
            return Ok(SyncExecutionResult {
                transaction_id: transaction_id.to_string(),
                created_commits: journal.nodes.iter().filter_map(|(id, e)| {
                    e.commit_sha.clone().map(|sha| (id.clone(), sha))
                }).collect(),
                push_results,
                phase: JournalPhase::Failed,
            });
        }

        return Ok(SyncExecutionResult {
            transaction_id: transaction_id.to_string(),
            created_commits: journal.nodes.iter().filter_map(|(id, e)| {
                e.commit_sha.clone().map(|sha| (id.clone(), sha))
            }).collect(),
            push_results,
            phase: JournalPhase::Completed,
        });
    }

    Err(format!("Transaction '{}' is in phase '{}' and cannot be resumed", transaction_id, journal.phase))
}

fn journal_dir(cfg: &WorkspaceConfig) -> Result<std::path::PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("Cannot get cwd: {}", e))?;
    let root_path = cfg
        .repos
        .iter()
        .find(|r| r.name == cfg.workspace || r.name == "phenix" || r.path == ".")
        .map(|r| r.resolved_path(cfg))
        .unwrap_or_else(|| cwd.clone());

    let dir = root_path.join(".stitch").join("transactions");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Create transaction dir: {}", e))?;
    Ok(dir)
}

fn write_journal(journal: &TransactionJournal, cfg: &WorkspaceConfig) -> Result<(), String> {
    let dir = journal_dir(cfg)?;
    let path = dir.join(format!("{}.json", journal.transaction_id));
    let content = serde_json::to_string_pretty(journal)
        .map_err(|e| format!("Serialize journal: {}", e))?;
    std::fs::write(&path, content).map_err(|e| format!("Write journal: {}", e))?;
    Ok(())
}

fn load_journal(transaction_id: &str, cfg: &WorkspaceConfig) -> Result<Option<TransactionJournal>, String> {
    let dir = journal_dir(cfg)?;
    let path = dir.join(format!("{}.json", transaction_id));
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Read journal: {}", e))?;
    let journal: TransactionJournal = serde_json::from_str(&content)
        .map_err(|e| format!("Parse journal: {}", e))?;
    Ok(Some(journal))
}

pub fn format_plan_output(plan: &SyncCommitPlan, json_output: bool) -> String {
    if json_output {
        return serde_json::to_string_pretty(&serde_json::json!({
            "decision": if plan.blocked_reasons.is_empty() { "ready" } else { "blocked" },
            "root": plan.root,
            "transaction_id": plan.transaction_id,
            "nodes": plan.node_plans.iter().map(|(id, np)| {
                serde_json::json!({
                    "name": id,
                    "dirty": np.dirty,
                    "commit_required": np.needs_code_commit,
                    "sync_update_required": np.needs_input_sync,
                    "message": np.message,
                })
            }).collect::<Vec<_>>(),
            "commit_order": plan.commit_order,
            "push_order": plan.push_order,
            "blocked_reasons": plan.blocked_reasons,
        }))
        .unwrap_or_default();
    }

    let mut output = String::new();
    output.push_str(&format!("Sync Plan: {}\n", plan.transaction_id));
    output.push_str(&format!("Root: {}\n\n", plan.root));

    if !plan.blocked_reasons.is_empty() {
        output.push_str("BLOCKED:\n");
        for r in &plan.blocked_reasons {
            output.push_str(&format!("  - {}\n", r));
        }
        output.push('\n');
    }

    output.push_str("Nodes:\n");
    for id in &plan.commit_order {
        let np = match plan.node_plans.get(id) {
            Some(p) => p,
            None => continue,
        };
        let actions: Vec<&str> = {
            let mut a = Vec::new();
            if np.needs_code_commit { a.push("commit"); }
            if np.needs_input_sync { a.push("sync-inputs"); }
            a
        };
        output.push_str(&format!("  {}: {}\n", id, actions.join(" + ")));
        if np.needs_input_sync {
            for u in &np.dependencies_to_update {
                output.push_str(&format!("    update input '{}' -> {}\n", u.input_name, u.dependency_node));
            }
        }
        output.push_str(&format!("    message: {}\n", np.message));
    }

    output.push_str(&format!("\nCommit order: {}\n", plan.commit_order.join(" -> ")));
    output.push_str(&format!("Push order:   {}\n", plan.push_order.join(" -> ")));

    output
}

pub fn format_result_output(result: &SyncExecutionResult, json_output: bool) -> String {
    if json_output {
        return serde_json::to_string_pretty(&serde_json::json!({
            "decision": match result.phase {
                JournalPhase::Completed => "completed",
                JournalPhase::Failed => "failed",
                JournalPhase::Committed => "committed",
                _ => "in_progress",
            },
            "transaction_id": result.transaction_id,
            "created_commits": result.created_commits,
            "push_results": result.push_results.iter().map(|(name, r)| {
                serde_json::json!({
                    "node": name,
                    "success": r.is_ok(),
                    "error": r.as_ref().err(),
                })
            }).collect::<Vec<_>>(),
        }))
        .unwrap_or_default();
    }

    let mut output = String::new();
    output.push_str(&format!("Transaction: {}\n", result.transaction_id));
    output.push_str(&format!("Phase: {}\n\n", result.phase));

    if !result.created_commits.is_empty() {
        output.push_str("Commits:\n");
        for (name, sha) in &result.created_commits {
            output.push_str(&format!("  {}: {}\n", name, sha));
        }
        output.push('\n');
    }

    if !result.push_results.is_empty() {
        output.push_str("Push results:\n");
        for (name, r) in &result.push_results {
            match r {
                Ok(()) => output.push_str(&format!("  {}: pushed\n", name)),
                Err(e) => output.push_str(&format!("  {}: FAILED - {}\n", name, e)),
            }
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph;

    fn make_test_graph() -> WorkspaceGraph {
        let mut nodes = BTreeMap::new();
        for name in &["tools", "shell", "root"] {
            let dir = std::env::temp_dir().join(format!("__sync_test_{}", name));
            nodes.insert(
                name.to_string(),
                graph::FlakeNode {
                    id: name.to_string(),
                    name: name.to_string(),
                    path: dir,
                    remote: None,
                    branch: "main".to_string(),
                },
            );
        }
        let edges = vec![
            graph::DependencyEdge::new("shell", "tools", "tools"),
            graph::DependencyEdge::new("root", "shell", "shell"),
            graph::DependencyEdge::new("root", "tools", "tools"),
        ];
        WorkspaceGraph {
            root: "root".to_string(),
            nodes,
            edges,
        }
    }

    fn make_status(name: &str, dirty: bool) -> RepoStatus {
        RepoStatus {
            name: name.to_string(),
            path: format!("/tmp/{}", name),
            branch: "main".to_string(),
            is_dirty: dirty,
            status: RepoAvailability::Present,
            staged_count: if dirty { 1 } else { 0 },
            unstaged_count: 0,
            untracked_count: 0,
            ahead: None,
            behind: None,
        }
    }

    #[test]
    fn test_plan_sync_dirty_tools() {
        let graph = make_test_graph();
        let statuses = vec![make_status("tools", true), make_status("shell", false), make_status("root", false)];
        let cfg = WorkspaceConfig { version: 1, workspace: "test".to_string(), repos: vec![], config_dir: None };
        let plan = plan_sync(&graph, &statuses, &cfg).unwrap();
        assert_eq!(plan.commit_order, vec!["tools", "shell", "root"]);
        let tools = plan.node_plans.get("tools").unwrap();
        assert!(tools.needs_code_commit);
        assert!(!tools.needs_input_sync);
        let shell = plan.node_plans.get("shell").unwrap();
        assert!(!shell.needs_code_commit);
        assert!(shell.needs_input_sync);
        let root = plan.node_plans.get("root").unwrap();
        assert!(!root.needs_code_commit);
        assert!(root.needs_input_sync);
    }

    #[test]
    fn test_plan_sync_dirty_shell() {
        let graph = make_test_graph();
        let statuses = vec![make_status("tools", false), make_status("shell", true), make_status("root", false)];
        let cfg = WorkspaceConfig { version: 1, workspace: "test".to_string(), repos: vec![], config_dir: None };
        let plan = plan_sync(&graph, &statuses, &cfg).unwrap();
        assert_eq!(plan.commit_order, vec!["shell", "root"]);
        let shell = plan.node_plans.get("shell").unwrap();
        assert!(shell.needs_code_commit);
        assert!(!shell.needs_input_sync);
        let root = plan.node_plans.get("root").unwrap();
        assert!(!root.needs_code_commit);
        assert!(root.needs_input_sync);
    }

    #[test]
    fn test_plan_sync_dirty_root() {
        let graph = make_test_graph();
        let statuses = vec![make_status("tools", false), make_status("shell", false), make_status("root", true)];
        let cfg = WorkspaceConfig { version: 1, workspace: "test".to_string(), repos: vec![], config_dir: None };
        let plan = plan_sync(&graph, &statuses, &cfg).unwrap();
        assert_eq!(plan.commit_order, vec!["root"]);
        let root = plan.node_plans.get("root").unwrap();
        assert!(root.needs_code_commit);
        assert!(!root.needs_input_sync);
    }

    #[test]
    fn test_plan_sync_dirty_tools_and_shell() {
        let graph = make_test_graph();
        let statuses = vec![make_status("tools", true), make_status("shell", true), make_status("root", false)];
        let cfg = WorkspaceConfig { version: 1, workspace: "test".to_string(), repos: vec![], config_dir: None };
        let plan = plan_sync(&graph, &statuses, &cfg).unwrap();
        assert_eq!(plan.commit_order, vec!["tools", "shell", "root"]);
        let tools = plan.node_plans.get("tools").unwrap();
        assert!(tools.needs_code_commit);
        assert!(!tools.needs_input_sync);
        let shell = plan.node_plans.get("shell").unwrap();
        assert!(shell.needs_code_commit);
        assert!(shell.needs_input_sync);
        let root = plan.node_plans.get("root").unwrap();
        assert!(!root.needs_code_commit);
        assert!(root.needs_input_sync);
    }

    #[test]
    fn test_plan_sync_no_dirty() {
        let graph = make_test_graph();
        let statuses = vec![make_status("tools", false), make_status("shell", false), make_status("root", false)];
        let cfg = WorkspaceConfig { version: 1, workspace: "test".to_string(), repos: vec![], config_dir: None };
        let plan = plan_sync(&graph, &statuses, &cfg).unwrap();
        assert!(plan.commit_order.is_empty());
    }

    #[test]
    fn test_plan_push_order_equals_commit_order() {
        let graph = make_test_graph();
        let statuses = vec![make_status("tools", true), make_status("shell", true), make_status("root", true)];
        let cfg = WorkspaceConfig { version: 1, workspace: "test".to_string(), repos: vec![], config_dir: None };
        let plan = plan_sync(&graph, &statuses, &cfg).unwrap();
        assert_eq!(plan.commit_order, plan.push_order);
    }

    #[test]
    fn test_update_flake_lock_input_basic() {
        use std::io::Write;
        let dir = std::env::temp_dir().join("__sync_test_flake_lock");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let lock_content = r#"{
            "nodes": {
                "phenix-pins": {
                    "locked": {
                        "rev": "abc123",
                        "narHash": "sha256-xxx"
                    },
                    "original": {
                        "url": "github:user/pins"
                    }
                }
            }
        }"#;
        let mut f = std::fs::File::create(dir.join("flake.lock")).unwrap();
        f.write_all(lock_content.as_bytes()).unwrap();

        // This will try nix first, fail, and fall back to direct editing
        update_flake_lock_input(&dir, "phenix-pins", "def4567890123456789012345678901234567890").unwrap();

        let content = std::fs::read_to_string(dir.join("flake.lock")).unwrap();
        let lock: serde_json::Value = serde_json::from_str(&content).unwrap();
        let rev = lock["nodes"]["phenix-pins"]["locked"]["rev"].as_str().unwrap().to_string();
        assert_eq!(rev, "def4567890123456789012345678901234567890");
        assert!(lock["nodes"]["phenix-pins"]["locked"].get("narHash").is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_input_sync_message() {
        let updates = vec![
            InputUpdate {
                input_name: "tools".to_string(),
                dependency_node: "phenix-tools".to_string(),
                target_rev: Some("abc123".to_string()),
            },
        ];
        let msg = input_sync_message("phenix-shell", &updates);
        assert!(msg.contains("phenix-tools"));
        assert!(msg.contains("abc123"));
    }

    #[test]
    fn test_generate_transaction_id() {
        let id = generate_transaction_id();
        assert!(id.starts_with("sync-"));
        assert!(id.len() > 10);
    }
}
