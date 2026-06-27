use std::collections::BTreeSet;
use std::path::Path;

use crate::discover;
use crate::execute;
use crate::model::{Phase, PlanRequest, RunMode};
use crate::planner;
use crate::report;

pub struct Submodule {
    pub name: String,
    pub path: String,
}

pub struct TopoNode {
    pub name: String,
    pub role: String,
    pub layer: u32,
}

pub fn parse_gitmodules(root: &Path) -> Result<Vec<Submodule>, String> {
    let gm_path = root.join(".gitmodules");
    if !gm_path.exists() {
        return Ok(Vec::new());
    }
    let content =
        std::fs::read_to_string(&gm_path).map_err(|e| format!("Failed to read .gitmodules: {e}"))?;
    let mut submodules = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_path: Option<String> = None;

    for line in content.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if t.starts_with('[') && t.ends_with(']') {
            if let (Some(name), Some(path)) = (current_name.take(), current_path.take()) {
                submodules.push(Submodule { name, path });
            }
            let inner = &t[1..t.len() - 1].trim();
            if let Some(sub_name) = inner.strip_prefix("submodule \"") {
                if let Some(end) = sub_name.strip_suffix('"') {
                    current_name = Some(end.to_string());
                    current_path = None;
                }
            }
        } else if let Some((key, value)) = t.split_once('=') {
            let k = key.trim();
            let v = value.trim().trim_matches('"');
            if k == "path" && current_name.is_some() {
                current_path = Some(v.to_string());
            }
        }
    }

    if let (Some(name), Some(path)) = (current_name, current_path) {
        submodules.push(Submodule { name, path });
    }

    Ok(submodules)
}

