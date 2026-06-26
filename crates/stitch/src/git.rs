use std::path::{Path, PathBuf};
use std::process::Command;

use crate::model::{RepoAvailability, RepoStatus};

/// A typed representation of a single changed file from `git status --porcelain=v1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitChangeKind {
    /// Modified but not staged
    Modified,
    /// Added to index (staged)
    Added,
    /// Deleted from index (staged)
    Deleted,
    /// Renamed (staged)
    Renamed,
    /// Copied (staged)
    Copied,
    /// Type changed (e.g. file → symlink)
    TypeChanged,
    /// Untracked file
    Untracked,
    /// Modified in index and worktree
    StagedAndModified,
    /// Deleted in worktree (not staged)
    WorktreeDeleted,
    /// Other / unknown two-letter code
    Other(String),
}

#[derive(Debug, Clone)]
pub struct GitChange {
    pub kind: GitChangeKind,
    pub path: String,
    pub old_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GitStatus {
    pub branch: String,
    pub changes: Vec<GitChange>,
}

impl GitStatus {
    pub fn is_dirty(&self) -> bool {
        !self.changes.is_empty()
    }

    pub fn staged_count(&self) -> usize {
        self.changes.iter().filter(|c| {
            matches!(c.kind, GitChangeKind::Added | GitChangeKind::Deleted | GitChangeKind::Renamed | GitChangeKind::Copied | GitChangeKind::TypeChanged | GitChangeKind::StagedAndModified)
        }).count()
    }

    pub fn unstaged_count(&self) -> usize {
        self.changes.iter().filter(|c| {
            matches!(c.kind, GitChangeKind::Modified | GitChangeKind::WorktreeDeleted | GitChangeKind::StagedAndModified)
        }).count()
    }

    pub fn untracked_count(&self) -> usize {
        self.changes.iter().filter(|c| {
            matches!(c.kind, GitChangeKind::Untracked)
        }).count()
    }

    pub fn staged_files(&self) -> Vec<&str> {
        self.changes.iter().filter_map(|c| {
            if matches!(c.kind, GitChangeKind::Added | GitChangeKind::Deleted | GitChangeKind::Renamed | GitChangeKind::Copied | GitChangeKind::TypeChanged | GitChangeKind::StagedAndModified) {
                Some(c.path.as_str())
            } else {
                None
            }
        }).collect()
    }

    pub fn unstaged_files(&self) -> Vec<&str> {
        self.changes.iter().filter_map(|c| {
            if matches!(c.kind, GitChangeKind::Modified | GitChangeKind::WorktreeDeleted | GitChangeKind::StagedAndModified) {
                Some(c.path.as_str())
            } else {
                None
            }
        }).collect()
    }

    pub fn all_files(&self) -> Vec<&str> {
        self.changes.iter().map(|c| c.path.as_str()).collect()
    }
}

/// A typed handle to a git repository.
#[derive(Debug, Clone)]
pub struct GitRepo {
    path: PathBuf,
}

impl GitRepo {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn open(path: &Path) -> Result<Self, String> {
        // Verify it's a git repo
        let output = Command::new("git")
            .args(["rev-parse", "--git-dir"])
            .current_dir(path)
            .output()
            .map_err(|e| format!("git rev-parse: {}", e))?;
        if !output.status.success() {
            return Err(format!("Not a git repository: {}", path.display()));
        }
        Ok(Self { path: path.to_path_buf() })
    }

    pub fn status(&self) -> Result<GitStatus, String> {
        let branch = self.branch()?;
        let porcelain = self.porcelain()?;
        let changes = parse_porcelain(&porcelain);
        Ok(GitStatus { branch, changes })
    }

