#![allow(clippy::too_many_arguments)]

use std::collections::BTreeMap;

use clap::{Parser, Subcommand};

use stitch::config;
use stitch::git;
use stitch::graph;
use stitch::status;
use stitch::sync;
use stitch::sync::Action;

fn main() {
    let cli = Cli::parse();

    let result = match &cli.command {
        Commands::Repos => cmd_repos(),
        Commands::Status {
            json,
            short,
            dirty_only,
            repo,
        } => cmd_status(*json, *short, *dirty_only, repo.as_deref()),
        Commands::Diff { repo, staged, json } => cmd_diff(repo.as_deref(), *staged, *json),
        Commands::Dag { mode, split, json } => cmd_dag(mode.as_deref(), split.as_deref(), *json),
        Commands::Commit {
            dry_run,
            json: json_output,
            apply,
            force,
            resume,
            messages,
        } => cmd_commit(
            *dry_run,
            *json_output,
            *apply,
            *force,
            resume.as_deref(),
            messages.as_deref(),
        ),
        Commands::Push {
            dry_run,
            json: json_output,
        } => cmd_push(*dry_run, *json_output),
        Commands::Sync {
            mode,
            apply,
            dry_run,
            json: json_output,
            no_push,
            repos,
            run_tend,
        } => cmd_sync(
            mode.as_deref(),
            *apply,
            *dry_run,
            *json_output,
            *no_push,
            repos,
            *run_tend,
        ),
        Commands::Changeset { command } => cmd_changeset(command),
    };

    if let Err(e) = result {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}

fn cmd_changeset(command: &ChangesetCliCommand) -> Result<(), String> {
    match command {
        ChangesetCliCommand::New { title } => stitch::changeset::new::execute(title),
        ChangesetCliCommand::Status { json } => {
            let cs = stitch::changeset::load_current()?;
            match cs {
                Some(cs) => {
                    if *json {
                        let output = serde_json::to_string_pretty(&cs)
                            .map_err(|e| format!("JSON: {}", e))?;
                        println!("{}", output);
                    } else {
                        println!("Changeset: {} ({})", cs.id, cs.title);
                        println!("State: {}", cs.state);
                        println!("Workspace: {}", cs.workspace);
                        println!();
                        for rp in &cs.repos {
                            let action = rp.action.as_deref().unwrap_or("-");
                            let msg = rp.message.as_deref().unwrap_or("<missing>");
                            let hash = rp.commit_hash.as_deref().unwrap_or("-");
                            println!(
                                "  {}  action={}  message={}  hash={}",
                                rp.name, action, msg, hash
                            );
                        }
                    }
                }
                None => {
                    println!("No active changeset.");
                }
            }
            Ok(())
        }
        ChangesetCliCommand::Plan { write, json } => {
            stitch::changeset::plan::execute(*write, *json)
        }
        ChangesetCliCommand::SetMessage { repo, message } => {
            stitch::changeset::set_message::execute(repo, message)
        }
        ChangesetCliCommand::SetFiles { repo, files } => {
            stitch::changeset::set_files::execute(repo, files)
        }
        ChangesetCliCommand::Validate { json } => stitch::changeset::validate::execute(*json),
        ChangesetCliCommand::Commit => stitch::changeset::commit::execute(),
        ChangesetCliCommand::Push => stitch::changeset::push::execute(),
        ChangesetCliCommand::Abort => stitch::changeset::abort::execute(),
    }
}

#[derive(Parser)]
#[command(
    name = "stitch",
    version,
    about = "Multi-repo Git coordinator for Phenix workspaces"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List configured repos
    Repos,
    /// Show multi-repo status (like git status across all repos)
    Status {
        #[arg(long, help = "Output as JSON")]
        json: bool,
        #[arg(long, help = "Short format")]
        short: bool,
        #[arg(long, help = "Dirty repos only")]
        dirty_only: bool,
        #[arg(long, help = "Filter by repo name")]
        repo: Option<String>,
    },
    /// Show diffs across repos
    Diff {
        #[arg(long, help = "Repo name")]
        repo: Option<String>,
        #[arg(long, help = "Show staged changes only")]
        staged: bool,
        #[arg(long, help = "Output as JSON")]
        json: bool,
    },
    /// Show ordered operation DAG (read-only)
    Dag {
        #[arg(long, default_value = "commit", help = "DAG mode: commit, sync, full")]
        mode: Option<String>,
        #[arg(
            long,
            default_value = "by-repo",
            help = "Split strategy: by-repo, by-path"
        )]
        split: Option<String>,
        #[arg(long, help = "Output as JSON")]
        json: bool,
    },
    /// Commit changed files in DAG dependency order (local commits only)
    Commit {
        #[arg(long, help = "Dry run (show plan, no mutations)")]
        dry_run: bool,
        #[arg(long, help = "JSON output for agent usage")]
        json: bool,
        #[arg(long, help = "Apply (required for actual commits)")]
        apply: bool,
        #[arg(long, help = "Allow edge cases like detached HEAD")]
        force: bool,
        #[arg(long, help = "Transaction ID to resume")]
        resume: Option<String>,
        #[arg(long, help = "Path to JSON messages file (from commit_template)")]
        messages: Option<String>,
    },
    /// Push committed changes in DAG dependency order
    Push {
        #[arg(long, help = "Dry run (show what would be pushed)")]
        dry_run: bool,
        #[arg(long, help = "Output as JSON")]
        json: bool,
    },
    /// Sync/update/push in dependency order (update flake inputs, validate, push)
    Sync {
        #[arg(long, default_value = "push", help = "Mode: push (pull/rebase planned)")]
        mode: Option<String>,
        #[arg(long, help = "Apply (required for actual sync operations)")]
        apply: bool,
        #[arg(long, help = "Dry run (show plan, no mutations)")]
        dry_run: bool,
        #[arg(long, help = "JSON output for agent usage")]
        json: bool,
        #[arg(long, help = "Skip push step")]
        no_push: bool,
        #[arg(long, help = "Filter by repo names")]
        repos: Vec<String>,
        #[arg(long, help = "Run tend checks before sync")]
        run_tend: bool,
    },
    /// Manage changesets (legacy, hidden)
    #[command(hide = true)]
    Changeset {
        #[command(subcommand)]
        command: ChangesetCliCommand,
    },
}

