use clap::{Parser, Subcommand};

mod changeset;
mod config;
mod git;
mod model;
mod status;
mod validate;

fn main() {
    let cli = Cli::parse();

    let result = match &cli.command {
        Commands::Repos => cmd_repos(),
        Commands::Status { json, short, dirty_only, repo } => cmd_status(*json, *short, *dirty_only, repo.as_deref()),
        Commands::Diff { repo, staged, json } => cmd_diff(repo.as_deref(), *staged, *json),
        Commands::Dag { mode, split, json } => cmd_dag(mode.as_deref(), split.as_deref(), *json),
        Commands::Commit { message, repo, messages, write_template, dry_run, no_tend, staged, apply } => {
            cmd_commit(message.as_deref(), repo.as_deref(), messages.as_deref(), *write_template, *dry_run, *no_tend, *staged, *apply)
        },
        Commands::Changeset { command } => changeset::dispatch(command),
    };

    if let Err(e) = result {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}

#[derive(Parser)]
#[command(name = "stitch", version, about = "Multi-repo Git coordinator for Phenix workspaces")]
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
        #[arg(long, default_value = "by-repo", help = "Split strategy: by-repo, by-path")]
        split: Option<String>,
        #[arg(long, help = "Output as JSON")]
        json: bool,
    },
    /// Create exact-file commits across repos
    Commit {
        #[arg(short, long, help = "Commit message (single repo only)")]
        message: Option<String>,
        #[arg(long, help = "Repo name")]
        repo: Option<String>,
        #[arg(long, help = "Path to JSON messages file")]
        messages: Option<String>,
        #[arg(long, help = "Write message template and exit")]
        write_template: bool,
        #[arg(long, help = "Dry run (no actual commits)")]
        dry_run: bool,
        #[arg(long, help = "Skip tend pre-check")]
        no_tend: bool,
        #[arg(long, help = "Only commit previously staged files (git add)")]
        staged: bool,
        #[arg(long, help = "Apply (required for actual commits)")]
        apply: bool,
    },
    /// Manage changesets (legacy)
    Changeset {
        #[command(subcommand)]
        command: changeset::ChangesetCommands,
    },
}

fn cmd_repos() -> Result<(), String> {
    let cfg = config::find_and_load()?;
    for repo in &cfg.repos {
        let exists = if repo.resolved_path(&cfg).exists() { "✓" } else { "✗" };
        println!("{}  {}  ({})", exists, repo.name, repo.path);
    }
    Ok(())
}

fn cmd_status(json: bool, short: bool, dirty_only: bool, repo_filter: Option<&str>) -> Result<(), String> {
    let cfg = config::find_and_load()?;
    let statuses = status::collect_all(&cfg)?;

    if json {
        let filtered: Vec<_> = statuses.iter()
            .filter(|s| repo_filter.map_or(true, |r| s.name == r))
            .filter(|s| !dirty_only || s.is_dirty)
            .collect();
        let output = serde_json::to_string_pretty(&filtered)
            .map_err(|e| format!("JSON: {}", e))?;
        println!("{}", output);
        return Ok(());
    }

    if short {
        for s in &statuses {
            if repo_filter.map_or(true, |r| s.name == r) && (!dirty_only || s.is_dirty) {
                let prefix = if s.is_dirty { "M" } else { " " };
                println!("{}  {}  {}", prefix, s.name, s.branch);
            }
        }
        return Ok(());
    }

    println!("Workspace: {}", cfg.workspace);
    println!();
    for s in &statuses {
        if repo_filter.map_or(true, |r| s.name == r) && (!dirty_only || s.is_dirty) {
            let dirty = if s.is_dirty { "yes" } else { "no" };
            println!("{}", s.name);
            println!("  branch: {}", s.branch);
            println!("  dirty: {}", dirty);
            println!("  staged: {}", s.staged_count);
            println!("  unstaged: {}", s.unstaged_count);
            println!("  untracked: {}", s.untracked_count);
            if s.is_dirty {
                let path = cfg.repos.iter().find(|r| r.name == s.name)
                    .map(|r| r.resolved_path(&cfg));
                if let Some(p) = path {
                    let diff = git::git_diff_names(&p).unwrap_or_default();
                    for f in &diff {
                        println!("    M {}", f);
                    }
                }
            }
            if let Some(ref ahead) = s.ahead { println!("  ahead: {}", ahead); }
            if let Some(ref behind) = s.behind { println!("  behind: {}", behind); }
            println!();
        }
    }
    Ok(())
}