    pub fn branch(&self) -> Result<String, String> {
        let output = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(&self.path)
            .output()
            .map_err(|e| format!("git branch: {}", e))?;
        if !output.status.success() {
            return Err(format!("Not a git repo or no HEAD: {}", self.path.display()));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    pub fn porcelain(&self) -> Result<String, String> {
        let output = Command::new("git")
            .args(["status", "--porcelain=v1"])
            .current_dir(&self.path)
            .output()
            .map_err(|e| format!("git status: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git status --porcelain: {}", stderr.trim()));
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    pub fn head(&self) -> Result<String, String> {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&self.path)
            .output()
            .map_err(|e| format!("git rev-parse HEAD: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git rev-parse HEAD: {}", stderr.trim()));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    pub fn short_head(&self) -> Result<String, String> {
        let output = Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .current_dir(&self.path)
            .output()
            .map_err(|e| format!("git rev-parse --short HEAD: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git rev-parse --short HEAD: {}", stderr.trim()));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    pub fn add(&self, files: &[String]) -> Result<(), String> {
        let mut args = vec!["add", "--"];
        for f in files {
            args.push(f.as_str());
        }
        let output = Command::new("git")
            .args(&args)
            .current_dir(&self.path)
            .output()
            .map_err(|e| format!("git add: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git add: {}", stderr.trim()));
        }
        Ok(())
    }

    pub fn commit(&self, message: &str) -> Result<(), String> {
        let output = Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(&self.path)
            .output()
            .map_err(|e| format!("git commit: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git commit: {}", stderr.trim()));
        }
        Ok(())
    }

    pub fn push(&self, branch: &str) -> Result<(), String> {
        let output = Command::new("git")
            .args(["push", "origin", branch])
            .current_dir(&self.path)
            .output()
            .map_err(|e| format!("git push: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git push failed: {}", stderr.trim()));
        }
        Ok(())
    }

    pub fn ahead_count(&self) -> Result<usize, String> {
        let output = Command::new("git")
            .args(["rev-list", "--count", "@{upstream}..HEAD"])
            .current_dir(&self.path)
            .output()
            .map_err(|e| format!("git rev-list: {}", e))?;
        if !output.status.success() {
            return Ok(0);
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout.trim().parse::<usize>().or(Ok(0))
    }

    pub fn has_merge_conflicts(&self) -> Result<bool, String> {
        let output = Command::new("git")
            .args(["grep", "-l", "^<<<<<<< \\|^=======\\|^>>>>>>> "])
            .current_dir(&self.path)
            .output()
            .map_err(|e| format!("git grep conflict: {}", e))?;
        Ok(output.status.success())
    }

    pub fn is_mid_merge(&self) -> Result<bool, String> {
        let mergets = self.path.join(".git/MERGE_MSG");
        Ok(mergets.exists())
    }

    pub fn remote_url(&self, name: &str) -> Result<String, String> {
        let output = Command::new("git")
            .args(["remote", "get-url", name])
            .current_dir(&self.path)
            .output()
            .map_err(|e| format!("git remote get-url: {}", e))?;
        if !output.status.success() {
            return Err(format!("No remote '{}' in {}", name, self.path.display()));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

/// Parse `git status --porcelain=v1` output into a list of typed changes.
pub fn parse_porcelain(porcelain: &str) -> Vec<GitChange> {
    let mut changes = Vec::new();
    for line in porcelain.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let (flags, path_part) = if line.len() >= 3 {
            let (f, p) = line.split_at(2);
            (f, p.trim())
        } else if line.len() == 2 {
            (line, "")
        } else {
            continue;
        };

        let chars: Vec<char> = flags.chars().collect();
        let (x, y) = if chars.len() >= 2 { (chars[0], chars[1]) } else { (' ', ' ') };

        if flags == "??" {
            changes.push(GitChange {
                kind: GitChangeKind::Untracked,
                path: path_part.to_string(),
                old_path: None,
            });
            continue;
        }

        if flags == "!!" {
            // Ignored - skip
            continue;
        }

        // Handle rename/copy: "R100 old\0new" or "C100 old\0new"
        let (path, old_path) = if x == 'R' || x == 'C' {
            let parts: Vec<&str> = path_part.split('\0').collect();
            if parts.len() >= 2 {
                (parts[1].to_string(), Some(parts[0].to_string()))
            } else {
                (path_part.to_string(), None)
            }
        } else {
            (path_part.to_string(), None)
        };

        let kind = match (x, y) {
            ('M', ' ') | ('M', _) if y != ' ' => GitChangeKind::StagedAndModified,
            ('M', _) => GitChangeKind::Added, // staged modification
            ('A', _) => GitChangeKind::Added,
            ('D', ' ') => GitChangeKind::Deleted,
            (' ', 'D') => GitChangeKind::WorktreeDeleted,
            (' ', 'M') => GitChangeKind::Modified,
            ('R', _) => GitChangeKind::Renamed,
            ('C', _) => GitChangeKind::Copied,
            ('T', _) => GitChangeKind::TypeChanged,
            _ => GitChangeKind::Other(format!("{}{}", x, y)),
        };

        changes.push(GitChange { kind, path, old_path });
    }
    changes
}

pub fn get_status(name: &str, repo_path: &Path) -> Result<RepoStatus, String> {
    let repo = GitRepo::open(repo_path)?;
    let git_status = repo.status()?;

    Ok(RepoStatus {
        name: name.to_string(),
        path: repo_path.to_string_lossy().to_string(),
        branch: git_status.branch,
        is_dirty: git_status.is_dirty(),
        status: RepoAvailability::Present,
        staged_count: git_status.staged_count(),
        unstaged_count: git_status.unstaged_count(),
        untracked_count: git_status.untracked_count(),
        ahead: None,
        behind: None,
    })
}

// Legacy free-function wrappers that delegate to GitRepo
pub fn git_branch(repo: &Path) -> Result<String, String> {
    GitRepo::open(repo)?.branch()
}

pub fn git_porcelain(repo: &Path) -> Result<String, String> {
    GitRepo::open(repo)?.porcelain()
}

pub fn git_head(repo: &Path) -> Result<String, String> {
    GitRepo::open(repo)?.head()
}

pub fn git_short_head(repo: &Path) -> Result<String, String> {
    GitRepo::open(repo)?.short_head()
}

pub fn git_add(repo: &Path, files: &[String]) -> Result<(), String> {
    GitRepo::open(repo)?.add(files)
}

pub fn git_commit(repo: &Path, message: &str) -> Result<(), String> {
    GitRepo::open(repo)?.commit(message)
}

pub fn git_push(repo: &Path, branch: &str) -> Result<(), String> {
    GitRepo::open(repo)?.push(branch)
}

pub fn git_diff_names(repo: &Path) -> Result<Vec<String>, String> {
    let git_status = GitRepo::open(repo)?.status()?;
    Ok(git_status.all_files().into_iter().map(|s| s.to_string()).collect())
}

pub fn git_diff_cached_names(repo: &Path) -> Result<Vec<String>, String> {
    let git_status = GitRepo::open(repo)?.status()?;
    Ok(git_status.staged_files().into_iter().map(|s| s.to_string()).collect())
}

pub fn has_merge_conflict_markers(repo: &Path) -> Result<bool, String> {
    GitRepo::open(repo)?.has_merge_conflicts()
}

pub fn git_ahead_count(repo: &Path, _branch: &str, _remote: &str) -> Result<usize, String> {
    GitRepo::open(repo)?.ahead_count()
}

pub fn is_mid_merge(repo: &Path) -> Result<bool, String> {
    GitRepo::open(repo)?.is_mid_merge()
}

pub fn git_remote(repo: &Path, name: &str) -> Result<String, String> {
    GitRepo::open(repo)?.remote_url(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count_porcelain(input: &str) -> (usize, usize, usize) {
        let changes = parse_porcelain(input);
        let staged = changes.iter().filter(|c| {
            matches!(c.kind, GitChangeKind::Added | GitChangeKind::Deleted | GitChangeKind::Renamed | GitChangeKind::Copied | GitChangeKind::TypeChanged | GitChangeKind::StagedAndModified)
        }).count();
        let unstaged = changes.iter().filter(|c| {
            matches!(c.kind, GitChangeKind::Modified | GitChangeKind::WorktreeDeleted | GitChangeKind::StagedAndModified)
        }).count();
        let untracked = changes.iter().filter(|c| {
            matches!(c.kind, GitChangeKind::Untracked)
        }).count();
        (staged, unstaged, untracked)
    }

    #[test]
    fn test_parse_porcelain_clean() {
        let (staged, unstaged, untracked) = count_porcelain("");
        assert_eq!(staged, 0);
        assert_eq!(unstaged, 0);
        assert_eq!(untracked, 0);
    }

    #[test]
    fn test_parse_porcelain_dirty() {
        let input = " M modified.rs\nA  added.rs\n?? untracked.rs\n";
        let (staged, unstaged, untracked) = count_porcelain(input);
        assert_eq!(staged, 1);
        assert_eq!(unstaged, 1);
        assert_eq!(untracked, 1);
    }

    #[test]
    fn test_parse_porcelain_all_staged() {
        let input = "M  staged_mod.rs\nA  added.rs\nD  deleted.rs\n";
        let changes = parse_porcelain(input);
        assert!(changes.iter().any(|c| matches!(c.kind, GitChangeKind::Added)));
        assert_eq!(changes.len(), 3);
    }
}
