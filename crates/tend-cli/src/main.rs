#![allow(clippy::ptr_arg, clippy::too_many_arguments)]

use std::path::PathBuf;
use std::str::FromStr;

use clap::{Parser, Subcommand};

use tend::config;
use tend::discover;
use tend::execute;
use tend::model::{Phase, PlanRequest, RunMode};
use tend::planner;
use tend::report;

#[derive(Parser)]
#[command(
    name = "tend",
    version,
    about = "Low-level composable task/verification/hook harness for Phenix"
)]
struct Cli {
    #[arg(global = true, long, help = "Set discovery root directory")]
    root: Option<PathBuf>,

    #[arg(global = true, long, short, help = "Explicit config file path(s)")]
    config: Vec<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Display the composed task tree
    Tree,
    /// List all discovered tasks
    List,
    /// Show config health and known checks (read-only)
    Status {
        #[arg(long, help = "Output as JSON")]
        json: bool,
    },
    /// Show which checks would run and why (read-only check plan)
    Plan {
        #[arg(long, default_value = "changed")]
        mode: String,
        #[arg(long, default_value = "verify")]
        phase: String,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        group: Option<String>,
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        base: Option<String>,
        #[arg(long)]
        json: bool,
        files: Vec<String>,
    },
    /// Run tasks with explicit phase and mode (agent-friendly)
    Run {
        #[arg(long, default_value = "verify")]
        phase: String,
        #[arg(long, default_value = "changed")]
        mode: String,
        #[arg(long)]
        profile: Option<String>,
    },
    /// Run non-mutating verification tasks
    Verify {
        #[command(subcommand)]
        mode: VerifyMode,
    },
    /// Run mutating fix tasks
    Fix {
        #[command(subcommand)]
        mode: FixMode,
    },
    /// Run mutating generate tasks
    Generate {
        #[command(subcommand)]
        mode: FixMode,
    },
    /// Run the default non-mutating gate preset
    Gate,
    /// Explain check failures: run verification and describe failures
    Explain,
    /// High-level check command with profile-based selection
    Check {
        #[arg(long, help = "Profile name (git-hook, pre-push, nix-check, manual, fix, stitch-sync)")]
        profile: String,
        #[arg(long, help = "Only check staged changes")]
        staged: bool,
        #[arg(long, help = "Offline mode (no network access)")]
        offline: bool,
        #[arg(long, help = "Locked mode (no dependency updates)")]
        locked: bool,
        #[arg(long, help = "Only check affected DAG nodes")]
        affected_dag: bool,
    },
    /// Validate configuration and profile assignments
    Validate {
        #[arg(long, help = "Validate profile safety rules")]
        profiles: bool,
    },
    /// Manage preflight tokens for hook skips
    Preflight {
        #[command(subcommand)]
        command: PreflightCommand,
    },
}

#[derive(Subcommand)]
enum PreflightCommand {
    /// Create a preflight token
    Create {
        #[arg(long, help = "Profile name")]
        profile: String,
        #[arg(long, help = "Only staged changes")]
        staged: bool,
    },
    /// Validate a preflight token
    Validate {
        #[arg(long, help = "Profile name")]
        profile: String,
        #[arg(long, help = "Token to validate")]
        token: String,
    },
}

#[derive(Subcommand, Clone, Debug)]
enum VerifyMode {
    Changed,
    Full,
    Force,
}

#[derive(Subcommand, Clone, Debug)]
enum FixMode {
    Changed,
    All,
}

