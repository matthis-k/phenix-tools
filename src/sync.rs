use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::graph::Graph;
use crate::node::SyncConfig;

pub struct SyncManager {
    pub base_dir: PathBuf,
    /// Workspace root (parent of nodes.json) for updating submodule pointers
    pub workspace: PathBuf,
    /// Optional custom commit message override
    pub commit_message: Option<String>,
}

impl SyncManager {
    pub fn new(base_dir: &Path, workspace: &Path, commit_message: Option<String>) -> Self {
        Self {
            base_dir: base_dir.to_path_buf(),
            workspace: workspace.to_path_buf(),
            commit_message,
        }
    }

    fn commit_msg(&self, default: &str) -> String {
        self.commit_message
            .clone()
            .unwrap_or_else(|| default.to_string())
    }

    pub fn read_nodes_file(path: &Path) -> Result<Vec<String>, String> {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("Failed to read nodes file: {}", e))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse nodes file: {}", e))
    }

    pub fn read_sync_config(path: &Path) -> Result<SyncConfig, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))
    }

    pub fn build_dag(&self, entry_points: &[String]) -> Result<Graph, String> {
        let mut graph = Graph::new();
        let mut known_repos: HashMap<String, PathBuf> = HashMap::new();

        for entry in entry_points {
            let path = Path::new(entry);
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| format!("Invalid path: {}", entry))?;
            known_repos.insert(name.to_string(), path.to_path_buf());
            graph.add_node(name);
        }

        loop {
            let current_names: Vec<String> = graph.adjacency.keys().cloned().collect();
            let mut added = false;

            for name in &current_names {
                let repo_path = known_repos
                    .get(name)
                    .ok_or_else(|| format!("Repo '{}' has no known path", name))?;

                let sync_path = repo_path.join("sync.json");
                if !sync_path.exists() {
                    continue;
                }

                let config = Self::read_sync_config(&sync_path)?;

                for dep in &config.depends_on {
                    if !graph.adjacency.contains_key(dep) {
                        let dep_path = self.base_dir.join(dep);
                        if !dep_path.exists() {
                            return Err(format!(
                                "Dependency '{}' of '{}' not found at {}",
                                dep,
                                name,
                                dep_path.display()
                            ));
                        }
                        known_repos.insert(dep.clone(), dep_path);
                        added = true;
                    }
                    graph.add_dep(name, dep);
                }
            }

            if !added {
                break;
            }
        }

        Ok(graph)
    }

    pub fn run_sync(&self, order: &[String]) -> Result<(), String> {
        let mut heads: HashMap<String, String> = HashMap::new();
        let mut completed: Vec<String> = Vec::new();

        let ws_head = self.git_head(&self.workspace).ok();
        if let Some(ref h) = ws_head {
            heads.insert("__workspace__".to_string(), h.clone());
        }

        let result = (|| -> Result<(), String> {
            for repo_name in order {
                let repo_path = self.base_dir.join(repo_name);
                println!("  [sync] {}:", repo_name);

                let head = self.git_head(&repo_path)?;
                heads.insert(repo_name.clone(), head);

                println!("    fetching...");
                self.git_pull(&repo_path)?;

                let sync_path = repo_path.join("sync.json");
                if sync_path.exists() {
                    let config = Self::read_sync_config(&sync_path)?;

                    for input in &config.update_inputs {
                        println!("    updating input: {}", input);
                        self.nix_flake_update(&repo_path, input)?;

                        if self.git_has_changes(&repo_path)? {
                            let msg = self.commit_msg(&format!("sync(automated): {}", input));
                            println!("    committing: {}", msg);
                            self.git_commit(&repo_path, &msg)?;
                            self.git_push(&repo_path)?;
                        }
                    }

                    for check in &config.checks {
                        println!("    check: {}", check);
                        self.run_check(&repo_path, check)?;
                    }
                }

                completed.push(repo_name.clone());
            }

            self.update_workspace_submodules()?;
            Ok(())
        })();

        if let Err(e) = result {
            eprintln!("Sync FAILED at step: {}", e);
            eprintln!("Rolling back {} modified repos...", completed.len());
            for repo_name in completed.iter().rev() {
                if let Some(head) = heads.get(repo_name) {
                    let repo_path = self.base_dir.join(repo_name);
                    eprintln!("  resetting {} to {}", repo_name, head);
                    if let Err(re) = self.git_reset(&repo_path, head) {
                        eprintln!("  WARN: rollback failed for {}: {}", repo_name, re);
                    }
                }
            }
            if let Some(head) = heads.get("__workspace__") {
                if self.git_has_changes(&self.workspace).unwrap_or(false) {
                    eprintln!("  resetting workspace to {}", head);
                    let _ = self.git_reset(&self.workspace, head);
                }
            }
            return Err("Sync failed and was rolled back".to_string());
        }

        Ok(())
    }

    fn update_workspace_submodules(&self) -> Result<(), String> {
        let ws = &self.workspace;
        if self.git_has_changes(ws)? {
            let msg = self.commit_msg("sync: update submodule pointers");
            println!("  [workspace] {}", msg);
            self.git_commit(ws, &msg)?;
            self.git_push(ws)?;
            println!("  [workspace] done.");
        } else {
            println!("  [workspace] no submodule changes.");
        }
        Ok(())
    }

    fn git_head(&self, repo: &Path) -> Result<String, String> {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo)
            .output()
            .map_err(|e| format!("git rev-parse HEAD: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git rev-parse HEAD: {}", stderr.trim()));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn git_pull(&self, repo: &Path) -> Result<(), String> {
        let output = Command::new("git")
            .args(["pull", "--ff-only"])
            .current_dir(repo)
            .output()
            .map_err(|e| format!("git pull: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git pull: {}", stderr.trim()));
        }
        Ok(())
    }

    fn git_has_changes(&self, repo: &Path) -> Result<bool, String> {
        let output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(repo)
            .output()
            .map_err(|e| format!("git status: {}", e))?;
        let out = String::from_utf8_lossy(&output.stdout);
        Ok(!out.trim().is_empty())
    }

    fn git_commit(&self, repo: &Path, msg: &str) -> Result<(), String> {
        Command::new("git")
            .args(["add", "-A"])
            .current_dir(repo)
            .output()
            .map_err(|e| format!("git add: {}", e))?;

        let output = Command::new("git")
            .args(["commit", "-m", msg])
            .current_dir(repo)
            .output()
            .map_err(|e| format!("git commit: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git commit: {}", stderr.trim()));
        }
        Ok(())
    }

    fn git_push(&self, repo: &Path) -> Result<(), String> {
        let output = Command::new("git")
            .args(["push"])
            .current_dir(repo)
            .output()
            .map_err(|e| format!("git push: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git push: {}", stderr.trim()));
        }
        Ok(())
    }

    fn git_reset(&self, repo: &Path, head: &str) -> Result<(), String> {
        let output = Command::new("git")
            .args(["reset", "--hard", head])
            .current_dir(repo)
            .output()
            .map_err(|e| format!("git reset: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git reset --hard {}: {}", head, stderr.trim()));
        }
        Ok(())
    }

    fn nix_flake_update(&self, repo: &Path, input: &str) -> Result<(), String> {
        let output = Command::new("nix")
            .args(["flake", "update", input])
            .current_dir(repo)
            .output()
            .map_err(|e| format!("nix flake update: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("nix flake update {}: {}", input, stderr.trim()));
        }
        Ok(())
    }

    fn run_check(&self, repo: &Path, check: &str) -> Result<(), String> {
        let parts: Vec<&str> = check.split_whitespace().collect();
        if parts.is_empty() {
            return Ok(());
        }
        let output = Command::new(parts[0])
            .args(&parts[1..])
            .current_dir(repo)
            .output()
            .map_err(|e| format!("Running '{}': {}", check, e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Check '{}': {}", check, stderr.trim()));
        }
        Ok(())
    }
}
