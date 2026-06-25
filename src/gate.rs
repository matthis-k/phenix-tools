use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Subcommand;
use glob::Pattern;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct ChecksFile {
    version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    scope: Option<ScopeConfig>,
    checks: Vec<CheckConfig>,
}

#[derive(Debug, Deserialize)]
struct ScopeConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    workdir: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CheckConfig {
    id: String,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    when: Option<WhenConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    workdir: Option<String>,
    command: Vec<String>,
    #[serde(default)]
    expect: ExpectConfig,
}

#[derive(Debug, Clone, Deserialize)]
struct WhenConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    paths: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Default)]
struct ExpectConfig {
    #[serde(default)]
    status: Option<i32>,
}

#[derive(Debug)]
struct Check {
    id: String,
    description: String,
    group: Option<String>,
    tags: Option<Vec<String>>,
    when: Option<WhenConfig>,
    workdir_setting: Option<String>,
    command: Vec<String>,
    expect: ExpectConfig,
    source: PathBuf,
    resolved_workdir: PathBuf,
}

struct CheckResult {
    id: String,
    source: PathBuf,
    resolved_workdir: PathBuf,
    command: Vec<String>,
    expect_status: Option<i32>,
    actual_status: Option<i32>,
    stdout: String,
    stderr: String,
    skipped: bool,
}

#[derive(Subcommand)]
pub enum GateCommands {
    /// List all known checks
    List,
    /// Run all checks
    All,
    /// Run checks affected by changed files (git diff)
    Changed,
    /// Run a specific check by ID
    Id { id: String },
}

pub fn dispatch(
    command: GateCommands,
    config_path: Option<PathBuf>,
    workspace_root: &Path,
) -> Result<(), String> {
    let checks = discover_checks(config_path, workspace_root)?;

    match command {
        GateCommands::List => {
            for check in &checks {
                let group = check.group.as_deref().unwrap_or("(no group)");
                println!(
                    "  {}  [{}]  {}",
                    check.id, group, check.description
                );
            }
            println!("\nTotal: {} checks", checks.len());
            Ok(())
        }
        GateCommands::All => run_checks(checks.iter().collect(), None),
        GateCommands::Changed => {
            let changed = get_changed_files(workspace_root)?;
            if changed.is_empty() {
                println!("No changed files detected.");
                return Ok(());
            }
            println!("Changed files:");
            for f in &changed {
                println!("  {}", f);
            }
            println!();
            run_checks(checks.iter().collect(), Some(&changed))
        }
        GateCommands::Id { id } => {
            let matched: Vec<&Check> = checks.iter().filter(|c| c.id == id).collect();
            if matched.is_empty() {
                return Err(format!("Check '{}' not found", id));
            }
            run_checks(matched, None)
        }
    }
}

fn discover_checks(
    config_path: Option<PathBuf>,
    workspace_root: &Path,
) -> Result<Vec<Check>, String> {
    let mut config_files = Vec::new();

    if let Some(explicit) = config_path {
        if !explicit.exists() {
            return Err(format!("Config file not found: {}", explicit.display()));
        }
        config_files.push(explicit);
    } else {
        collect_check_files(workspace_root, &mut config_files)?;
    }

    if config_files.is_empty() {
        return Err("No .phenix-checks.json files found.".to_string());
    }

    config_files.sort();

    let mut seen_ids = HashSet::new();
    let mut checks = Vec::new();

    for path in &config_files {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;

        let parsed: ChecksFile = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))?;

        if parsed.version < 1 {
            return Err(format!(
                "{}: unsupported version {}",
                path.display(),
                parsed.version
            ));
        }

        let config_dir = path.parent().unwrap_or(workspace_root);
        let file_scope_workdir = parsed
            .scope
            .as_ref()
            .and_then(|s| s.workdir.as_deref())
            .unwrap_or("config");

        for check_cfg in &parsed.checks {
            if !seen_ids.insert(check_cfg.id.clone()) {
                return Err(format!(
                    "Duplicate check ID '{}' (from {})",
                    check_cfg.id,
                    path.display()
                ));
            }

            let setting = check_cfg
                .workdir
                .as_deref()
                .unwrap_or(file_scope_workdir);
            let resolved_workdir =
                resolve_workdir(setting, config_dir, workspace_root)?;

            checks.push(Check {
                id: check_cfg.id.clone(),
                description: check_cfg.description.clone(),
                group: check_cfg.group.clone(),
                tags: check_cfg.tags.clone(),
                when: check_cfg.when.clone(),
                workdir_setting: check_cfg.workdir.clone(),
                command: check_cfg.command.clone(),
                expect: ExpectConfig {
                    status: check_cfg.expect.status,
                },
                source: path.clone(),
                resolved_workdir,
            });
        }
    }

    Ok(checks)
}