pub fn parse_topology(root: &Path) -> Vec<TopoNode> {
    let topo_path = root.join(".stitch").join("topology.json");
    if !topo_path.exists() {
        return Vec::new();
    }
    let content = match std::fs::read_to_string(&topo_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let val: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let repos = match val.get("repos").and_then(|v| v.as_array()) {
        Some(r) => r,
        None => return Vec::new(),
    };

    let mut topo = Vec::new();
    for repo in repos {
        let name = match repo.get("name").and_then(|v| v.as_str()) {
            Some(n) => n,
            None => continue,
        };
        let role = repo
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let layer = repo.get("layer").and_then(|v| v.as_u64()).unwrap_or(999) as u32;
        topo.push(TopoNode {
            name: name.to_string(),
            role: role.to_string(),
            layer,
        });
    }

    topo
}

pub fn get_changed_files(root: &Path) -> Result<Vec<String>, String> {
    let mut all = Vec::new();

    let output = std::process::Command::new("git")
        .args(["diff", "--name-only"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("git diff: {e}"))?;
    if output.status.success() {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let t = line.trim();
            if !t.is_empty() {
                all.push(t.to_string());
            }
        }
    }

    let output = std::process::Command::new("git")
        .args(["diff", "--cached", "--name-only"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("git diff --cached: {e}"))?;
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

fn get_changed_gitlinks(root: &Path) -> Result<Vec<String>, String> {
    let mut changed = Vec::new();

    let output = std::process::Command::new("git")
        .args(["diff", "--name-only"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("git diff: {e}"))?;
    if output.status.success() {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            changed.push(line.trim().to_string());
        }
    }

    let output = std::process::Command::new("git")
        .args(["diff", "--cached", "--name-only"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("git diff --cached: {e}"))?;
    if output.status.success() {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            changed.push(line.trim().to_string());
        }
    }

    changed.sort();
    changed.dedup();
    Ok(changed)
}

fn submodule_is_dirty(root: &Path, sub_path: &str) -> Result<bool, String> {
    let sub_full = root.join(sub_path);
    if !sub_full.join(".git").exists() {
        return Ok(false);
    }
    let output = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&sub_full)
        .output()
        .map_err(|e| format!("git status in submodule: {e}"))?;
    let status = String::from_utf8_lossy(&output.stdout);
    Ok(status.lines().any(|l| !l.trim().is_empty()))
}

pub fn run_affected_dag(
    root: &Path,
    phase: Phase,
    mode: RunMode,
    profile: Option<&str>,
) -> Result<i32, String> {
    let submodules = parse_gitmodules(root)?;
    let topology = parse_topology(root);
    let changed_root_files = get_changed_files(root)?;
    let changed_gitlinks = get_changed_gitlinks(root)?;

    let mut affected: BTreeSet<String> = BTreeSet::new();

    affected.insert("root".to_string());

    for sub in &submodules {
        let gitlink_changed = changed_gitlinks.iter().any(|g| g == &sub.path);
        let dirty = if !sub.path.is_empty() {
            submodule_is_dirty(root, &sub.path)?
        } else {
            false
        };
        let root_matches = changed_root_files.iter().any(|f| f.starts_with(&sub.path));

        if gitlink_changed || dirty || root_matches {
            affected.insert(sub.name.clone());
        }
    }

    if !topology.is_empty() {
        let topo_by_name: std::collections::BTreeMap<&str, u32> = topology
            .iter()
            .map(|t| (t.name.as_str(), t.layer))
            .collect();

        let mut to_process: Vec<String> = affected.iter().cloned().collect();
        while let Some(name) = to_process.pop() {
            let layer = topo_by_name.get(name.as_str()).copied().unwrap_or(0);
            for (topo_name, topo_layer) in &topo_by_name {
                if *topo_layer > layer && !affected.contains(*topo_name) {
                    affected.insert((*topo_name).to_string());
                    to_process.push((*topo_name).to_string());
                }
            }
        }
    }

    let mut any_failed = false;
    for sub in &submodules {
        if affected.contains(&sub.name) && !root.join(&sub.path).join(".tend.json").exists() {
            eprintln!(
                "ERROR: Affected submodule '{}' ({}) has no .tend.json",
                sub.name, sub.path
            );
            any_failed = true;
        }
    }
    if any_failed {
        return Ok(1);
    }

    println!("Affected workspace nodes ({}):", affected.len());
    for name in &affected {
        println!("  - {name}");
    }
    println!();

    let mut global_failed = false;

    for name in &affected {
        let node_path = if *name == "root" || *name == "phenix" {
            root.to_path_buf()
        } else if let Some(sub) = submodules.iter().find(|s| s.name == *name) {
            root.join(&sub.path)
        } else {
            continue;
        };

        if !node_path.join(".tend.json").exists() {
            eprintln!("SKIPPED: '{name}' (no .tend.json)");
            global_failed = true;
            continue;
        }

        if !node_path.join(".git").exists() && *name != "root" && *name != "phenix" {
            println!("Skipping uninitialized submodule '{name}'");
            continue;
        }

        println!("--- Checking '{name}' ---");

        let discovered = match discover::discover_configs(&node_path, None) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("  DISCOVERY FAILED: {e}");
                global_failed = true;
                continue;
            }
        };
        let nodes = discover::resolve_nodes(&node_path, discovered);

        let files = get_changed_files(&node_path).unwrap_or_default();

        let req = PlanRequest {
            phase,
            mode,
            profile: profile.map(|s| s.to_string()),
            group: None,
            target: None,
            files,
            offline: false,
            locked: false,
        };

        let plan = match planner::build_plan(&nodes, &req) {
            Ok(p) => p,
            Err(planner::PlanError::MutatingRefused(id)) => {
                eprintln!("  MUTATING TASK REFUSED: {id}");
                global_failed = true;
                continue;
            }
        };

        if plan.items.is_empty() {
            println!("  No matching tasks for '{name}'");
            continue;
        }

        println!("  Running {} task(s)", plan.items.len());
        let results = execute::execute_plan(&plan.items, &node_path);
        let (failed, _passed, _skipped) = report::print_results(&results, false);

        if failed > 0 {
            global_failed = true;
        }
    }

    if global_failed {
        Ok(1)
    } else {
        println!("\nAll affected-DAG checks passed.");
        Ok(0)
    }
}