fn cmd_diff(repo: Option<&str>, staged: bool, json: bool) -> Result<(), String> {
    let cfg = config::find_and_load()?;

    let target_repos: Vec<_> = if let Some(name) = repo {
        vec![cfg.repos.iter().find(|r| r.name == name)
            .ok_or_else(|| format!("Repo '{}' not found", name))?]
    } else {
        cfg.repos.iter().collect()
    };

    let mut all_diffs: Vec<serde_json::Value> = Vec::new();

    for r in &target_repos {
        let path = r.resolved_path(&cfg);
        if !path.join(".git").exists() { continue; }

        let mut args = vec!["diff"];
        if staged { args.push("--cached"); }

        let output = std::process::Command::new("git")
            .args(&args).current_dir(&path).output()
            .map_err(|e| format!("git diff: {}", e))?;

        let diff_text = String::from_utf8_lossy(&output.stdout).to_string();
        if diff_text.trim().is_empty() { continue; }

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
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "diffs": all_diffs
        })).unwrap());
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
        if !s.is_dirty { continue; }
        let repo_cfg = cfg.repos.iter().find(|r| r.name == s.name);
        let diff = repo_cfg.map(|r| {
            let p = r.resolved_path(&cfg);
            git::git_diff_names(&p).unwrap_or_default()
        }).unwrap_or_default();

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
                let mut by_dir: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
                for f in &diff {
                    let dir = f.rfind('/').map(|i| f[..i].to_string()).unwrap_or_else(|| "root".to_string());
                    by_dir.entry(dir).or_default().push(f.clone());
                }
                for (dir, files) in &by_dir {
                    nodes.push(serde_json::json!({
                        "id": format!("{}:{}", s.name, dir.replace('/', "_")),
                        "kind": "commit", "repo": s.name, "files": files,
                        "depends_on": [format!("{}:precheck", s.name)]
                    }));
                }
            },
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
        let commit_ids: Vec<String> = nodes.iter()
            .filter(|n| n["kind"] == "commit")
            .filter_map(|n| n["id"].as_str().map(|s| s.to_string()))
            .collect();

        if !commit_ids.is_empty() {
            if let Some(root) = cfg.repos.iter().find(|r| r.name.contains("root") || r.name == "phenix") {
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
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "nodes": nodes, "total": nodes.len(), "mode": mode
        })).unwrap());
    } else {
        let title = match mode {
            "sync" => "Sync DAG:",
            "full" => "Full DAG:",
            _ => "Commit DAG:"
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
                        if let Some(p) = f.as_str() { println!("      {}", p); }
                    }
                }
            }
            if let Some(deps) = n["depends_on"].as_array() {
                if !deps.is_empty() {
                    println!("    depends_on:");
                    for d in deps {
                        if let Some(d) = d.as_str() { println!("      {}", d); }
                    }
                }
            }
            println!();
        }
        println!("Total: {} node(s)", nodes.len());
    }
    Ok(())
}