fn main() {
    let cli = Cli::parse();

    let root = cli
        .root
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let configs = if cli.config.is_empty() {
        None
    } else {
        Some(cli.config)
    };

    let exit_code = match cli.command {
        Commands::Tree => cmd_tree(&root, configs.as_deref()),
        Commands::List => cmd_list(&root, configs.as_deref()),
        Commands::Status { json } => cmd_status(&root, configs.as_deref(), json),
        Commands::Plan {
            mode,
            phase,
            profile,
            group,
            target,
            base: _,
            json,
            files,
        } => cmd_plan(
            &root,
            configs.as_deref(),
            &mode,
            &phase,
            profile.as_deref(),
            group.as_deref(),
            target.as_deref(),
            &files,
            json,
        ),
        Commands::Run { phase, mode, profile } => match Phase::from_str(&phase) {
            Ok(p) => match RunMode::from_str(&mode) {
                Ok(m) => cmd_run(&root, configs.as_deref(), p, m, profile.as_deref()),
                Err(e) => Err(e),
            },
            Err(e) => Err(e),
        },
        Commands::Verify { mode } => cmd_verify(&root, configs.as_deref(), &mode),
        Commands::Fix { mode } => cmd_fix(&root, configs.as_deref(), &mode),
        Commands::Generate { mode } => cmd_generate(&root, configs.as_deref(), &mode),
        Commands::Gate => cmd_gate(&root, configs.as_deref()),
        Commands::Explain => cmd_explain(&root, configs.as_deref()),
        Commands::Check {
            profile,
            staged,
            offline,
            locked,
            affected_dag,
        } => cmd_check(
            &root,
            configs.as_deref(),
            &profile,
            staged,
            offline,
            locked,
            affected_dag,
        ),
        Commands::Validate { profiles } => {
            if profiles {
                cmd_validate_profiles(&root, configs.as_deref())
            } else {
                cmd_status(&root, configs.as_deref(), false)
            }
        }
        Commands::Preflight { command } => cmd_preflight(&root, configs.as_deref(), command),
    };

    match exit_code {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    }
}

fn get_changed_files(root: &std::path::Path) -> Result<Vec<String>, String> {
    let mut all = Vec::new();

    let output = std::process::Command::new("git")
        .args(["diff", "--name-only"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("git diff: {e}"))?;
    if output.status.success() {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let t = line.trim();
            if !t.is_empty() {
                all.push(t.to_string());
            }
        }
    }

    let output = std::process::Command::new("git")
        .args(["diff", "--cached", "--name-only"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("git diff --cached: {e}"))?;
    if output.status.success() {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let t = line.trim();
            if !t.is_empty() {
                all.push(t.to_string());
            }
        }
    }

    all.sort();
    all.dedup();
    Ok(all)
}

fn get_staged_files(root: &std::path::Path) -> Result<Vec<String>, String> {
    let output = std::process::Command::new("git")
        .args(["diff", "--cached", "--name-only"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("git diff --cached: {e}"))?;

    let mut files = Vec::new();
    if output.status.success() {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let t = line.trim();
            if !t.is_empty() {
                files.push(t.to_string());
            }
        }
    }

    files.sort();
    files.dedup();
    Ok(files)
}

fn cmd_check(
    root: &PathBuf,
    configs: Option<&[PathBuf]>,
    profile: &str,
    staged: bool,
    _offline: bool,
    _locked: bool,
    _affected_dag: bool,
) -> Result<i32, String> {
    // Determine phase and mode from profile
    let (phase, mode) = if profile == "fix" {
        (Phase::Fix, RunMode::Full)
    } else if staged {
        (Phase::Verify, RunMode::Staged)
    } else {
        (Phase::Verify, RunMode::Changed)
    };

    let discovered =
        discover::discover_configs(root, configs).map_err(|e| format!("discovery failed: {e}"))?;
    let nodes = discover::resolve_nodes(root, discovered);

    let files = match mode {
        RunMode::Staged => get_staged_files(root).unwrap_or_default(),
        RunMode::Changed => get_changed_files(root).unwrap_or_default(),
        _ => Vec::new(),
    };

    let req = PlanRequest {
        phase,
        mode,
        profile: Some(profile.to_string()),
        group: None,
        target: None,
        files,
        offline: false,
        locked: false,
    };

    let plan = planner::build_plan(&nodes, &req).map_err(|e| match e {
        planner::PlanError::MutatingRefused(id) => {
            format!("mutating task '{id}' refused in non-mutating command")
        }
    })?;

    if plan.items.is_empty() {
        println!("No tasks to run for profile '{profile}'.");
        return Ok(0);
    }

    println!("Running profile '{profile}' ({} tasks):", plan.items.len());
    println!();

    let result = execute::execute_plan(&plan.items, root);
    let (failed, _passed, _skipped) = report::print_results(&result, false);

    if failed > 0 {
        Ok(1)
    } else {
        Ok(0)
    }
}

