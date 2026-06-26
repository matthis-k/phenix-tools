use phenix_mcp_core::mcp::{McpTool, ToolContext};
use phenix_mcp_core::result::{ErrorKind, ToolFailure, ToolResult};
use phenix_mcp_core::types::{MutationLevel, ToolMetadata};
use serde_json::{json, Value};

fn mk_err(kind: ErrorKind, msg: &str, audit_id: &str) -> ToolFailure {
    ToolFailure::new(kind, msg, audit_id)
}

fn tool_meta(mutation: MutationLevel) -> ToolMetadata {
    ToolMetadata {
        mutation,
        requires_plan: None,
        requires_clean_worktree: None,
        requires_confirmation: None,
        allowed_roots_only: Some(true),
    }
}

fn walk_tend_configs(root: &std::path::Path) -> Result<Vec<(tend::discover::DiscoveredNode, tend::model::ResolvedNode)>, String> {
    let discovered = tend::discover::discover_configs(root, None).map_err(|e| format!("{}", e))?;
    let resolved = tend::discover::resolve_nodes(root, discovered.clone());
    Ok(discovered.into_iter().zip(resolved).collect())
}

fn get_changed_files(root: &std::path::Path) -> Vec<String> {
    let mut all = Vec::new();
    for args in [&["diff", "--name-only"] as &[&str], &["diff", "--cached", "--name-only"]] {
        if let Ok(output) = std::process::Command::new("git").args(args).current_dir(root).output() {
            if output.status.success() {
                for line in String::from_utf8_lossy(&output.stdout).lines() {
                    let t = line.trim();
                    if !t.is_empty() { all.push(t.to_string()); }
                }
            }
        }
    }
    all.sort();
    all.dedup();
    all
}

pub struct TendStatusTool;

impl McpTool for TendStatusTool {
    fn name(&self) -> &str { "tend.status" }
    fn description(&self) -> &str { "Show known checks, config health, and config tree" }
    fn metadata(&self) -> ToolMetadata { tool_meta(MutationLevel::ReadOnly) }
    fn input_schema(&self) -> Value {
        json!({
            "root": { "type": "string", "description": "Root directory" },
            "json": { "type": "boolean", "description": "Output as JSON" }
        })
    }
    fn call(&self, input: Value, ctx: &ToolContext) -> Result<Value, ToolFailure> {
        let audit_id = ctx.audit.generate_id();
        let root = input.get("root").and_then(|v| v.as_str()).map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        let pairs = match walk_tend_configs(&root) {
            Ok(p) => p,
            Err(e) => return Err(mk_err(ErrorKind::NotFound, &format!("Discovery: {}", e), &audit_id)),
        };

        let configs: Vec<Value> = pairs.iter().map(|(d, r)| {
            json!({
                "config_path": d.config_path.to_string_lossy(),
                "node_path": d.node_path.to_string_lossy(),
                "id": r.id,
                "description": r.description,
                "tags": r.tags,
                "task_count": r.tasks.len()
            })
        }).collect();

        let result = ToolResult::ok(
            json!({ "configs": configs, "total": configs.len() }),
            format!("{} config(s), {} task(s) across workspace", configs.len(),
                pairs.iter().map(|(_, r)| r.tasks.len()).sum::<usize>()),
            &audit_id,
        );
        Ok(serde_json::to_value(&result).unwrap_or_default())
    }
}

pub struct TendPlanTool;

