use std::path::PathBuf;

use clap::{Parser, Subcommand};

mod checks;
mod config;
mod discover;
mod execute;
mod model;
mod planner;
mod report;

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
    /// Run only tasks affected by changed files
    Changed,
    /// Run all tasks, respecting when.changed.paths
    Full,
    /// Run all tasks, ignoring when.changed.paths
    Force,
}

#[derive(Subcommand, Clone, Debug)]
enum FixMode {
    /// Run only tasks affected by changed files
    Changed,
    /// Run all applicable tasks
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