fn cmd_validate_profiles(root: &PathBuf, configs: Option<&[PathBuf]>) -> Result<i32, String> {
    let discovered =
        discover::discover_configs(root, configs).map_err(|e| format!("discovery failed: {e}"))?;
    let nodes = discover::resolve_nodes(root, discovered);

    match tend::profiles::validate_profiles(&nodes) {
        Ok(()) => {
            println!("Profile validation: OK");
            Ok(0)
        }
        Err(violations) => {
            eprintln!("Profile validation FAILED:");
            for v in &violations {
                eprintln!("  {v}");
            }
            Err(format!("{} profile violation(s) found", violations.len()))
        }
    }
}

fn cmd_preflight(
    root: &PathBuf,
    configs: Option<&[PathBuf]>,
    command: PreflightCommand,
) -> Result<i32, String> {
    match command {
        PreflightCommand::Create { profile, staged } => {
            // Create a preflight token
            let discovered = discover::discover_configs(root, configs)
                .map_err(|e| format!("discovery failed: {e}"))?;
            let nodes = discover::resolve_nodes(root, discovered);

            let files = if staged {
                get_staged_files(root).unwrap_or_default()
            } else {
                get_changed_files(root).unwrap_or_default()
            };

            let req = PlanRequest {
                phase: Phase::Verify,
                mode: if staged { RunMode::Staged } else { RunMode::Changed },
                profile: Some(profile.clone()),
                group: None,
                target: None,
                files,
                offline: false,
                locked: false,
            };

            let plan = planner::build_plan(&nodes, &req).map_err(|e| format!("{e}"))?;
            let task_ids: Vec<String> = plan.items.iter().map(|i| i.task_id.clone()).collect();

            // Build a preflight token
            let tree_hash = get_git_tree_hash(root).unwrap_or_else(|| "unknown".to_string());
            let token = PreflightToken {
                version: 1,
                profile: profile.clone(),
                tree_hash,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                tasks: task_ids,
            };

            let token_json =
                serde_json::to_string(&token).map_err(|e| format!("serialize token: {e}"))?;
            let token_b64 = base64_encode(&token_json);

            println!("{token_b64}");
            Ok(0)
        }
        PreflightCommand::Validate { profile, token } => {
            let token_json = match base64_decode(&token) {
                Some(s) => s,
                None => return Err("invalid preflight token encoding".to_string()),
            };

            let stored: PreflightToken = serde_json::from_str(&token_json)
                .map_err(|e| format!("invalid preflight token: {e}"))?;

            let tree_hash = get_git_tree_hash(root).unwrap_or_else(|| "unknown".to_string());

            if stored.version != 1 {
                return Err(format!("unsupported preflight token version: {}", stored.version));
            }

            if stored.profile != profile {
                return Err(format!(
                    "preflight token profile '{}' does not match requested profile '{profile}'",
                    stored.profile
                ));
            }

            if stored.tree_hash != tree_hash {
                return Err(format!(
                    "preflight token tree hash '{}' does not match current '{}'",
                    stored.tree_hash, tree_hash
                ));
            }

            // Token age check: reject tokens older than 5 minutes
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            if now > stored.timestamp && now - stored.timestamp > 300 {
                return Err("preflight token has expired (max 5 minutes)".to_string());
            }

            println!("Preflight token valid for profile '{profile}'");
            println!("  tasks: {}", stored.tasks.len());
            Ok(0)
        }
    }
}

