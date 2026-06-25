use std::path::PathBuf;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};

mod graph;
mod node;
mod sync;

#[derive(Parser)]
#[command(name = "phenix-tools", bin_name = "phenix-tools")]
#[command(about = "Phenix cross-repo developer and maintenance tooling")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        shell: Shell,
    },
    /// Sync repos in dependency order
    Sync {
        /// Path to JSON nodes file listing entry point repos
        nodes: PathBuf,
        /// Custom commit message (default: "update/sync: <input>")
        #[arg(short, long)]
        message: Option<String>,
        /// Dry run: print the plan without executing
        #[arg(long)]
        plan: bool,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            generate(shell, &mut cmd, "phenix-tools", &mut std::io::stdout());
        }
        Commands::Sync { nodes, message, plan } => {
            if let Err(e) = run_sync(&nodes, message, plan) {
                eprintln!("error: {}", e);
                std::process::exit(1);
            }
        }
    }
}

fn run_sync(nodes_arg: &PathBuf, message: Option<String>, plan: bool) -> Result<(), String> {
    let nodes_path = if nodes_arg.is_absolute() {
        nodes_arg.clone()
    } else {
        let cwd = std::env::current_dir().map_err(|e| format!("Cannot get cwd: {}", e))?;
        cwd.join(nodes_arg)
    };

    let base_dir = nodes_path
        .parent()
        .ok_or_else(|| "Cannot determine base directory from nodes file path".to_string())?;

    if !nodes_path.exists() {
        return Err(format!("Nodes file not found: {}", nodes_path.display()));
    }

    let entry_points = sync::SyncManager::read_nodes_file(&nodes_path)?;
    if entry_points.is_empty() {
        return Err("Nodes file contains no entries".to_string());
    }

    let resolved: Vec<String> = entry_points
        .iter()
        .map(|ep| {
            let p = PathBuf::from(ep);
            if p.is_absolute() {
                ep.clone()
            } else {
                base_dir.join(ep).to_string_lossy().to_string()
            }
        })
        .collect();

    println!("Entry points:");
    for ep in &resolved {
        println!("  {}", ep);
    }

    let manager = sync::SyncManager::new(base_dir, base_dir, message);
    let dag = manager.build_dag(&resolved)?;

    println!("\nDependency graph:");
    println!("{}", dag.dot_format());

    let topo = dag.topological_sort()?;
    println!("Update order:");
    for (i, node) in topo.order.iter().enumerate() {
        println!("  {}. {}", i + 1, node);
    }

    if plan {
        println!("\nPlan mode: no changes made.");
        return Ok(());
    }

    println!("\nExecuting sync...");
    manager.run_sync(&topo.order)?;
    println!("\nSync complete.");
    Ok(())
}
