use std::fs;
use std::path::Path;
use std::process::Command;

use tend::discover;
use tend::execute;
use tend::model::{Phase, PlanRequest, RunMode};
use tend::planner;

fn clippy_available() -> bool {
    Command::new("cargo")
        .args(["clippy", "--help"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn init_git_repo(path: &Path) {
    Command::new("git")
        .args(["init"])
        .current_dir(path)
        .output()
        .expect("git init should succeed");
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(path)
        .output()
        .expect("git config should succeed");
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(path)
        .output()
        .expect("git config should succeed");
}

fn setup_minimal_crate(root: &Path, src: &str) {
    let cargo_toml = r#"[package]
name = "test-crate"
version = "0.1.0"
edition = "2021"
"#;
    fs::write(root.join("Cargo.toml"), cargo_toml).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), src).unwrap();
}

fn run_tend_config(root: &Path) -> Vec<execute::ExecutionResult> {
    let discovered = discover::discover_configs(root, None).unwrap();
    let nodes = discover::resolve_nodes(root, discovered);

    let req = PlanRequest {
        phase: Phase::Verify,
        mode: RunMode::Force,
        group: None,
        target: None,
        files: vec![],
    };

    let plan = planner::build_plan(&nodes, &req).unwrap();
    execute::execute_plan(&plan.items, root)
}

#[test]
fn cargo_check_and_clippy_discovered_and_run() {
    if !clippy_available() {
        eprintln!("skipping clippy test: cargo clippy not available");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    init_git_repo(root);
    setup_minimal_crate(root, "pub fn hello() -> &'static str { \"hello\" }\n");

    let config = r#"{
        "version": 1,
        "node": {
            "id": "test",
            "tasks": [
                {
                    "id": "cargo-check",
                    "phase": "verify",
                    "kind": "command",
                    "command": ["cargo", "check", "--quiet", "--workspace", "--all-targets"]
                },
                {
                    "id": "cargo-clippy",
                    "phase": "verify",
                    "kind": "command",
                    "command": ["cargo", "clippy", "--quiet", "--workspace", "--all-targets", "--", "-D", "warnings"]
                }
            ]
        }
    }"#;
    fs::write(root.join(".tend.json"), config).unwrap();

    let discovered = discover::discover_configs(root, None).unwrap();
    let nodes = discover::resolve_nodes(root, discovered);

    let req = PlanRequest {
        phase: Phase::Verify,
        mode: RunMode::Force,
        group: None,
        target: None,
        files: vec![],
    };

    let plan = planner::build_plan(&nodes, &req).unwrap();
    assert!(plan.items.iter().any(|i| i.task_id == "cargo-check"),
        "cargo-check should be in the plan");
    assert!(plan.items.iter().any(|i| i.task_id == "cargo-clippy"),
        "cargo-clippy should be in the plan");

    let results = execute::execute_plan(&plan.items, root);

    let check = results.iter().find(|r| r.task_id == "cargo-check")
        .expect("cargo-check result should exist");
    assert!(check.outcome.is_pass(),
        "cargo check should pass: stderr={:?} stdout={:?}",
        check.stderr, check.stdout);

    let clippy = results.iter().find(|r| r.task_id == "cargo-clippy")
        .expect("cargo-clippy result should exist");
    assert!(clippy.outcome.is_pass(),
        "cargo clippy should pass on clean code: stderr={:?} stdout={:?}",
        clippy.stderr, clippy.stdout);

    assert!(!clippy.stdout.contains("warning:"),
        "clippy stdout should not contain warnings: {:?}", clippy.stdout);
    assert!(!clippy.stderr.contains("warning:"),
        "clippy stderr should not contain warnings: {:?}", clippy.stderr);
}

#[test]
fn cargo_clippy_fails_on_warnings() {
    if !clippy_available() {
        eprintln!("skipping clippy negative test: cargo clippy not available");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    init_git_repo(root);
    // Code that triggers a clippy warning: integer suffix on `1`
    setup_minimal_crate(root, "pub fn add_one(x: i32) -> i32 { x + 1i32 }\n");

    let config = r#"{
        "version": 1,
        "node": {
            "id": "test",
            "tasks": [
                {
                    "id": "cargo-clippy",
                    "phase": "verify",
                    "kind": "command",
                    "command": ["cargo", "clippy", "--quiet", "--workspace", "--all-targets", "--", "-D", "warnings"]
                }
            ]
        }
    }"#;
    fs::write(root.join(".tend.json"), config).unwrap();

    let results = run_tend_config(root);

    let clippy = results.iter().find(|r| r.task_id == "cargo-clippy")
        .expect("cargo-clippy result should exist");
    assert!(clippy.outcome.is_failure(),
        "cargo clippy should fail on warning code: stderr={:?} stdout={:?}",
        clippy.stderr, clippy.stdout);
}