#[derive(Subcommand)]
enum ChangesetCliCommand {
    /// Create a new changeset
    New {
        /// Title for the new changeset
        title: String,
    },
    /// Show current changeset status
    Status {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Build/review a plan for the current changeset
    Plan {
        /// Write the plan to the active changeset
        #[arg(long)]
        write: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Set commit message for a repo in the changeset
    SetMessage {
        /// Repo name
        repo: String,
        /// Commit message
        message: String,
    },
    /// Set tracked files for a repo in the changeset
    SetFiles {
        /// Repo name
        repo: String,
        /// Files to track
        files: Vec<String>,
    },
    /// Validate the current changeset
    Validate {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Commit the validated changeset
    Commit,
    /// Push committed changeset repos
    Push,
    /// Abort the current changeset
    Abort,
}

fn cmd_repos() -> Result<(), String> {
    let cfg = config::find_and_load()?;
    for repo in &cfg.repos {
        let exists = if repo.resolved_path(&cfg).exists() {
            "\u{2713}"
        } else {
            "\u{2717}"
        };
        println!("{}  {}  ({})", exists, repo.name, repo.path);
    }
    Ok(())
}

fn cmd_status(
    json: bool,
    short: bool,
    dirty_only: bool,
    repo_filter: Option<&str>,
) -> Result<(), String> {
    let cfg = config::find_and_load()?;
    let statuses = status::collect_all(&cfg)?;

    if json {
        let filtered: Vec<_> = statuses
            .iter()
            .filter(|s| repo_filter.is_none_or(|r| s.name == r))
            .filter(|s| !dirty_only || s.is_dirty)
            .collect();
        let output = serde_json::to_string_pretty(&filtered).map_err(|e| format!("JSON: {}", e))?;
        println!("{}", output);
        return Ok(());
    }

    if short {
        for s in &statuses {
            if repo_filter.is_none_or(|r| s.name == r) && (!dirty_only || s.is_dirty) {
                let prefix = if s.is_dirty { "M" } else { " " };
                println!("{}  {}  {}", prefix, s.name, s.branch);
            }
        }
        return Ok(());
    }

    println!("Workspace: {}", cfg.workspace);
    println!();
    for s in &statuses {
        if repo_filter.is_none_or(|r| s.name == r) && (!dirty_only || s.is_dirty) {
            let dirty = if s.is_dirty { "yes" } else { "no" };
            println!("{}", s.name);
            println!("  branch: {}", s.branch);
            println!("  dirty: {}", dirty);
            println!("  staged: {}", s.staged_count);
            println!("  unstaged: {}", s.unstaged_count);
            println!("  untracked: {}", s.untracked_count);
            if s.is_dirty {
                let path = cfg
                    .repos
                    .iter()
                    .find(|r| r.name == s.name)
                    .map(|r| r.resolved_path(&cfg));
                if let Some(p) = path {
                    let diff = git::git_diff_names(&p).unwrap_or_default();
                    for f in &diff {
                        println!("    M {}", f);
                    }
                }
            }
            if let Some(ref ahead) = s.ahead {
                println!("  ahead: {}", ahead);
            }
            if let Some(ref behind) = s.behind {
                println!("  behind: {}", behind);
            }
            println!();
        }
    }
    Ok(())
}

fn cmd_diff(repo: Option<&str>, staged: bool, json: bool) -> Result<(), String> {
    let cfg = config::find_and_load()?;

    let target_repos: Vec<_> = if let Some(name) = repo {
        vec![cfg
            .repos
            .iter()
            .find(|r| r.name == name)
            .ok_or_else(|| format!("Repo '{}' not found", name))?]
    } else {
        cfg.repos.iter().collect()
    };

    let mut all_diffs: Vec<serde_json::Value> = Vec::new();

    for r in &target_repos {
        let path = r.resolved_path(&cfg);
        if !path.join(".git").exists() {
            continue;
        }

        let mut args = vec!["diff"];
        if staged {
            args.push("--cached");
        }

        let output = std::process::Command::new("git")
            .args(&args)
            .current_dir(&path)
            .output()
            .map_err(|e| format!("git diff: {}", e))?;

        let diff_text = String::from_utf8_lossy(&output.stdout).to_string();
        if diff_text.trim().is_empty() {
            continue;
        }

        if json {
            all_diffs.push(serde_json::json!({
                "repo": r.name, "diff": diff_text
            }));
        } else {
            println!("--- {} ---", r.name);
            println!("{}", diff_text);
        }
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "diffs": all_diffs
            }))
            .unwrap()
        );
    }
    Ok(())
}

