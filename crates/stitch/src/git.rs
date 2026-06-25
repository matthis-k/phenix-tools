use std::path::Path;
use std::process::Command;

use crate::model::RepoStatus;

pub fn get_status(repo_path: &Path) -> Result<RepoStatus, String> {
    let name = repo_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let branch = git_branch(repo_path)?;
    let porcelain = git_porcelain(repo_path)?;

    let mut staged = 0usize;
    let mut unstaged = 0usize;
    let mut untracked = 0usize;

    for line in porcelain.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let flags = if line.len() >= 2 { &line[..2] } else { line };
        match flags {
            "??" => untracked += 1,
            "!!" => {} // ignored
            _ => {
                let c = flags.chars().next().unwrap_or(' ');
                let c2 = flags.chars().nth(1).unwrap_or(' ');
                if c != ' ' && c != '?' && c != '!' {
                    staged += 1;
                }
                if c2 != ' ' && c2 != '?' && c2 != '!' {
                    unstaged += 1;
                }
            }
        }
    }

    let is_dirty = staged > 0 || unstaged > 0 || untracked > 0;

    let path_str = repo_path.to_string_lossy().to_string();

    Ok(RepoStatus {
        name,
        path: path_str,
        branch,
        is_dirty,
        staged_count: staged,
        unstaged_count: unstaged,
        untracked_count: untracked,
        ahead: None,
        behind: None,
    })
}

fn git_branch(repo: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(repo)
        .output()
        .map_err(|e| format!("git branch: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "Not a git repo or no HEAD: {}",
            repo.display()
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn git_porcelain(repo: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .args(["status", "--porcelain=v1"])
        .current_dir(repo)
        .output()
        .map_err(|e| format!("git status: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git status --porcelain: {}", stderr.trim()));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub fn git_diff_names(repo: &Path) -> Result<Vec<String>, String> {
    git_diff_with_flags(repo, &["diff", "--name-only"])
}

pub fn git_diff_cached_names(repo: &Path) -> Result<Vec<String>, String> {
    git_diff_with_flags(repo, &["diff", "--cached", "--name-only"])
}

fn git_diff_with_flags(repo: &Path, args: &[&str]) -> Result<Vec<String>, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .map_err(|e| format!("git diff: {}", e))?;

    let mut files = Vec::new();
    if output.status.success() {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let t = line.trim();
            if !t.is_empty() {
                files.push(t.to_string());
            }
        }
    }
    Ok(files)
}

#[allow(dead_code)]
pub fn git_head(repo: &Path) -> Result<String, String> {
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

pub fn git_short_head(repo: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(repo)
        .output()
        .map_err(|e| format!("git rev-parse --short HEAD: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git rev-parse --short HEAD: {}", stderr.trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn git_add(repo: &Path, files: &[String]) -> Result<(), String> {
    let mut args = vec!["add", "--"];
    for f in files {
        args.push(f.as_str());
    }
    let output = Command::new("git")
        .args(&args)
        .current_dir(repo)
        .output()
        .map_err(|e| format!("git add: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git add: {}", stderr.trim()));
    }
    Ok(())
}

pub fn git_commit(repo: &Path, message: &str) -> Result<(), String> {
    let output = Command::new("git")
        .args(["commit", "-m", message])
        .current_dir(repo)
        .output()
        .map_err(|e| format!("git commit: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git commit: {}", stderr.trim()));
    }
    Ok(())
}

pub fn has_merge_conflict_markers(repo: &Path) -> Result<bool, String> {
    let output = Command::new("git")
        .args(["grep", "-l", "^<<<<<<< \\|^=======\\|^>>>>>>> "])
        .current_dir(repo)
        .output()
        .map_err(|e| format!("git grep conflict: {}", e))?;
    // Exit code 0 means found matches, 1 means none
    Ok(output.status.success())
}

pub fn is_mid_merge(repo: &Path) -> Result<bool, String> {
    let mergets = repo.join(".git/MERGE_MSG");
    Ok(mergets.exists())
}

#[cfg(test)]
mod tests {
    fn count_porcelain(input: &str) -> (usize, usize, usize) {
        let mut staged = 0;
        let mut unstaged = 0;
        let mut untracked = 0;
        for raw_line in input.lines() {
            if raw_line.trim().is_empty() { continue; }
            let flags = if raw_line.len() >= 2 { &raw_line[..2] } else { raw_line };
            match flags {
                "??" => untracked += 1,
                "!!" => {}
                _ => {
                    let c = flags.chars().next().unwrap_or(' ');
                    let c2 = flags.chars().nth(1).unwrap_or(' ');
                    if c != ' ' && c != '?' && c != '!' { staged += 1; }
                    if c2 != ' ' && c2 != '?' && c2 != '!' { unstaged += 1; }
                }
            }
        }
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
}