fn get_git_tree_hash(root: &std::path::Path) -> Option<String> {
    std::process::Command::new("git")
        .args(["write-tree"])
        .current_dir(root)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok().map(|s| s.trim().to_string())
            } else {
                None
            }
        })
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PreflightToken {
    version: u32,
    profile: String,
    tree_hash: String,
    timestamp: u64,
    tasks: Vec<String>,
}

fn base64_encode(input: &str) -> String {
    // Simple base64 encoding without external dependency
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = input.as_bytes();
    let mut result = String::new();
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

fn base64_decode(input: &str) -> Option<String> {
    // Simple base64 decoding
    const DECODE: [i8; 128] = {
        let mut table = [-1i8; 128];
        let chars = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut i = 0;
        while i < 64 {
            table[chars[i] as usize] = i as i8;
            i += 1;
        }
        table
    };

    let cleaned: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = cleaned.as_bytes();
    if bytes.len() % 4 != 0 {
        return None;
    }

    let mut result = Vec::new();
    for chunk in bytes.chunks(4) {
        if chunk.len() < 4 {
            return None;
        }
        let vals: Vec<u8> = chunk
            .iter()
            .take_while(|&&b| b != b'=')
            .filter_map(|&b| {
                if (b as usize) < 128 {
                    let v = DECODE[b as usize];
                    if v >= 0 { Some(v as u8) } else { None }
                } else {
                    None
                }
            })
            .collect();

        if vals.is_empty() {
            break;
        }

        let mut triple: u32 = 0;
        for (i, &v) in vals.iter().enumerate() {
            triple |= (v as u32) << (18 - i * 6);
        }

        result.push((triple >> 16) as u8);
        if vals.len() > 2 {
            result.push((triple >> 8) as u8);
        }
        if vals.len() > 3 {
            result.push(triple as u8);
        }
    }

    String::from_utf8(result).ok()
}

fn cmd_tree(root: &PathBuf, configs: Option<&[PathBuf]>) -> Result<i32, String> {
    let discovered =
        discover::discover_configs(root, configs).map_err(|e| format!("discovery failed: {e}"))?;
    let nodes = discover::resolve_nodes(root, discovered);

    println!("Task tree (root: {})", root.display());
    for node in &nodes {
        let path_str = if node.node_path.to_string_lossy() == "." {
            String::from(".")
        } else {
            node.node_path.to_string_lossy().to_string()
        };
        println!("  {}", path_str);
        println!("    id: {}", node.id);
        if !node.description.is_empty() {
            println!("    description: {}", node.description);
        }
        if !node.tags.is_empty() {
            println!("    tags: {}", node.tags.join(", "));
        }
        println!("    tasks: {}", node.tasks.len());
        for task in &node.tasks {
            let desc = task.config.description.as_deref().unwrap_or("");
            let profiles = task.config.profiles.as_ref()
                .map(|p| format!(" profiles=[{}]", p.join(",")))
                .unwrap_or_default();
            println!(
                "      {}  [{}]{}  {}",
                task.config.id, task.config.phase, profiles, desc
            );
        }
        println!();
    }

    Ok(0)
}

fn cmd_list(root: &PathBuf, configs: Option<&[PathBuf]>) -> Result<i32, String> {
    let discovered =
        discover::discover_configs(root, configs).map_err(|e| format!("discovery failed: {e}"))?;
    let nodes = discover::resolve_nodes(root, discovered);

    for node in &nodes {
        for task in &node.tasks {
            let path_str = if node.node_path.to_string_lossy() == "." {
                String::from(".")
            } else {
                node.node_path.to_string_lossy().to_string()
            };
            let desc = task.config.description.as_deref().unwrap_or("");
            let mutates = task
                .config
                .mutates
                .unwrap_or_else(|| config::default_mutates(&task.config.phase));
            let profiles = task.config.profiles.as_ref()
                .map(|p| format!(" [{}]", p.join(",")))
                .unwrap_or_default();
            println!(
                "{}  {}  [{}]  {}{}  {}",
                task.config.id,
                path_str,
                task.config.phase,
                if mutates { "mut" } else { "ro" },
                profiles,
                desc
            );
        }
    }

    Ok(0)
}

fn cmd_run(
    root: &PathBuf,
    configs: Option<&[PathBuf]>,
    phase: Phase,
    mode: RunMode,
    profile: Option<&str>,
) -> Result<i32, String> {
    let discovered =
        discover::discover_configs(root, configs).map_err(|e| format!("discovery failed: {e}"))?;
    let nodes = discover::resolve_nodes(root, discovered);

    let files = if mode == RunMode::Changed {
        get_changed_files(root).unwrap_or_default()
    } else {
        Vec::new()
    };

    let req = PlanRequest {
        phase,
        mode,
        profile: profile.map(|s| s.to_string()),
        group: None,
        target: None,
        files,
        offline: false,
        locked: false,
    };

    let plan = planner::build_plan(&nodes, &req).map_err(|e| match e {
        planner::PlanError::MutatingRefused(id) => {
            format!("mutating task '{id}' refused in non-mutating command")
        }
    })?;

    if plan.items.is_empty() {
        println!("No tasks to run.");
        return Ok(0);
    }

    let result = execute::execute_plan(&plan.items, root);
    let (failed, _passed, _skipped) = report::print_results(&result, false);

    if failed > 0 {
        Ok(1)
    } else {
        Ok(0)
    }
}

fn cmd_verify(
    root: &PathBuf,
    configs: Option<&[PathBuf]>,
    mode: &VerifyMode,
) -> Result<i32, String> {
    let run_mode = match mode {
        VerifyMode::Changed => RunMode::Changed,
        VerifyMode::Full => RunMode::Full,
        VerifyMode::Force => RunMode::Force,
    };
    cmd_run(root, configs, Phase::Verify, run_mode, None)
}

fn cmd_fix(root: &PathBuf, configs: Option<&[PathBuf]>, mode: &FixMode) -> Result<i32, String> {
    let run_mode = match mode {
        FixMode::Changed => RunMode::Changed,
        FixMode::All => RunMode::Full,
    };
    cmd_run(root, configs, Phase::Fix, run_mode, None)
}

fn cmd_generate(
    root: &PathBuf,
    configs: Option<&[PathBuf]>,
    mode: &FixMode,
) -> Result<i32, String> {
    let run_mode = match mode {
        FixMode::Changed => RunMode::Changed,
        FixMode::All => RunMode::Full,
    };
    cmd_run(root, configs, Phase::Generate, run_mode, None)
}

fn cmd_gate(root: &PathBuf, configs: Option<&[PathBuf]>) -> Result<i32, String> {
    cmd_run(root, configs, Phase::Verify, RunMode::Changed, None)
}

fn cmd_explain(root: &PathBuf, configs: Option<&[PathBuf]>) -> Result<i32, String> {
    let discovered =
        discover::discover_configs(root, configs).map_err(|e| format!("discovery failed: {e}"))?;
    let nodes = discover::resolve_nodes(root, discovered);

    println!("tend explain");
    println!();
    println!("Persisted run reports are not implemented yet.");
    println!("This command currently explains the configured checks and gives stable reproduction commands.");
    println!();

    let mut total = 0usize;

    for node in &nodes {
        for task in &node.tasks {
            total += 1;

            let desc = task.config.description.as_deref().unwrap_or("");
            println!("check: {}", task.config.id);
            println!("  node: {}", node.id);
            println!("  phase: {}", task.config.phase);
            if !desc.is_empty() {
                println!("  description: {}", desc);
            }
            let profiles = task.config.profiles.as_ref()
                .map(|p| p.join(", "))
                .unwrap_or_else(|| "manual (default)".to_string());
            println!("  profiles: {}", profiles);
            println!(
                "  reproduce: tend run --phase {} --mode full",
                task.config.phase
            );
            println!();
        }
    }

    if total == 0 {
        println!("No tasks discovered.");
        return Ok(1);
    }

    println!("Total: {total} task(s)");
    Ok(0)
}

fn cmd_status(root: &PathBuf, configs: Option<&[PathBuf]>, json: bool) -> Result<i32, String> {
    let discovered =
        discover::discover_configs(root, configs).map_err(|e| format!("discovery failed: {e}"))?;
    let nodes = discover::resolve_nodes(root, discovered);

    if json {
        let entries: Vec<serde_json::Value> = nodes
            .iter()
            .map(|n| {
                serde_json::json!({
                    "node_path": n.node_path.to_string_lossy(),
                    "id": n.id,
                    "description": n.description,
                    "tags": n.tags,
                    "tasks": n.tasks.len()
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "configs": entries, "total": entries.len()
            }))
            .unwrap()
        );
    } else {
        println!("Tend workspace status (root: {})", root.display());
        for node in &nodes {
            println!("  {}  ({})", node.id, node.node_path.to_string_lossy());
            println!("    tasks: {}", node.tasks.len());
            if !node.tags.is_empty() {
                println!("    tags: {}", node.tags.join(", "));
            }
            println!();
        }
        println!(
            "Total: {} configs, {} tasks",
            nodes.len(),
            nodes.iter().map(|n| n.tasks.len()).sum::<usize>()
        );
    }
    Ok(0)
}

fn cmd_plan(
    root: &PathBuf,
    configs: Option<&[PathBuf]>,
    mode: &str,
    phase: &str,
    profile: Option<&str>,
    group: Option<&str>,
    target: Option<&str>,
    files: &[String],
    json: bool,
) -> Result<i32, String> {
    let run_mode = RunMode::from_str(mode).unwrap_or(RunMode::Changed);
    let phase = Phase::from_str(phase).unwrap_or(Phase::Verify);

    let discovered =
        discover::discover_configs(root, configs).map_err(|e| format!("discovery failed: {e}"))?;
    let nodes = discover::resolve_nodes(root, discovered);

    let plan_files = match run_mode {
        RunMode::Changed | RunMode::Staged => {
            let mut all = get_changed_files(root).unwrap_or_default();
            if !files.is_empty() {
                all.extend(files.iter().cloned());
            }
            all
        }
        _ => files.to_vec(),
    };

    let req = PlanRequest {
        phase,
        mode: run_mode,
        profile: profile.map(|s| s.to_string()),
        group: group.map(|s| s.to_string()),
        target: target.map(|s| s.to_string()),
        files: plan_files,
        offline: false,
        locked: false,
    };

    let plan = planner::build_plan(&nodes, &req).map_err(|e| format!("{e}"))?;

    if json {
        let profile_name = profile.unwrap_or("none");
        let items: Vec<serde_json::Value> = plan
            .items
            .iter()
            .map(|item| {
                let mut cmd = Vec::new();
                if let tend::model::TaskKind::Command { command, .. } = &item.step.kind {
                    cmd = command.clone();
                }
                serde_json::json!({
                    "id": item.task_id,
                    "command": cmd,
                    "tags": [],
                    "mutates": false,
                    "interactive": false,
                    "sandbox_safe": true
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "profile": profile_name,
                "tasks": items
            }))
            .unwrap()
        );
    } else {
        let profile_str = profile.map(|p| format!(" profile='{p}'")).unwrap_or_default();
        println!(
            "Checks that would run (mode: {}, phase: {},{}):",
            run_mode, phase, profile_str
        );
        println!();
        if plan.items.is_empty() {
            println!("  (no checks match)");
        } else {
            for (i, item) in plan.items.iter().enumerate() {
                println!(
                    "{}. {} [{}]",
                    i + 1,
                    item.task_id,
                    item.step.kind.description()
                );
                if !item.description.is_empty() {
                    println!("   description: {}", item.description);
                }
                println!("   reason: {}", item.reason);
                if !item.matched_files.is_empty() {
                    println!("   files:");
                    for f in &item.matched_files {
                        println!("     {}", f);
                    }
                }
                println!();
            }
            println!("Total: {} check(s) would run", plan.items.len());
        }
    }
    Ok(0)
}