fn cmd_dag(mode: Option<&str>, split: Option<&str>, json: bool) -> Result<(), String> {
    let mode = mode.unwrap_or("commit");
    let split = split.unwrap_or("by-repo");
    let cfg = config::find_and_load()?;
    let statuses = status::collect_all(&cfg)?;

    let mut nodes: Vec<serde_json::Value> = Vec::new();

    for s in &statuses {
        if !s.is_dirty {
            continue;
        }
        let repo_cfg = cfg.repos.iter().find(|r| r.name == s.name);
        let diff = repo_cfg
            .map(|r| {
                let p = r.resolved_path(&cfg);
                git::git_diff_names(&p).unwrap_or_default()
            })
            .unwrap_or_default();

        if mode != "sync" {
            nodes.push(serde_json::json!({
                "id": format!("{}:precheck", s.name),
                "kind": "check", "repo": s.name,
                "command": ["tend", "run", "--changed"],
                "depends_on": []
            }));
        }

        match split {
            "by-path" => {
                let mut by_dir: std::collections::HashMap<String, Vec<String>> =
                    std::collections::HashMap::new();
                for f in &diff {
                    let dir = f
                        .rfind('/')
                        .map(|i| f[..i].to_string())
                        .unwrap_or_else(|| "root".to_string());
                    by_dir.entry(dir).or_default().push(f.clone());
                }
                for (dir, files) in &by_dir {
                    nodes.push(serde_json::json!({
                        "id": format!("{}:{}", s.name, dir.replace('/', "_")),
                        "kind": "commit", "repo": s.name, "files": files,
                        "depends_on": [format!("{}:precheck", s.name)]
                    }));
                }
            }
            _ => {
                nodes.push(serde_json::json!({
                    "id": format!("{}:commit", s.name),
                    "kind": "commit", "repo": s.name, "files": diff,
                    "depends_on": [format!("{}:precheck", s.name)]
                }));
            }
        }
    }

    if mode == "full" || mode == "sync" {
        let commit_ids: Vec<String> = nodes
            .iter()
            .filter(|n| n["kind"] == "commit")
            .filter_map(|n| n["id"].as_str().map(|s| s.to_string()))
            .collect();

        if !commit_ids.is_empty() {
            if let Some(root) = cfg
                .repos
                .iter()
                .find(|r| r.name.contains("root") || r.name == "phenix")
            {
                nodes.push(serde_json::json!({
                    "id": format!("{}:update-pins", root.name),
                    "kind": "update-pins", "repo": root.name,
                    "files": ["flake.lock"],
                    "depends_on": commit_ids
                }));
            }
        }
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "nodes": nodes, "total": nodes.len(), "mode": mode
            }))
            .unwrap()
        );
    } else {
        let title = match mode {
            "sync" => "Sync DAG:",
            "full" => "Full DAG:",
            _ => "Commit DAG:",
        };
        println!("{}", title);
        println!();
        for (i, n) in nodes.iter().enumerate() {
            let kind = n["kind"].as_str().unwrap_or("?");
            let id = n["id"].as_str().unwrap_or("?");
            println!("[{}] {}", i + 1, id);
            println!("    kind: {}", kind);
            if let Some(files) = n["files"].as_array() {
                if !files.is_empty() {
                    println!("    files:");
                    for f in files {
                        if let Some(p) = f.as_str() {
                            println!("      {}", p);
                        }
                    }
                }
            }
            if let Some(deps) = n["depends_on"].as_array() {
                if !deps.is_empty() {
                    println!("    depends_on:");
                    for d in deps {
                        if let Some(d) = d.as_str() {
                            println!("      {}", d);
                        }
                    }
                }
            }
            println!();
        }
        println!("Total: {} node(s)", nodes.len());
    }
    Ok(())
}

