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
#[command(name = "tend", version, about = "Low-level composable task/verification/hook harness for Phenix")]
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
        Commands::Plan { mode, phase, group, target, base: _, json, files } => {
            cmd_plan(&root, configs.as_deref(), &mode, &phase, group.as_deref(), target.as_deref(), &files, json)
        },
        Commands::Run { phase, mode } => {
            match Phase::from_str(&phase) {
                Ok(p) => match RunMode::from_str(&mode) {
                    Ok(m) => cmd_run(&root, configs.as_deref(), p, m),
                    Err(e) => Err(e),
                },
                Err(e) => Err(e),
            }
        },
        Commands::Verify { mode } => cmd_verify(&root, configs.as_deref(), &mode),
        Commands::Fix { mode } => cmd_fix(&root, configs.as_deref(), &mode),
        Commands::Generate { mode } => cmd_generate(&root, configs.as_deref(), &mode),
        Commands::Gate => cmd_gate(&root, configs.as_deref()),
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

fn cmd_tree(root: &PathBuf, configs: Option<&[PathBuf]>) -> Result<i32, String> {
    let discovered = discover::discover_configs(root, configs)
        .map_err(|e| format!("discovery failed: {e}"))?;
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
            let desc = task
                .config
                .description
                .as_deref()
                .unwrap_or("");
            println!("      {}  [{}]  {}", task.config.id, task.config.phase, desc);
        }
        println!();
    }

    Ok(0)
}

fn cmd_list(root: &PathBuf, configs: Option<&[PathBuf]>) -> Result<i32, String> {
    let discovered = discover::discover_configs(root, configs)
        .map_err(|e| format!("discovery failed: {e}"))?;
    let nodes = discover::resolve_nodes(root, discovered);

    for node in &nodes {
        for task in &node.tasks {
            let path_str = if node.node_path.to_string_lossy() == "." {
                String::from(".")
            } else {
                node.node_path.to_string_lossy().to_string()
            };
            let desc = task
                .config
                .description
                .as_deref()
                .unwrap_or("");
            let mutates = task
                .config
                .mutates
                .unwrap_or_else(|| config::default_mutates(&task.config.phase));
            println!(
                "{}  {}  [{}]  {}  {}",
                task.config.id,
                path_str,
                task.config.phase,
                if mutates { "mut" } else { "ro" },
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
) -> Result<i32, String> {
    let discovered = discover::discover_configs(root, configs)
        .map_err(|e| format!("discovery failed: {e}"))?;
    let nodes = discover::resolve_nodes(root, discovered);

    let files = if mode == RunMode::Changed {
        get_changed_files(root).unwrap_or_default()
    } else {
        Vec::new()
    };

    let req = PlanRequest { phase, mode, group: None, target: None, files };

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
    cmd_run(root, configs, Phase::Verify, run_mode)
}

fn cmd_fix(
    root: &PathBuf,
    configs: Option<&[PathBuf]>,
    mode: &FixMode,
) -> Result<i32, String> {
    let run_mode = match mode {
        FixMode::Changed => RunMode::Changed,
        FixMode::All => RunMode::Full,
    };
    cmd_run(root, configs, Phase::Fix, run_mode)
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
    cmd_run(root, configs, Phase::Generate, run_mode)
}

fn cmd_gate(root: &PathBuf, configs: Option<&[PathBuf]>) -> Result<i32, String> {
    cmd_run(root, configs, Phase::Verify, RunMode::Changed)
}

fn cmd_status(root: &PathBuf, configs: Option<&[PathBuf]>, json: bool) -> Result<i32, String> {
    let discovered = discover::discover_configs(root, configs)
        .map_err(|e| format!("discovery failed: {e}"))?;
    let nodes = discover::resolve_nodes(root, discovered);

    if json {
        let entries: Vec<serde_json::Value> = nodes.iter().map(|n| {
            serde_json::json!({
                "node_path": n.node_path.to_string_lossy(),
                "id": n.id,
                "description": n.description,
                "tags": n.tags,
                "tasks": n.tasks.len()
            })
        }).collect();
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "configs": entries, "total": entries.len()
        })).unwrap());
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
        println!("Total: {} configs, {} tasks",
            nodes.len(),
            nodes.iter().map(|n| n.tasks.len()).sum::<usize>());
    }
    Ok(0)
}

fn cmd_plan(
    root: &PathBuf,
    configs: Option<&[PathBuf]>,
    mode: &str,
    phase: &str,
    group: Option<&str>,
    target: Option<&str>,
    files: &[String],
    json: bool,
) -> Result<i32, String> {
    let run_mode = RunMode::from_str(mode).unwrap_or(RunMode::Changed);
    let phase = Phase::from_str(phase).unwrap_or(Phase::Verify);

    let discovered = discover::discover_configs(root, configs)
        .map_err(|e| format!("discovery failed: {e}"))?;
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
        group: group.map(|s| s.to_string()),
        target: target.map(|s| s.to_string()),
        files: plan_files,
    };

    let plan = planner::build_plan(&nodes, &req)
        .map_err(|e| format!("{e}"))?;

    if json {
        let checks: Vec<serde_json::Value> = plan.items.iter().map(|item| {
            serde_json::json!({
                "id": item.task_id,
                "group": item.chain_id.split('.').next().unwrap_or(&item.task_id),
                "kind": item.step.kind.description(),
                "phase": item.phase,
                "reason": item.reason.to_string(),
                "files": item.matched_files,
                "depends_on": []
            })
        }).collect();
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "checks": checks,
            "total": checks.len()
        })).unwrap());
    } else {
        println!("Checks that would run (mode: {}, phase: {}):", run_mode, phase);
        println!();
        if plan.items.is_empty() {
            println!("  (no checks match)");
        } else {
            for (i, item) in plan.items.iter().enumerate() {
                println!("{}. {} [{}]", i + 1, item.task_id, item.step.kind.description());
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
