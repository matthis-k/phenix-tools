use std::path::PathBuf;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};

mod gate;

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
    /// Run gate checks
    Gate {
        #[command(subcommand)]
        command: gate::GateCommands,
        /// Explicit path to a .phenix-checks.json file
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            generate(shell, &mut cmd, "phenix-tools", &mut std::io::stdout());
        }
        Commands::Gate { command, config } => {
            let workspace_root = &std::env::current_dir().unwrap_or_default();
            if let Err(e) = gate::dispatch(command, config, workspace_root) {
                eprintln!("error: {}", e);
                std::process::exit(1);
            }
        }
    }
}
