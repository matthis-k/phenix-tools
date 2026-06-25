pub mod abort;
pub mod commit;
pub mod new;
pub mod plan;
pub mod push;
pub mod set_files;
pub mod set_message;
pub mod validate;

use clap::Subcommand;

use crate::model::{Changeset, WorkspaceConfig};

const PLAN_FILE: &str = ".stitch-plan.json";

#[derive(Subcommand)]
pub enum ChangesetCommands {
    /// Create a new changeset
    New {
        /// Title for the new changeset
        title: String,
    },
    /// Show current changeset status
    Status,
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

pub fn dispatch(command: &ChangesetCommands) -> Result<(), String> {
    match command {
        ChangesetCommands::New { title } => new::execute(title),
        ChangesetCommands::Status => status_cmd(),
        ChangesetCommands::Plan { write, json } => plan::execute(*write, *json),
        ChangesetCommands::SetMessage { repo, message } => set_message::execute(repo, message),
        ChangesetCommands::SetFiles { repo, files } => set_files::execute(repo, files),
        ChangesetCommands::Validate { json } => validate::execute(*json),
        ChangesetCommands::Commit => commit::execute(),
        ChangesetCommands::Push => push::execute(),
        ChangesetCommands::Abort => abort::execute(),
    }
}

fn status_cmd() -> Result<(), String> {
    let cs = load_current()?;
    match cs {
        Some(cs) => {
            let output = serde_json::to_string_pretty(&cs)
                .map_err(|e| format!("JSON: {}", e))?;
            println!("{}", output);
        }
        None => {
            println!("No active changeset.");
        }
    }
    Ok(())
}

// --- Storage ---

fn stitch_dir() -> std::path::PathBuf {
    let cwd = std::env::current_dir().unwrap_or_default();
    cwd.join(".stitch")
}

fn plan_path() -> std::path::PathBuf {
    let cwd = std::env::current_dir().unwrap_or_default();
    cwd.join(PLAN_FILE)
}

fn changesets_dir() -> std::path::PathBuf {
    stitch_dir().join("changesets")
}

fn changeset_path(id: &str) -> std::path::PathBuf {
    changesets_dir().join(format!("{}.json", id))
}

pub fn load_current() -> Result<Option<Changeset>, String> {
    let plan_p = plan_path();
    if plan_p.exists() {
        let content = std::fs::read_to_string(&plan_p)
            .map_err(|e| format!("Failed to read plan file: {}", e))?;
        let cs: Changeset = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse plan file: {}", e))?;
        return Ok(Some(cs));
    }

    let cs_dir = changesets_dir();
    if !cs_dir.exists() {
        return Ok(None);
    }

    let mut entries: Vec<_> = std::fs::read_dir(&cs_dir)
        .map_err(|e| format!("Failed to read {}: {}", cs_dir.display(), e))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "json"))
        .collect();

    entries.sort_by_key(|e| e.path());
    if let Some(latest) = entries.last() {
        let content = std::fs::read_to_string(latest.path())
            .map_err(|e| format!("Failed to read {}: {}", latest.path().display(), e))?;
        let cs: Changeset = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse {}: {}", latest.path().display(), e))?;
        Ok(Some(cs))
    } else {
        Ok(None)
    }
}

pub fn save(cs: &Changeset) -> Result<(), String> {
    let cs_dir = changesets_dir();
    std::fs::create_dir_all(&cs_dir)
        .map_err(|e| format!("Failed to create {}: {}", cs_dir.display(), e))?;

    let cs_path = changeset_path(&cs.id);
    let content = serde_json::to_string_pretty(cs)
        .map_err(|e| format!("JSON serialize: {}", e))?;
    std::fs::write(&cs_path, &content)
        .map_err(|e| format!("Failed to write {}: {}", cs_path.display(), e))?;

    let plan_p = plan_path();
    std::fs::write(&plan_p, &content)
        .map_err(|e| format!("Failed to write {}: {}", plan_p.display(), e))?;

    Ok(())
}

pub fn clear_plan_file() -> Result<(), String> {
    let plan_p = plan_path();
    if plan_p.exists() {
        std::fs::remove_file(&plan_p)
            .map_err(|e| format!("Failed to remove plan file: {}", e))?;
    }
    Ok(())
}

pub fn verify_no_newxos_mutation(_cfg: &WorkspaceConfig, cs: &Changeset) -> Result<(), String> {
    for rp in &cs.repos {
        if rp.path == "newxos" || rp.path.starts_with("newxos/") {
            return Err(format!(
                "Changeset includes '{}' which is in newxos. Stitch must not mutate newxos.",
                rp.path
            ));
        }
        for f in &rp.files {
            if f.starts_with("newxos") || f.contains("/newxos/") {
                return Err(format!(
                    "Changeset file '{}' is inside newxos. Stitch must not mutate newxos.",
                    f
                ));
            }
        }
    }
    Ok(())
}
