use std::str::FromStr;

use phenix_mcp_core::input::parse_tool_input;
use phenix_mcp_core::mcp::{McpTool, ToolContext};
use phenix_mcp_core::result::{ErrorKind, ToolFailure, ToolResult};
use phenix_mcp_core::types::{MutationLevel, ToolMetadata};
use serde::Deserialize;
use serde_json::{json, Value};
use tend::model::{Phase, PlanRequest, RunMode};

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

fn walk_tend_configs(
    root: &std::path::Path,
) -> Result<Vec<(tend::discover::DiscoveredNode, tend::model::ResolvedNode)>, String> {
    let discovered = tend::discover::discover_configs(root, None).map_err(|e| format!("{}", e))?;
    let resolved = tend::discover::resolve_nodes(root, discovered.clone());
    Ok(discovered.into_iter().zip(resolved).collect())
}

fn get_changed_files(root: &std::path::Path) -> Vec<String> {
    let mut all = Vec::new();
    for args in [
        &["diff", "--name-only"] as &[&str],
        &["diff", "--cached", "--name-only"],
    ] {
        if let Ok(output) = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
        {
            if output.status.success() {
                for line in String::from_utf8_lossy(&output.stdout).lines() {
                    let t = line.trim();
                    if !t.is_empty() {
                        all.push(t.to_string());
                    }
                }
            }
        }
    }
    all.sort();
    all.dedup();
    all
}

pub struct TendStatusTool;

#[derive(Deserialize, Default)]
struct StatusInput {
    #[serde(default)]
    root: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    json: bool,
}

impl McpTool for TendStatusTool {
    fn name(&self) -> &str {
        "tend.status"
    }
    fn description(&self) -> &str {
        "Show known checks, config health, and config tree"
    }
    fn metadata(&self) -> ToolMetadata {
        tool_meta(MutationLevel::ReadOnly)
    }
    fn input_schema(&self) -> Value {
        json!({
            "root": { "type": "string", "description": "Root directory" },
            "json": { "type": "boolean", "description": "Output as JSON" }
        })
    }
    fn call(&self, input: Value, ctx: &ToolContext) -> Result<Value, ToolFailure> {
        let audit_id = ctx.audit.generate_id();
        let typed: StatusInput = parse_tool_input(&input, &audit_id)?;
        let root = typed
            .root
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        let pairs = match walk_tend_configs(&root) {
            Ok(p) => p,
            Err(e) => {
                return Err(mk_err(
                    ErrorKind::NotFound,
                    &format!("Discovery: {}", e),
                    &audit_id,
                ))
            }
        };

        let configs: Vec<Value> = pairs
            .iter()
            .map(|(d, r)| {
                json!({
                    "config_path": d.config_path.to_string_lossy(),
                    "node_path": d.node_path.to_string_lossy(),
                    "id": r.id,
                    "description": r.description,
                    "tags": r.tags,
                    "task_count": r.tasks.len()
                })
            })
            .collect();

        let result = ToolResult::ok(
            json!({ "configs": configs, "total": configs.len() }),
            format!(
                "{} config(s), {} task(s) across workspace",
                configs.len(),
                pairs.iter().map(|(_, r)| r.tasks.len()).sum::<usize>()
            ),
            &audit_id,
        );
        Ok(serde_json::to_value(&result).unwrap_or_default())
    }
}

#[derive(Deserialize, Default)]
struct PlanInput {
    #[serde(default)]
    root: Option<String>,
    #[serde(default)]
    phase: Option<String>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    groups: Vec<String>,
    #[serde(default)]
    targets: Vec<String>,
    #[serde(default)]
    #[allow(dead_code)]
    json: bool,
}

impl PlanInput {
    fn to_plan_request(&self, root: &std::path::Path) -> PlanRequest {
        let mode = RunMode::from_str(self.mode.as_deref().unwrap_or("changed"))
            .unwrap_or(RunMode::Changed);
        let phase =
            Phase::from_str(self.phase.as_deref().unwrap_or("verify")).unwrap_or(Phase::Verify);
        let mut files = self.files.clone();
        if matches!(mode, RunMode::Changed | RunMode::Staged) && files.is_empty() {
            files = get_changed_files(root);
        }
        let group = if self.groups.is_empty() {
            None
        } else {
            Some(self.groups[0].clone())
        };
        let target = if self.targets.is_empty() {
            None
        } else {
            Some(self.targets[0].clone())
        };
        PlanRequest {
            phase,
            mode,
            group,
            target,
            files,
        }
    }
}

pub struct TendPlanTool;

