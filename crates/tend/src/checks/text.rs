use std::path::Path;

use globset::{GlobBuilder, GlobSetBuilder};

use super::CheckResult;

pub fn run_forbid(path_globs: &[String], patterns: &[String], workdir: &Path) -> CheckResult {
    if path_globs.is_empty() || patterns.is_empty() {
        return CheckResult::skip();
    }

    let mut glob_builder = GlobSetBuilder::new();
    for p in path_globs {
        let glob = match GlobBuilder::new(p).literal_separator(true).build() {
            Ok(g) => g,
            Err(e) => return CheckResult::error(format!("invalid glob '{p}': {e}")),
        };
        glob_builder.add(glob);
    }
    let glob_set = match glob_builder.build() {
        Ok(g) => g,
        Err(e) => return CheckResult::error(format!("glob build: {e}")),
    };

    let mut violations = Vec::new();

    let walker = walkdir::WalkDir::new(workdir).into_iter();
    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let name = entry.file_name().to_str().unwrap_or("");
        if entry.file_type().is_dir() && name.starts_with('.') && entry.depth() > 0 {
            continue;
        }

        if !entry.file_type().is_file() {
            continue;
        }

        let rel = match entry.path().strip_prefix(workdir) {
            Ok(r) => r,
            Err(_) => continue,
        };

        if !glob_set.is_match(rel) {
            continue;
        }

        let content = match std::fs::read_to_string(entry.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };

        for pattern in patterns {
            if content.contains(pattern.as_str()) {
                violations.push(format!(
                    "found forbidden pattern '{}' in {}",
                    pattern,
                    rel.display()
                ));
                break;
            }
        }
    }

    if violations.is_empty() {
        CheckResult::pass()
    } else {
        CheckResult::fail(violations.join("; "))
    }
}

pub fn run_require(path_globs: &[String], patterns: &[String], workdir: &Path) -> CheckResult {
    if path_globs.is_empty() || patterns.is_empty() {
        return CheckResult::skip();
    }

    let mut glob_builder = GlobSetBuilder::new();
    for p in path_globs {
        let glob = match GlobBuilder::new(p).literal_separator(true).build() {
            Ok(g) => g,
            Err(e) => return CheckResult::error(format!("invalid glob '{p}': {e}")),
        };
        glob_builder.add(glob);
    }
    let glob_set = match glob_builder.build() {
        Ok(g) => g,
        Err(e) => return CheckResult::error(format!("glob build: {e}")),
    };

    let mut missing = patterns.to_vec();

    let walker = walkdir::WalkDir::new(workdir).into_iter();
    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let name = entry.file_name().to_str().unwrap_or("");
        if entry.file_type().is_dir() && name.starts_with('.') && entry.depth() > 0 {
            continue;
        }

        if !entry.file_type().is_file() {
            continue;
        }

        let rel = match entry.path().strip_prefix(workdir) {
            Ok(r) => r,
            Err(_) => continue,
        };

        if !glob_set.is_match(rel) {
            continue;
        }

        let content = match std::fs::read_to_string(entry.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };

        missing.retain(|p| !content.contains(p.as_str()));
    }

    if missing.is_empty() {
        CheckResult::pass()
    } else {
        CheckResult::fail(format!(
            "required patterns not found: {}",
            missing.join(", ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn test_forbid_text_finds_violation() {
        let dir = test_dir();
        fs::write(dir.path().join("test.md"), b"contains id=\"secret\" here").unwrap();

        let result = run_forbid(&["*.md".to_string()], &["id=\"".to_string()], dir.path());
        assert!(result.outcome.is_failure());
        match &result.outcome {
            crate::checks::CheckOutcome::Failed { reason } => {
                assert!(reason.contains("forbidden pattern"))
            }
            _ => panic!("expected failure"),
        }
    }

    #[test]
    fn test_forbid_text_clean() {
        let dir = test_dir();
        fs::write(dir.path().join("test.md"), b"clean file no issues").unwrap();

        let result = run_forbid(&["*.md".to_string()], &["id=\"".to_string()], dir.path());
        assert!(result.outcome.is_pass());
    }

    #[test]
    fn test_require_text_found() {
        let dir = test_dir();
        fs::write(
            dir.path().join("readme.md"),
            b"This document describes the intended workflow",
        )
        .unwrap();

        let result = run_require(
            &["*.md".to_string()],
            &["intended workflow".to_string()],
            dir.path(),
        );
        assert!(result.outcome.is_pass());
    }

    #[test]
    fn test_require_text_missing() {
        let dir = test_dir();
        fs::write(dir.path().join("readme.md"), b"other content").unwrap();

        let result = run_require(
            &["*.md".to_string()],
            &["required phrase".to_string()],
            dir.path(),
        );
        assert!(result.outcome.is_failure());
    }
}