fn cmd_push(dry_run: bool, json_output: bool) -> Result<(), String> {
    let cfg = config::find_and_load()?;
    let dag = graph::discover_graph(&cfg)?;
    let order = dag.topological_order()?;

    let mut to_push = Vec::new();
    for node_id in &order {
        let node = match dag.get_node(node_id) {
            Some(n) => n,
            None => continue,
        };
        if !node.path.join(".git").exists() {
            continue;
        }
        let remote = git::git_remote(&node.path, "origin").ok();
        if remote.is_none() {
            continue;
        }
        let ahead = git::git_ahead_count(&node.path, &node.branch, "origin").unwrap_or(0);
        if ahead > 0 {
            to_push.push((node_id.clone(), node.name.clone(), ahead));
        }
    }

    if to_push.is_empty() {
        if json_output {
            println!(r#"{{"pushed": [], "message": "Nothing to push"}}"#);
        } else {
            println!("Nothing to push.");
        }
        return Ok(());
    }

    if json_output {
        let nodes: Vec<serde_json::Value> = to_push
            .iter()
            .map(|(id, name, ahead)| serde_json::json!({"name": name, "id": id, "ahead": ahead}))
            .collect();
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "push_order": order.iter().filter(|id| to_push.iter().any(|(tid, _, _)| tid == *id)).collect::<Vec<_>>(),
            "nodes": nodes
        })).unwrap());
        if dry_run {
            return Ok(());
        }
    } else if dry_run {
        println!("Would push (dependency order):");
        for (_, name, ahead) in &to_push {
            println!("  {} ({} ahead)", name, ahead);
        }
        return Ok(());
    } else {
        println!("Pushing (dependency order):");
    }

    let mut results: BTreeMap<String, Result<(), String>> = BTreeMap::new();
    for (node_id, name, _) in &to_push {
        let node = dag
            .get_node(node_id)
            .ok_or_else(|| format!("Node '{}' not found", node_id))?;
        if !json_output {
            print!("  {}... ", name);
        }
        let result = git::git_push(&node.path, &node.branch);
        if let Err(ref e) = result {
            if !json_output {
                println!("FAILED: {}", e);
            }
            results.insert(name.clone(), Err(e.clone()));
            return Err(format!("Push failed for '{}': {}", name, e));
        }
        if !json_output {
            println!("pushed");
        }
        results.insert(name.clone(), Ok(()));
    }

    if json_output {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "pushed": results.iter().map(|(name, r)| {
                serde_json::json!({"name": name, "success": r.is_ok(), "error": r.as_ref().err()})
            }).collect::<Vec<_>>()
        })).unwrap());
    }

    Ok(())
}

