use crate::git;
use crate::model::{RepoStatus, WorkspaceConfig};

pub fn collect_all(cfg: &WorkspaceConfig) -> Result<Vec<RepoStatus>, String> {
    let mut statuses = Vec::new();
    for repo in &cfg.repos {
        let path = repo.resolved_path(cfg);
        if !path.exists() || !path.join(".git").exists() {
            statuses.push(RepoStatus {
                name: repo.name.clone(),
                path: repo.path.clone(),
                branch: String::new(),
                is_dirty: false,
                staged_count: 0,
                unstaged_count: 0,
                untracked_count: 0,
                ahead: None,
                behind: None,
            });
            continue;
        }
        let s = git::get_status(&path)?;
        statuses.push(s);
    }
    Ok(statuses)
}
