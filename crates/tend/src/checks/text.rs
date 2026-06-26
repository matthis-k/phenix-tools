use std::path::Path;

use globset::{GlobBuilder, GlobSetBuilder};

use crate::model::Step;

use super::CheckResult;

pub fn run_forbid(step: &Step, workdir: &Path) -> CheckResult {
    let patterns = &step.patterns;
    let path_globs = &step.paths;

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

pub fn run_require(step: &Step, workdir: &Path) -> CheckResult {
    let patterns = &step.patterns;
    let path_globs = &step.paths;

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

    let mut missing = patterns.clone();

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
    use crate::model::Step;
    use std::fs;

    fn test_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn make_step(kind: &str, paths: Vec<String>, patterns: Vec<String>) -> Step {
        Step {
            kind: kind.to_string(),
            command: vec![],
            paths,
            patterns,
            always: false,
            description: String::new(),
            expect: None,
        }
    }

    #[test]
    fn test_forbid_text_finds_violation() {
        let dir = test_dir();
        fs::write(dir.path().join("test.md"), b"contains id=\"secret\" here").unwrap();

        let step = make_step(
            "forbidText",
            vec!["*.md".to_string()],
            vec!["id=\"".to_string()],
        );

        let result = run_forbid(&step, dir.path());
        assert!(!result.passed);
        assert!(result.reason.contains("forbidden pattern"));
    }

    #[test]
    fn test_forbid_text_clean() {
        let dir = test_dir();
        fs::write(dir.path().join("test.md"), b"clean file no issues").unwrap();

        let step = make_step(
            "forbidText",
            vec!["*.md".to_string()],
            vec!["id=\"".to_string()],
        );

        let result = run_forbid(&step, dir.path());
        assert!(result.passed);
    }

    #[test]
    fn test_require_text_found() {
        let dir = test_dir();
        fs::write(
            dir.path().join("readme.md"),
            b"This document describes the intended workflow",
        )
        .unwrap();

        let step = make_step(
            "requireText",
            vec!["*.md".to_string()],
            vec!["intended workflow".to_string()],
        );

        let result = run_require(&step, dir.path());
        assert!(result.passed);
    }

    #[test]
    fn test_require_text_missing() {
        let dir = test_dir();
        fs::write(dir.path().join("readme.md"), b"other content").unwrap();

        let step = make_step(
            "requireText",
            vec!["*.md".to_string()],
            vec!["required phrase".to_string()],
        );

        let result = run_require(&step, dir.path());
        assert!(!result.passed);
    }
}
