use std::path::PathBuf;

use clap::{Parser, Subcommand};

use tend::config;
use tend::discover;
use tend::execute;
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
        Commands::Plan { mode, group, target, base: _, json, files } => {
            cmd_plan(&root, configs.as_deref(), &mode, group.as_deref(), target.as_deref(), &files, json)
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
    phase: &str,
    mode: &str,
) -> Result<i32, String> {
    let discovered = discover::discover_configs(root, configs)
        .map_err(|e| format!("discovery failed: {e}"))?;
    let nodes = discover::resolve_nodes(root, discovered);

    let changed_files = if mode == "changed" {
        Some(get_changed_files(root).unwrap_or_default())
    } else {
        None
    };

    let changed_ref = changed_files.as_deref();

    let plan = planner::build_plan(&nodes, phase, mode, changed_ref).map_err(|e| match e {
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
    let mode_str = match mode {
        VerifyMode::Changed => "changed",
        VerifyMode::Full => "full",
        VerifyMode::Force => "force",
    };
    cmd_run(root, configs, "verify", mode_str)
}

fn cmd_fix(
    root: &PathBuf,
    configs: Option<&[PathBuf]>,
    mode: &FixMode,
) -> Result<i32, String> {
    let mode_str = match mode {
        FixMode::Changed => "changed",
        FixMode::All => "full",
    };
    cmd_run(root, configs, "fix", mode_str)
}

fn cmd_generate(
    root: &PathBuf,
    configs: Option<&[PathBuf]>,
    mode: &FixMode,
) -> Result<i32, String> {
    let mode_str = match mode {
        FixMode::Changed => "changed",
        FixMode::All => "full",
    };
    cmd_run(root, configs, "generate", mode_str)
}

fn cmd_gate(root: &PathBuf, configs: Option<&[PathBuf]>) -> Result<i32, String> {
    cmd_run(root, configs, "verify", "changed")
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
    group: Option<&str>,
    target: Option<&str>,
    files: &[String],
    json: bool,
) -> Result<i32, String> {
    let discovered = discover::discover_configs(root, configs)
        .map_err(|e| format!("discovery failed: {e}"))?;
    let nodes = discover::resolve_nodes(root, discovered);

    let changed_files = if mode == "changed" || mode == "staged" {
        Some(get_changed_files(root).unwrap_or_default())
    } else if !files.is_empty() {
        Some(files.to_vec())
    } else {
        None
    };
    let changed_ref = changed_files.as_deref();

    if json {
        let mut checks: Vec<serde_json::Value> = Vec::new();
        for node in &nodes {
            for task in &node.tasks {
                if let Some(g) = group { if node.id != *g { continue; } }
                if let Some(t) = target { if task.config.id != *t { continue; } }

                let matched_files: Vec<String> = match (mode, changed_ref) {
                    ("changed" | "staged", Some(cf)) => {
                        if let Some(ref when) = task.config.when {
                            if let Some(ref changed) = when.changed {
                                cf.iter().filter(|f| {
                                    planner::task_matches_paths(&changed.paths, &[(*f).clone()])
                                }).cloned().collect()
                            } else { vec![] }
                        } else { vec![] }
                    },
                    _ => vec![],
                };

                let should_run = match mode {
                    "all" | "force" => true,
                    _ => {
                        if let Some(ref when) = task.config.when {
                            if let Some(ref changed) = when.changed {
                                if let Some(cf) = changed_ref {
                                    planner::task_matches_paths(&changed.paths, cf)
                                } else { true }
                            } else { true }
                        } else { true }
                    }
                };

                if !should_run { continue; }

                let reason = if !matched_files.is_empty() {
                    format!("matched {} pattern(s)", matched_files.len())
                } else if mode == "all" {
                    "selected by --mode all".to_string()
                } else {
                    "selected explicitly".to_string()
                };

                checks.push(serde_json::json!({
                    "id": task.config.id,
                    "group": node.id,
                    "kind": task.config.kind,
                    "phase": task.config.phase,
                    "reason": reason,
                    "files": matched_files,
                    "depends_on": []
                }));
            }
        }
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "checks": checks,
            "total": checks.len()
        })).unwrap());
    } else {
        println!("Checks that would run (mode: {}):", mode);
        println!();
        let mut idx = 0u32;
        for node in &nodes {
            for task in &node.tasks {
                if let Some(g) = group { if node.id != *g { continue; } }
                if let Some(t) = target { if task.config.id != *t { continue; } }

                let matched_files: Vec<String> = match (mode, changed_ref) {
                    ("changed" | "staged", Some(cf)) => {
                        if let Some(ref when) = task.config.when {
                            if let Some(ref changed) = when.changed {
                                cf.iter().filter(|f| {
                                    planner::task_matches_paths(&changed.paths, &[(*f).clone()])
                                }).cloned().collect()
                            } else { vec![] }
                        } else { vec![] }
                    },
                    _ => vec![],
                };

                let should_run = match mode {
                    "all" | "force" => true,
                    _ => {
                        if let Some(ref when) = task.config.when {
                            if let Some(ref changed) = when.changed {
                                if let Some(cf) = changed_ref {
                                    planner::task_matches_paths(&changed.paths, cf)
                                } else { true }
                            } else { true }
                        } else { true }
                    }
                };

                if !should_run { continue; }

                idx += 1;
                let reason = if !matched_files.is_empty() {
                    format!("matched **/{} pattern(s)", matched_files.len())
                } else {
                    "selected explicitly".to_string()
                };

                println!("{}. {}", idx, task.config.id);
                println!("   reason: {}", reason);
                if !matched_files.is_empty() {
                    println!("   files:");
                    for f in &matched_files {
                        println!("     {}", f);
                    }
                }
                println!();
            }
        }
        if idx == 0 {
            println!("  (no checks match)");
        } else {
            println!("Total: {} check(s) would run", idx);
        }
    }
    Ok(0)
}