impl McpTool for TendPlanTool {
    fn name(&self) -> &str {
        "tend.plan"
    }
    fn description(&self) -> &str {
        "Show which checks would run and why (read-only check plan)"
    }
    fn metadata(&self) -> ToolMetadata {
        tool_meta(MutationLevel::ReadOnly)
    }
    fn input_schema(&self) -> Value {
        json!({
            "root": { "type": "string", "description": "Root directory" },
            "phase": { "type": "string", "enum": ["verify", "fix", "generate", "setup", "cleanup"] },
            "files": { "type": "array", "items": { "type": "string" } },
            "groups": { "type": "array", "items": { "type": "string" } },
            "targets": { "type": "array", "items": { "type": "string" } },
            "mode": { "type": "string", "enum": ["changed", "staged", "all", "force", "selected"] },
            "base": { "type": "string" },
            "json": { "type": "boolean" }
        })
    }
    fn call(&self, input: Value, ctx: &ToolContext) -> Result<Value, ToolFailure> {
        let audit_id = ctx.audit.generate_id();
        let typed: PlanInput = parse_tool_input(&input, &audit_id)?;
        let root = typed
            .root
            .as_ref()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        let req = typed.to_plan_request(&root);

        let pairs = match walk_tend_configs(&root) {
            Ok(p) => p,
            Err(e) => {
                return Err(mk_err(
                    ErrorKind::NotFound,
                    &format!("Discovery: {}", e),
                    &audit_id,
                ))
            }
        };

        let nodes: Vec<_> = pairs.into_iter().map(|(_, r)| r).collect();

        let plan = match tend::planner::build_plan(&nodes, &req) {
            Ok(p) => p,
            Err(e) => {
                return Err(mk_err(
                    ErrorKind::Internal,
                    &format!("Plan: {}", e),
                    &audit_id,
                ))
            }
        };

        let checks: Vec<Value> = plan
            .items
            .iter()
            .map(|item| {
                json!({
                    "id": item.task_id,
                    "group": item.chain_id.split('.').next().unwrap_or(&item.task_id),
                    "kind": item.step.kind.description(),
                    "phase": item.phase,
                    "reason": item.reason.to_string(),
                    "files": item.matched_files,
                    "depends_on": []
                })
            })
            .collect();

        let result = ToolResult::ok(
            json!({ "checks": checks, "total": checks.len() }),
            format!("{} check(s) would run", checks.len()),
            &audit_id,
        );
        Ok(serde_json::to_value(&result).unwrap_or_default())
    }
}

pub struct TendRunTool;

#[derive(Deserialize, Default)]
struct RunInput {
    #[serde(default)]
    root: Option<String>,
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    groups: Vec<String>,
    #[serde(default)]
    targets: Vec<String>,
    #[serde(default)]
    phase: Option<String>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    fail_fast: bool,
}

impl RunInput {
    fn to_plan_request(&self, root: &std::path::Path) -> PlanRequest {
        let mode = RunMode::from_str(self.mode.as_deref().unwrap_or("changed"))
            .unwrap_or(RunMode::Changed);
        let phase =
            Phase::from_str(self.phase.as_deref().unwrap_or("verify")).unwrap_or(Phase::Verify);
        let mut files = self.files.clone();
        if matches!(mode, RunMode::Changed | RunMode::Staged) && files.is_empty() {
            files = get_changed_files(root);
        }
        let group = if self.groups.is_empty() {
            None
        } else {
            Some(self.groups[0].clone())
        };
        let target = if self.targets.is_empty() {
            None
        } else {
            Some(self.targets[0].clone())
        };
        PlanRequest {
            phase,
            mode,
            group,
            target,
            files,
        }
    }
}