fn cmd_commit(
    dry_run: bool,
    json_output: bool,
    apply: bool,
    force: bool,
    resume_id: Option<&str>,
    messages_path: Option<&str>,
) -> Result<(), String> {
    let cfg = config::find_and_load()?;

    if let Some(tx_id) = resume_id {
        let result = sync::resume_sync(tx_id, &cfg, true)?;
        println!("{}", sync::format_result_output(&result, json_output));
        return Ok(());
    }

    let dag = graph::discover_graph(&cfg)?;
    let statuses = status::collect_all(&cfg)?;
    let mut plan = sync::plan_commit(&dag, &statuses, &cfg, false)?;
    plan.actions.retain(|a| matches!(a, Action::Commit { .. }));

    if dry_run {
        println!("{}", sync::format_plan_output(&plan, json_output));
        return Ok(());
    }

    if !plan.blocked_reasons.is_empty() && !force {
        println!("{}", sync::format_plan_output(&plan, json_output));
        return Err("Commit blocked. Use --force to override or fix the issues.".to_string());
    }

    if plan.actions.is_empty() {
        println!("Nothing to commit.");
        return Ok(());
    }

    if !apply {
        println!("{}", sync::format_plan_output(&plan, json_output));
        return Err(
            "Set --apply to execute the commit, or use --dry-run to preview."
                .to_string(),
        );
    }

    let messages: Option<BTreeMap<String, String>> = if let Some(path) = messages_path {
        let content = std::fs::read_to_string(path).map_err(|e| format!("Read messages: {}", e))?;
        let raw: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&content).map_err(|e| format!("Parse messages: {}", e))?;
        let mut msgs = BTreeMap::new();
        for (key, val) in raw {
            let msg = val
                .get("subject")
                .and_then(|v| v.as_str())
                .unwrap_or(&key)
                .to_string();
            msgs.insert(key, msg);
        }
        Some(msgs)
    } else {
        None
    };

    let result = sync::execute_sync(&plan, &dag, &cfg, true, messages.as_ref(), force)?;

    println!("{}", sync::format_result_output(&result, json_output));

    Ok(())
}

fn cmd_sync(
    mode: Option<&str>,
    apply: bool,
    dry_run: bool,
    json_output: bool,
    no_push: bool,
    _repos: &[String],
    _run_tend: bool,
) -> Result<(), String> {
    let _mode = mode.unwrap_or("push");
    let cfg = config::find_and_load()?;
    let dag = graph::discover_graph(&cfg)?;
    let statuses = status::collect_all(&cfg)?;
    let mut plan = sync::plan_sync(&dag, &statuses, &cfg)?;
    plan.actions.retain(|a| !matches!(a, Action::Commit { .. }));

    if dry_run {
        println!("{}", sync::format_plan_output(&plan, json_output));
        return Ok(());
    }

    if plan.actions.is_empty() {
        println!("Nothing to sync.");
        return Ok(());
    }

    if !apply {
        println!("{}", sync::format_plan_output(&plan, json_output));
        return Err(
            "Set --apply to execute the sync, or use --dry-run to preview."
                .to_string(),
        );
    }

    let result = sync::execute_sync(&plan, &dag, &cfg, no_push, None, false)?;
    println!("{}", sync::format_result_output(&result, json_output));
    Ok(())
}