impl McpTool for TendPlanTool {
    fn name(&self) -> &str { "tend.plan" }
    fn description(&self) -> &str { "Show which checks would run and why (read-only check plan)" }
    fn metadata(&self) -> ToolMetadata { tool_meta(MutationLevel::ReadOnly) }
    fn input_schema(&self) -> Value {
        json!({
            "root": { "type": "string" },
            "files": { "type": "array", "items": { "type": "string" } },
            "groups": { "type": "array", "items": { "type": "string" } },
            "targets": { "type": "array", "items": { "type": "string" } },
            "mode": { "type": "string", "enum": ["changed", "staged", "all", "selected"] },
            "base": { "type": "string" },
            "json": { "type": "boolean" }
        })
    }
    fn call(&self, input: Value, ctx: &ToolContext) -> Result<Value, ToolFailure> {
        let audit_id = ctx.audit.generate_id();
        let root = std::env::current_dir().unwrap_or_default();
        let mode = input.get("mode").and_then(|v| v.as_str()).unwrap_or("changed");

        let changed_files = match mode {
            "changed" | "staged" => Some(get_changed_files(&root)),
            _ => None,
        };

        let explicit_files: Vec<String> = input.get("files")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str()).map(|s| s.to_string()).collect())
            .unwrap_or_default();

        let files = if !explicit_files.is_empty() { Some(explicit_files) } else { changed_files };
        let file_slice = files.as_deref();

        let pairs = match walk_tend_configs(&root) {
            Ok(p) => p,
            Err(e) => return Err(mk_err(ErrorKind::NotFound, &format!("Discovery: {}", e), &audit_id)),
        };

        let checks: Vec<Value> = pairs.iter().flat_map(|(_, node)| {
            node.tasks.iter().filter_map(|task| {
                let should_run = match mode {
                    "all" | "force" => true,
                    _ => {
                        if let Some(ref when) = task.config.when {
                            if let Some(ref changed) = when.changed {
                                if let Some(cf) = file_slice {
                                    tend::planner::task_matches_paths(&changed.paths, cf)
                                } else {
                                    true
                                }
                            } else { true }
                        } else { true }
                    }
                };
                if !should_run { return None; }

                let matched_files: Vec<String> = match (mode, file_slice) {
                    ("changed" | "staged", Some(cf)) => {
                        if let Some(ref when) = task.config.when {
                            if let Some(ref changed) = when.changed {
                                cf.iter().filter(|f| tend::planner::task_matches_paths(&changed.paths, &[(*f).clone()])).cloned().collect()
                            } else { vec![] }
                        } else { vec![] }
                    },
                    _ => vec![],
                };

                let reason = if !matched_files.is_empty() {
                    format!("matched {} pattern(s)", matched_files.len())
                } else {
                    "selected explicitly".to_string()
                };

                Some(json!({
                    "id": task.config.id,
                    "group": node.id,
                    "kind": task.config.kind,
                    "phase": task.config.phase,
                    "reason": reason,
                    "files": matched_files,
                    "depends_on": []
                }))
            })
        }).collect();

        let result = ToolResult::ok(
            json!({ "checks": checks, "total": checks.len() }),
            format!("{} check(s) would run", checks.len()),
            &audit_id,
        );
        Ok(serde_json::to_value(&result).unwrap_or_default())
    }
}

pub struct TendRunTool;