impl McpTool for TendRunTool {
    fn name(&self) -> &str {
        "tend.run"
    }
    fn description(&self) -> &str {
        "Execute checks, selected by files/groups/targets or from a saved plan"
    }
    fn metadata(&self) -> ToolMetadata {
        tool_meta(MutationLevel::WritesWorktree)
    }
    fn input_schema(&self) -> Value {
        json!({
            "root": { "type": "string", "description": "Root directory" },
            "files": { "type": "array", "items": { "type": "string" } },
            "groups": { "type": "array", "items": { "type": "string" } },
            "targets": { "type": "array", "items": { "type": "string" } },
            "phase": { "type": "string", "enum": ["verify", "fix", "generate", "setup", "cleanup"] },
            "mode": { "type": "string", "enum": ["changed", "staged", "all", "force", "selected"] },
            "fail_fast": { "type": "boolean" },
            "timeout_seconds": { "type": "number" },
            "output": { "type": "string", "enum": ["summary", "full", "failed_only"] }
        })
    }
    fn call(&self, input: Value, ctx: &ToolContext) -> Result<Value, ToolFailure> {
        let audit_id = ctx.audit.generate_id();
        let typed: RunInput = parse_tool_input(&input, &audit_id)?;
        let root = typed
            .root
            .as_ref()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        let fail_fast = typed.fail_fast;
        let req = typed.to_plan_request(&root);

        let pairs = match walk_tend_configs(&root) {
            Ok(p) => p,
            Err(e) => {
                return Err(mk_err(
                    ErrorKind::NotFound,
                    &format!("Discovery: {}", e),
                    &audit_id,
                ))
            }
        };

        let nodes: Vec<_> = pairs.into_iter().map(|(_, r)| r).collect();
        let plan = match tend::planner::build_plan(&nodes, &req) {
            Ok(p) => p,
            Err(e) => {
                return Err(mk_err(
                    ErrorKind::Internal,
                    &format!("Plan: {}", e),
                    &audit_id,
                ))
            }
        };

        if plan.items.is_empty() {
            let result = ToolResult::ok(
                json!({ "passed": 0, "failed": 0, "skipped": 0, "failures": [] }),
                "No checks to run",
                &audit_id,
            );
            return Ok(serde_json::to_value(&result).unwrap_or_default());
        }

        let results = tend::execute::execute_plan(&plan.items, &root);
        let mut passed = 0;
        let mut failed = 0;
        let mut skipped = 0;
        let mut failures: Vec<Value> = Vec::new();

        for r in &results {
            match &r.outcome {
                tend::checks::CheckOutcome::Skipped { .. } => {
                    skipped += 1;
                }
                tend::checks::CheckOutcome::Passed => {
                    passed += 1;
                }
                tend::checks::CheckOutcome::Failed { reason }
                | tend::checks::CheckOutcome::Errored { reason } => {
                    failed += 1;
                    failures.push(json!({
                        "check_id": r.task_id, "kind": r.kind,
                        "reason": reason,
                        "stdout_tail": r.stdout.chars().take(500).collect::<String>(),
                        "stderr_tail": r.stderr.chars().take(500).collect::<String>()
                    }));
                    if fail_fast {
                        break;
                    }
                }
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
    fn name(&self) -> &str {
        "tend.explain"
    }
    fn description(&self) -> &str {
        "Explain a check failure with repro command and likely causes"
    }
    fn metadata(&self) -> ToolMetadata {
        tool_meta(MutationLevel::ReadOnly)
    }
    fn input_schema(&self) -> Value {
        json!({
            "root": { "type": "string", "description": "Root directory" },
            "run_id": { "type": "string", "description": "Run ID from tend.run" },
            "check_id": { "type": "string", "description": "Optional check ID to filter" },
            "include_repro_command": { "type": "boolean" }
        })
    }
    fn call(&self, input: Value, ctx: &ToolContext) -> Result<Value, ToolFailure> {
        let audit_id = ctx.audit.generate_id();
        let check_id = input.get("check_id").and_then(|v| v.as_str());
        let root = input
            .get("root")
            .and_then(|v| v.as_str())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        let pairs = match walk_tend_configs(&root) {
            Ok(p) => p,
            Err(e) => {
                return Err(mk_err(
                    ErrorKind::NotFound,
                    &format!("Discovery: {}", e),
                    &audit_id,
                ))
            }
        };

        let nodes: Vec<_> = pairs.into_iter().map(|(_, r)| r).collect();
        let req = PlanRequest {
            phase: Phase::Verify,
            mode: RunMode::Force,
            group: None,
            target: None,
            files: Vec::new(),
        };
        let plan = match tend::planner::build_plan(&nodes, &req) {
            Ok(p) => p,
            Err(e) => {
                return Err(mk_err(
                    ErrorKind::Internal,
                    &format!("Plan: {}", e),
                    &audit_id,
                ))
            }
        };

        let items: Vec<_> = if let Some(cid) = check_id {
            plan.items
                .iter()
                .filter(|item| item.task_id == cid)
                .collect()
        } else {
            plan.items.iter().collect()
        };

        if items.is_empty() {
            return Err(mk_err(
                ErrorKind::NotFound,
                "No matching checks found",
                &audit_id,
            ));
        }

        let exec_results = tend::execute::execute_plan(
            &items.iter().map(|i| (*i).clone()).collect::<Vec<_>>(),
            &root,
        );

        let explanations: Vec<Value> = exec_results
            .iter()
            .filter(|r| r.outcome.is_failure())
            .map(|r| {
                let reason = match &r.outcome {
                    tend::checks::CheckOutcome::Failed { reason }
                    | tend::checks::CheckOutcome::Errored { reason } => reason.clone(),
                    _ => String::new(),
                };
                json!({
                    "check_id": r.task_id,
                    "failure_kind": "exit_code",
                    "explanation": reason,
                    "relevant_output": r.stderr.chars().take(1000).collect::<String>(),
                    "likely_causes": ["Check command failed with non-zero exit code"]
                })
            })
            .collect();

        let result = ToolResult::ok(
            json!({ "failures": explanations, "total_failures": explanations.len() }),
            format!("Found {} failure(s)", explanations.len()),
            &audit_id,
        );
        Ok(serde_json::to_value(&result).unwrap_or_default())
    }
}