fn resolve_workdir(
    setting: &str,
    config_dir: &Path,
    workspace_root: &Path,
) -> Result<PathBuf, String> {
    match setting {
        "config" => Ok(config_dir.to_path_buf()),
        "repo" => Ok(
            find_git_root(config_dir).unwrap_or_else(|| workspace_root.to_path_buf()),
        ),
        "cwd" => Ok(std::env::current_dir()
            .map_err(|e| format!("Cannot get cwd: {}", e))?),
        _ => {
            let candidate = config_dir.join(setting);
            if candidate.exists() {
                Ok(candidate)
            } else {
                Ok(config_dir.to_path_buf())
            }
        }
    }
}

fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut current = Some(start.to_path_buf());
    while let Some(dir) = current {
        if dir.join(".git").exists() {
            return Some(dir);
        }
        current = dir.parent().map(|p| p.to_path_buf());
    }
    None
}

fn collect_check_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if !dir.is_dir() {
        return Ok(());
    }

    let check_file = dir.join(".phenix-checks.json");
    if check_file.exists() {
        files.push(check_file);
    }

    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("Failed to read directory {}: {}", dir.display(), e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with('.') {
                    continue;
                }
                if name == "target" {
                    continue;
                }
            }
            collect_check_files(&path, files)?;
        }
    }

    Ok(())
}

fn get_changed_files(workspace_root: &Path) -> Result<Vec<String>, String> {
    let mut all = Vec::new();

    let output = Command::new("git")
        .args(["diff", "--name-only"])
        .current_dir(workspace_root)
        .output()
        .map_err(|e| format!("git diff: {}", e))?;

    if output.status.success() {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let t = line.trim();
            if !t.is_empty() {
                all.push(t.to_string());
            }
        }
    }

    let output = Command::new("git")
        .args(["diff", "--cached", "--name-only"])
        .current_dir(workspace_root)
        .output()
        .map_err(|e| format!("git diff --cached: {}", e))?;

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

fn check_affected(check: &Check, changed_files: &[String]) -> bool {
    let when = match &check.when {
        Some(w) => w,
        None => return true,
    };

    let patterns = match &when.paths {
        Some(p) => p,
        None => return true,
    };

    if patterns.is_empty() {
        return true;
    }

    for pattern_str in patterns {
        let pat = match Pattern::new(pattern_str) {
            Ok(p) => p,
            Err(_) => continue,
        };
        for changed in changed_files {
            if pat.matches(changed) {
                return true;
            }
        }
    }

    false
}

fn run_checks(checks: Vec<&Check>, changed_files: Option<&[String]>) -> Result<(), String> {
    let mut failed = 0;
    let mut passed = 0;
    let mut skipped = 0;
    let mut results = Vec::new();

    for check in checks {
        if let Some(changed) = changed_files {
            if !check_affected(check, changed) {
                skipped += 1;
                continue;
            }
        }

        let result = execute_check(check);
        if result.skipped {
            skipped += 1;
            results.push(result);
            continue;
        }

        let expected = result.expect_status.unwrap_or(0);
        if result.actual_status == Some(expected) {
            passed += 1;
        } else {
            failed += 1;
        }
        results.push(result);
    }

    for r in &results {
        if r.skipped {
            continue;
        }
        let expected = r.expect_status.unwrap_or(0);
        if r.actual_status != Some(expected) {
            println!("FAILED {}", r.id);
            println!("  config: {}", r.source.display());
            println!("  workdir: {}", r.resolved_workdir.display());
            println!("  command: {}", r.command.join(" "));
            println!(
                "  status: {} (expected {})",
                r.actual_status.unwrap_or(-1),
                expected
            );
            for line in r.stdout.lines() {
                println!("  stdout: {}", line);
            }
            for line in r.stderr.lines() {
                println!("  stderr: {}", line);
            }
            println!();
        }
    }

    println!("Summary:");
    println!("  failed: {}", failed);
    println!("  passed: {}", passed);
    println!("  skipped: {}", skipped);

    if failed > 0 {
        Err(format!("{} check(s) failed", failed))
    } else {
        Ok(())
    }
}

fn execute_check(check: &Check) -> CheckResult {
    if check.command.is_empty() {
        return CheckResult {
            id: check.id.clone(),
            source: check.source.clone(),
            resolved_workdir: check.resolved_workdir.clone(),
            command: check.command.clone(),
            expect_status: check.expect.status,
            actual_status: None,
            stdout: String::new(),
            stderr: String::new(),
            skipped: true,
        };
    }

    let program = &check.command[0];
    let args: Vec<&str> = check.command[1..].iter().map(|s| s.as_str()).collect();

    let output = match Command::new(program)
        .args(&args)
        .current_dir(&check.resolved_workdir)
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            return CheckResult {
                id: check.id.clone(),
                source: check.source.clone(),
                resolved_workdir: check.resolved_workdir.clone(),
                command: check.command.clone(),
                expect_status: check.expect.status,
                actual_status: None,
                stdout: String::new(),
                stderr: format!("Failed to execute: {}", e),
                skipped: true,
            };
        }
    };

    CheckResult {
        id: check.id.clone(),
        source: check.source.clone(),
        resolved_workdir: check.resolved_workdir.clone(),
        command: check.command.clone(),
        expect_status: check.expect.status,
        actual_status: Some(output.status.code().unwrap_or(-1)),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        skipped: false,
    }
}