impl McpTool for TendRunTool {
    fn name(&self) -> &str { "tend.run" }
    fn description(&self) -> &str { "Execute checks, selected by files/groups/targets or from a saved plan" }
    fn metadata(&self) -> ToolMetadata { tool_meta(MutationLevel::WritesWorktree) }
    fn input_schema(&self) -> Value {
        json!({
            "files": { "type": "array", "items": { "type": "string" } },
            "groups": { "type": "array", "items": { "type": "string" } },
            "targets": { "type": "array", "items": { "type": "string" } },
            "mode": { "type": "string", "enum": ["changed", "staged", "all", "selected"] },
            "fail_fast": { "type": "boolean" },
            "timeout_seconds": { "type": "number" },
            "output": { "type": "string", "enum": ["summary", "full", "failed_only"] }
        })
    }
    fn call(&self, input: Value, ctx: &ToolContext) -> Result<Value, ToolFailure> {
        let audit_id = ctx.audit.generate_id();
        let root = std::env::current_dir().unwrap_or_default();
        let mode = input.get("mode").and_then(|v| v.as_str()).unwrap_or("changed");
        let fail_fast = input.get("fail_fast").and_then(|v| v.as_bool()).unwrap_or(false);

        let pairs = match walk_tend_configs(&root) {
            Ok(p) => p,
            Err(e) => return Err(mk_err(ErrorKind::NotFound, &format!("Discovery: {}", e), &audit_id)),
        };

        let changed_files = match mode {
            "changed" | "staged" => Some(get_changed_files(&root)),
            _ => None,
        };

        let nodes: Vec<_> = pairs.into_iter().map(|(_, r)| r).collect();
        let plan = match tend::planner::build_plan(&nodes, "verify", if mode == "all" { "force" } else { mode }, changed_files.as_deref()) {
            Ok(p) => p,
            Err(e) => return Err(mk_err(ErrorKind::Internal, &format!("Plan: {}", e), &audit_id)),
        };

        if plan.items.is_empty() {
            let result = ToolResult::ok(
                json!({ "passed": 0, "failed": 0, "skipped": 0, "failures": [] }),
                "No checks to run", &audit_id,
            );
            return Ok(serde_json::to_value(&result).unwrap_or_default());
        }

        let results = tend::execute::execute_plan(&plan.items, &root);
        let mut passed = 0; let mut failed = 0; let mut skipped = 0;
        let mut failures: Vec<Value> = Vec::new();

        for r in &results {
            if r.skipped { skipped += 1; }
            else if r.passed { passed += 1; }
            else {
                failed += 1;
                failures.push(json!({
                    "check_id": r.task_id, "kind": r.kind,
                    "reason": r.reason,
                    "stdout_tail": r.stdout.chars().take(500).collect::<String>(),
                    "stderr_tail": r.stderr.chars().take(500).collect::<String>()
                }));
                if fail_fast { break; }
            }
        }

        let result = ToolResult::ok(
            json!({ "passed": passed, "failed": failed, "skipped": skipped, "failures": failures }),
            format!("{} passed, {} failed, {} skipped", passed, failed, skipped),
            &audit_id,
        );
        Ok(serde_json::to_value(&result).unwrap_or_default())
    }
}

pub struct TendExplainTool;

impl McpTool for TendExplainTool {
    fn name(&self) -> &str { "tend.explain" }
    fn description(&self) -> &str { "Explain a check failure with repro command and likely causes" }
    fn metadata(&self) -> ToolMetadata { tool_meta(MutationLevel::ReadOnly) }
    fn input_schema(&self) -> Value {
        json!({
            "run_id": { "type": "string", "description": "Run ID from tend.run" },
            "check_id": { "type": "string", "description": "Optional check ID to filter" },
            "include_repro_command": { "type": "boolean" }
        })
    }
    fn call(&self, input: Value, ctx: &ToolContext) -> Result<Value, ToolFailure> {
        let audit_id = ctx.audit.generate_id();
        let check_id = input.get("check_id").and_then(|v| v.as_str());
        let root = std::env::current_dir().unwrap_or_default();

        let pairs = match walk_tend_configs(&root) {
            Ok(p) => p,
            Err(e) => return Err(mk_err(ErrorKind::NotFound, &format!("Discovery: {}", e), &audit_id)),
        };

        let nodes: Vec<_> = pairs.into_iter().map(|(_, r)| r).collect();
        let plan = match tend::planner::build_plan(&nodes, "verify", "force", None) {
            Ok(p) => p,
            Err(e) => return Err(mk_err(ErrorKind::Internal, &format!("Plan: {}", e), &audit_id)),
        };

        let items: Vec<_> = if let Some(cid) = check_id {
            plan.items.iter().filter(|item| item.task_id == cid).collect()
        } else {
            plan.items.iter().collect()
        };

        if items.is_empty() {
            return Err(mk_err(ErrorKind::NotFound, "No matching checks found", &audit_id));
        }

        let exec_results = tend::execute::execute_plan(
            &items.iter().map(|i| (*i).clone()).collect::<Vec<_>>(), &root,
        );

        let explanations: Vec<Value> = exec_results.iter().filter(|r| !r.passed && !r.skipped).map(|r| {
            json!({
                "check_id": r.task_id,
                "failure_kind": "exit_code",
                "explanation": r.reason,
                "relevant_output": r.stderr.chars().take(1000).collect::<String>(),
                "likely_causes": ["Check command failed with non-zero exit code"]
            })
        }).collect();

        let result = ToolResult::ok(
            json!({ "failures": explanations, "total_failures": explanations.len() }),
            format!("Found {} failure(s)", explanations.len()), &audit_id,
        );
        Ok(serde_json::to_value(&result).unwrap_or_default())
    }
}