fn cmd_commit(
    message: Option<&str>,
    repo: Option<&str>,
    messages_path: Option<&str>,
    write_template: bool,
    dry_run: bool,
    no_tend: bool,
    staged: bool,
    apply: bool,
) -> Result<(), String> {
    let get_diff = |path: &std::path::Path| -> Vec<String> {
        if staged {
            git::git_diff_cached_names(path).unwrap_or_default()
        } else {
            git::git_diff_names(path).unwrap_or_default()
        }
    };

    let cfg = config::find_and_load()?;
    let statuses = status::collect_all(&cfg)?;

    let dirty_repos: Vec<_> = statuses.iter().filter(|s| s.is_dirty).collect();

    if dirty_repos.is_empty() {
        println!("Nothing to commit.");
        return Ok(());
    }

    if write_template {
        let mut msgs = serde_json::Map::new();
        for s in &dirty_repos {
            let repo_cfg = cfg.repos.iter().find(|r| r.name == s.name);
            let diff = repo_cfg.map(|r| {
                let p = r.resolved_path(&cfg);
                get_diff(&p)
            }).unwrap_or_default();

            msgs.insert(format!("{}:commit", s.name), serde_json::json!({
                "subject": "", "body": "", "files": diff
            }));
        }
        let path = std::path::Path::new(".stitch").join("messages.json");
        std::fs::create_dir_all(".stitch").ok();
        let content = serde_json::to_string_pretty(&serde_json::json!(msgs)).unwrap();
        std::fs::write(&path, &content).map_err(|e| format!("Write: {}", e))?;
        println!("Message template written to {}", path.display());
        println!("Edit it, then run: stitch commit --messages {}", path.display());
        return Ok(());
    }

    if message.is_none() && messages_path.is_none() && dirty_repos.len() > 1 {
        println!("{} repos need commits but no message provided.", dirty_repos.len());
        println!();
        println!("To write a single message: stitch commit -m \"message\"");
        println!("To write per-repo messages: stitch commit --write-template");
        println!("To commit a single repo:    stitch commit --repo <name> -m \"message\"");
        return Ok(());
    }

    if !apply && !dry_run {
        return Err("Set --apply to commit, or --dry-run to preview".to_string());
    }

    let msgs: Option<serde_json::Map<String, serde_json::Value>> = if let Some(path) = messages_path {
        let content = std::fs::read_to_string(path).map_err(|e| format!("Read messages: {}", e))?;
        Some(serde_json::from_str(&content).map_err(|e| format!("Parse messages: {}", e))?)
    } else {
        None
    };

    if !no_tend && apply {
        let root = std::env::current_dir().unwrap_or_default();
        if let Ok(discovered) = tend::discover::discover_configs(&root, None) {
            let nodes = tend::discover::resolve_nodes(&root, discovered);
            if let Ok(plan) = tend::planner::build_plan(&nodes, "verify", "changed", None) {
                let results = tend::execute::execute_plan(&plan.items, &root);
                let failures: Vec<_> = results.iter().filter(|r| !r.passed && !r.skipped).collect();
                if !failures.is_empty() {
                    return Err(format!("Tend gate blocked: {} check(s) failed. Run `tend run` to see details.", failures.len()));
                }
            }
        }
    }

    for s in &dirty_repos {
        if let Some(r) = repo { if s.name != *r { continue; } }

        let repo_cfg = cfg.repos.iter().find(|r| r.name == s.name);
        let repo_cfg = match repo_cfg { Some(r) => r, None => continue };
        let repo_path = repo_cfg.resolved_path(&cfg);

        let diff = get_diff(&repo_path);
        let node_key = format!("{}:commit", s.name);

        let files_to_stage: Vec<String> = if let Some(ref msgs) = msgs {
            if let Some(node_msg) = msgs.get(&node_key) {
                node_msg.get("files").and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str()).map(|s| s.to_string()).collect())
                    .unwrap_or(diff.clone())
            } else { diff.clone() }
        } else { diff.clone() };

        let msg = if let Some(ref msgs) = msgs {
            if let Some(node_msg) = msgs.get(&node_key) {
                node_msg.get("subject").and_then(|v| v.as_str()).unwrap_or(message.unwrap_or("")).to_string()
            } else { message.unwrap_or(&format!("Apply changes to {}", s.name)).to_string() }
        } else { message.unwrap_or(&format!("Apply changes to {}", s.name)).to_string() };

        if dry_run {
            println!("[dry-run] {}: {}", s.name, msg);
            for f in &files_to_stage { println!("         {}", f); }
            continue;
        }

        git::git_add(&repo_path, &files_to_stage)?;
        let trailed = model::add_trailers(&msg, &"cli", &cfg.workspace);
        git::git_commit(&repo_path, &trailed)?;
        let hash = git::git_short_head(&repo_path).ok();
        println!("  committed {}: {}", s.name, hash.unwrap_or_default());
    }

    Ok(())
}
