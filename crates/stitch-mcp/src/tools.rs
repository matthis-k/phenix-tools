use std::collections::{BTreeMap, HashMap};

use phenix_mcp_core::mcp::{McpTool, ToolContext};
use phenix_mcp_core::result::{ErrorKind, ToolFailure, ToolResult};
use phenix_mcp_core::types::{MutationLevel, ToolMetadata};
use serde_json::{json, Value};

use stitch::graph;
use stitch::sync;

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

pub struct StitchStatusTool;

impl McpTool for StitchStatusTool {
    fn name(&self) -> &str {
        "stitch.status"
    }
    fn description(&self) -> &str {
        "Show multi-repo git status across all configured repos"
    }
    fn metadata(&self) -> ToolMetadata {
        tool_meta(MutationLevel::ReadOnly)
    }
    fn input_schema(&self) -> Value {
        json!({
            "repos": { "type": "array", "items": {"type": "string"} },
            "include_untracked": { "type": "boolean" },
            "include_remote": { "type": "boolean" },
            "short": { "type": "boolean" },
            "dirty_only": { "type": "boolean" }
        })
    }
    fn call(&self, input: Value, ctx: &ToolContext) -> Result<Value, ToolFailure> {
        let audit_id = ctx.audit.generate_id();
        let repo_filter: Vec<String> = input
            .get("repos")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();
        let dirty_only = input
            .get("dirty_only")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let short = input
            .get("short")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let include_untracked = input
            .get("include_untracked")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let cfg = match stitch::config::find_and_load() {
            Ok(c) => c,
            Err(e) => {
                return Err(mk_err(
                    ErrorKind::NotFound,
                    &format!("Config: {}", e),
                    &audit_id,
                ))
            }
        };

        let statuses = stitch::status::collect_all(&cfg).unwrap_or_default();
        let mut repo_statuses: Vec<Value> = Vec::new();
        let mut short_lines: Vec<String> = Vec::new();
        let mut dirty_repos: Vec<String> = Vec::new();

        for s in &statuses {
            if !repo_filter.is_empty() && !repo_filter.contains(&s.name) {
                continue;
            }
            if dirty_only && !s.is_dirty {
                continue;
            }
            if s.is_dirty {
                dirty_repos.push(s.name.clone());
            }

            let changes = if include_untracked {
                let repo_cfg = cfg.repos.iter().find(|r| r.name == s.name);

                repo_cfg
                    .map(|r| {
                        let p = r.resolved_path(&cfg);
                        stitch::git::git_diff_names(&p).unwrap_or_default()
                    })
                    .unwrap_or_default()
            } else {
                vec![]
            };

            if short {
                for f in &changes {
                    short_lines.push(format!("M  {}", f));
                }
            }

            repo_statuses.push(json!({
                "name": s.name,
                "path": s.path,
                "branch": s.branch,
                "head": "",
                "dirty": s.is_dirty,
                "staged_count": s.staged_count,
                "unstaged_count": s.unstaged_count,
                "untracked_count": if include_untracked { s.untracked_count } else { 0 },
                "changes": changes.iter().map(|f| json!({"status": "modified", "path": f})).collect::<Vec<_>>()
            }));
        }

        let result = ToolResult::ok(
            json!({
                "workspace": cfg.workspace,
                "repos": repo_statuses,
                "dirty_repos": dirty_repos,
                "short": short_lines,
                "total": repo_statuses.len()
            }),
            format!("{} repos, {} dirty", repo_statuses.len(), dirty_repos.len()),
            &audit_id,
        );
        Ok(serde_json::to_value(&result).unwrap_or_default())
    }
}

pub struct StitchDiffTool;

impl McpTool for StitchDiffTool {
    fn name(&self) -> &str {
        "stitch.diff"
    }
    fn description(&self) -> &str {
        "Show diffs across repos"
    }
    fn metadata(&self) -> ToolMetadata {
        tool_meta(MutationLevel::ReadOnly)
    }
    fn input_schema(&self) -> Value {
        json!({
            "repo": { "type": "string", "description": "Repo name" },
            "staged": { "type": "boolean" },
            "json": { "type": "boolean" }
        })
    }
    fn call(&self, input: Value, ctx: &ToolContext) -> Result<Value, ToolFailure> {
        let audit_id = ctx.audit.generate_id();
        let repo_name = input.get("repo").and_then(|v| v.as_str()).unwrap_or("");
        let staged = input
            .get("staged")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let cfg = match stitch::config::find_and_load() {
            Ok(c) => c,
            Err(e) => {
                return Err(mk_err(
                    ErrorKind::NotFound,
                    &format!("Config: {}", e),
                    &audit_id,
                ))
            }
        };

        let target_repos: Vec<_> = if repo_name.is_empty() {
            cfg.repos.iter().collect()
        } else {
            let r = cfg.repos.iter().find(|r| r.name == repo_name);
            match r {
                Some(r) => vec![r],
                None => {
                    return Err(mk_err(
                        ErrorKind::NotFound,
                        &format!("Repo '{}' not found", repo_name),
                        &audit_id,
                    ))
                }
            }
        };

        let mut diffs: Vec<Value> = Vec::new();
        for repo in &target_repos {
            let path = repo.resolved_path(&cfg);
            if !path.join(".git").exists() {
                continue;
            }
            let mut args = vec!["diff"];
            if staged {
                args.push("--cached");
            }

            if let Ok(output) = std::process::Command::new("git")
                .args(&args)
                .current_dir(&path)
                .output()
            {
                if output.status.success() {
                    diffs.push(json!({
                        "repo": repo.name,
                        "diff": String::from_utf8_lossy(&output.stdout).to_string()
                    }));
                }
            }

            if !staged {
                let name_args = vec!["diff", "--name-only"];
                if let Ok(output) = std::process::Command::new("git")
                    .args(&name_args)
                    .current_dir(&path)
                    .output()
                {
                    if output.status.success() {
                        let files: Vec<String> = String::from_utf8_lossy(&output.stdout)
                            .lines()
                            .map(|l| l.trim().to_string())
                            .filter(|l| !l.is_empty())
                            .collect();
                        if let Some(d) = diffs.last_mut() {
                            d["files"] = json!(files);
                        }
                    }
                }
            }
        }

        let result = ToolResult::ok(
            json!({ "diffs": diffs, "total": diffs.len() }),
            format!("{} diff(s)", diffs.len()),
            &audit_id,
        );
        Ok(serde_json::to_value(&result).unwrap_or_default())
    }
}

pub struct StitchDagTool;

impl McpTool for StitchDagTool {
    fn name(&self) -> &str {
        "stitch.dag"
    }
    fn description(&self) -> &str {
        "Show ordered operation DAG for commit or sync (read-only)"
    }
    fn metadata(&self) -> ToolMetadata {
        tool_meta(MutationLevel::ReadOnly)
    }
    fn input_schema(&self) -> Value {
        json!({
            "mode": { "type": "string", "enum": ["commit", "sync", "full"] },
            "repos": { "type": "array", "items": {"type": "string"} },
            "staged": { "type": "boolean", "description": "Use staged files only" },
            "split": { "type": "string", "enum": ["by-repo", "by-path", "manual"] },
            "run_tend": { "type": "boolean" },
            "json": { "type": "boolean" }
        })
    }
    fn call(&self, input: Value, ctx: &ToolContext) -> Result<Value, ToolFailure> {
        let audit_id = ctx.audit.generate_id();
        let mode = input
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("commit");
        let split = input
            .get("split")
            .and_then(|v| v.as_str())
            .unwrap_or("by-repo");
        let staged = input
            .get("staged")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let run_tend = input
            .get("run_tend")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let repo_filter: Vec<String> = input
            .get("repos")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        let cfg = match stitch::config::find_and_load() {
            Ok(c) => c,
            Err(e) => {
                return Err(mk_err(
                    ErrorKind::NotFound,
                    &format!("Config: {}", e),
                    &audit_id,
                ))
            }
        };

        let statuses = stitch::status::collect_all(&cfg).unwrap_or_default();
        let mut nodes: Vec<Value> = Vec::new();

        for s in &statuses {
            if !repo_filter.is_empty() && !repo_filter.contains(&s.name) {
                continue;
            }
            if !s.is_dirty {
                continue;
            }

            let repo_cfg = match cfg.repos.iter().find(|r| r.name == s.name) {
                Some(r) => r,
                None => continue,
            };
            let repo_path = repo_cfg.resolved_path(&cfg);
            let diff = if staged {
                stitch::git::git_diff_cached_names(&repo_path).unwrap_or_default()
            } else {
                stitch::git::git_diff_names(&repo_path).unwrap_or_default()
            };

            if run_tend && mode != "sync" {
                nodes.push(json!({
                    "id": format!("{}:precheck", s.name),
                    "kind": "check",
                    "repo": s.name,
                    "command": ["tend", "run", "--changed"],
                    "depends_on": []
                }));
            }

            match split {
                "by-path" => {
                    let mut by_dir: HashMap<String, Vec<String>> = HashMap::new();
                    for f in &diff {
                        let dir = f
                            .rfind('/')
                            .map(|i| f[..i].to_string())
                            .unwrap_or_else(|| "root".to_string());
                        by_dir.entry(dir).or_default().push(f.clone());
                    }
                    for (dir, files) in &by_dir {
                        let deps = if run_tend && mode != "sync" {
                            vec![format!("{}:precheck", s.name)]
                        } else {
                            vec![]
                        };
                        nodes.push(json!({
                            "id": format!("{}:{}", s.name, dir.replace('/', "_")),
                            "kind": "commit",
                            "repo": s.name,
                            "files": files,
                            "depends_on": deps
                        }));
                    }
                }
                _ => {
                    let deps = if run_tend && mode != "sync" {
                        vec![format!("{}:precheck", s.name)]
                    } else {
                        vec![]
                    };
                    nodes.push(json!({
                        "id": format!("{}:commit", s.name),
                        "kind": "commit",
                        "repo": s.name,
                        "files": diff,
                        "depends_on": deps
                    }));
                }
            }
        }

        if mode == "full" || mode == "sync" {
            let commit_ids: Vec<String> = nodes
                .iter()
                .filter(|n| n["kind"] == "commit")
                .filter_map(|n| n["id"].as_str().map(|s| s.to_string()))
                .collect();

            if !commit_ids.is_empty() {
                let root_repo = cfg
                    .repos
                    .iter()
                    .find(|r| r.name.contains("root") || r.name == "phenix");
                if let Some(root) = root_repo {
                    nodes.push(json!({
                        "id": format!("{}:update-pins", root.name),
                        "kind": "update-pins",
                        "repo": root.name,
                        "files": ["flake.lock"],
                        "depends_on": commit_ids
                    }));
                }
            }
        }

        let result = ToolResult::ok(
            json!({ "nodes": nodes, "total": nodes.len(), "mode": mode }),
            format!("DAG: {} node(s) in {} mode", nodes.len(), mode),
            &audit_id,
        );
        Ok(serde_json::to_value(&result).unwrap_or_default())
    }
}

pub struct StitchCommitTemplateTool;

impl McpTool for StitchCommitTemplateTool {
    fn name(&self) -> &str {
        "stitch.commit_template"
    }
    fn description(&self) -> &str {
        "Generate a JSON message template for sync commit nodes (read-only)"
    }
    fn metadata(&self) -> ToolMetadata {
        tool_meta(MutationLevel::ReadOnly)
    }
    fn input_schema(&self) -> Value {
        json!({
            "dag_id": { "type": "string", "description": "DAG ID from stitch.dag" },
            "repos": { "type": "array", "items": {"type": "string"} },
            "staged": { "type": "boolean", "description": "Use staged files only" }
        })
    }
    fn call(&self, input: Value, ctx: &ToolContext) -> Result<Value, ToolFailure> {
        let audit_id = ctx.audit.generate_id();
        let _dag_id = input.get("dag_id").and_then(|v| v.as_str());
        let staged = input
            .get("staged")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let cfg = stitch::config::find_and_load().unwrap_or(stitch::model::WorkspaceConfig {
            version: 1,
            workspace: ".".to_string(),
            repos: vec![],
            config_dir: None,
        });
        let statuses = stitch::status::collect_all(&cfg).unwrap_or_default();

        let mut messages = serde_json::Map::new();
        for s in &statuses {
            if !s.is_dirty {
                continue;
            }
            let repo_cfg = match cfg.repos.iter().find(|r| r.name == s.name) {
                Some(r) => r,
                None => continue,
            };
            let repo_path = repo_cfg.resolved_path(&cfg);
            let diff = if staged {
                stitch::git::git_diff_cached_names(&repo_path).unwrap_or_default()
            } else {
                stitch::git::git_diff_names(&repo_path).unwrap_or_default()
            };

            messages.insert(
                format!("{}:commit", s.name),
                json!({ "subject": "", "body": "", "files": diff }),
            );
        }

        let result = ToolResult::ok(
            json!({ "messages": messages, "total": messages.len() }),
            format!("{} commit node(s) need messages", messages.len()),
            &audit_id,
        );
        Ok(serde_json::to_value(&result).unwrap_or_default())
    }
}

pub struct StitchCommitTool;

impl McpTool for StitchCommitTool {
    fn name(&self) -> &str {
        "stitch.commit"
    }
    fn description(&self) -> &str {
        "DAG-wide sync commit: commit changed nodes, update dependent flake inputs, validate, and push in dependency order. Requires apply: true"
    }
    fn metadata(&self) -> ToolMetadata {
        tool_meta(MutationLevel::CreatesCommit)
    }
    fn input_schema(&self) -> Value {
        json!({
            "apply": { "type": "boolean", "description": "Must be true to execute" },
            "dry_run": { "type": "boolean", "description": "Plan mode (no mutations)" },
            "no_push": { "type": "boolean", "description": "Commit locally without pushing" },
            "force": { "type": "boolean", "description": "Allow edge cases like detached HEAD" },
            "messages": {
                "type": "object",
                "description": "Keyed by node name, each with subject, body, files",
                "additionalProperties": {
                    "type": "object",
                    "properties": {
                        "subject": { "type": "string" },
                        "body": { "type": "string" },
                        "files": { "type": "array", "items": { "type": "string" } }
                    }
                }
            },
            "resume": { "type": "string", "description": "Transaction ID to resume" }
        })
    }
    fn call(&self, input: Value, ctx: &ToolContext) -> Result<Value, ToolFailure> {
        let audit_id = ctx.audit.generate_id();
        let apply = input
            .get("apply")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let dry_run = input
            .get("dry_run")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let no_push = input
            .get("no_push")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let force = input
            .get("force")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let resume_id = input.get("resume").and_then(|v| v.as_str());

        if !apply && !dry_run {
            return Err(mk_err(
                ErrorKind::PolicyDenied,
                "Must set apply=true to execute, or use dry_run=true for plan-only",
                &audit_id,
            ));
        }

        let cfg = match stitch::config::find_and_load() {
            Ok(c) => c,
            Err(e) => {
                return Err(mk_err(
                    ErrorKind::NotFound,
                    &format!("Config: {}", e),
                    &audit_id,
                ))
            }
        };

        // Resume mode
        if let Some(tx_id) = resume_id {
            let result = match sync::resume_sync(tx_id, &cfg, no_push) {
                Ok(r) => r,
                Err(e) => {
                    return Err(mk_err(
                        ErrorKind::NotFound,
                        &format!("Resume failed: {}", e),
                        &audit_id,
                    ))
                }
            };
            let out = ToolResult::ok(
                json!({
                    "transaction_id": result.transaction_id,
                    "phase": result.phase.to_string(),
                    "created_commits": result.created_commits,
                    "push_results": result.push_results.iter().map(|(name, r)| {
                        json!({"node": name, "success": r.is_ok(), "error": r.as_ref().err()})
                    }).collect::<Vec<_>>()
                }),
                format!("Resumed transaction: {}", result.transaction_id),
                &audit_id,
            );
            return Ok(serde_json::to_value(&out).unwrap_or_default());
        }

        match graph::discover_graph(&cfg) {
            Ok(dag) => {
                let statuses = stitch::status::collect_all(&cfg).unwrap_or_default();
                match sync::plan_sync(&dag, &statuses, &cfg) {
                    Ok(plan) => {
                        if dry_run {
                            let action_list: Vec<serde_json::Value> = plan
                                .actions
                                .iter()
                                .map(|a| match a {
                                    sync::Action::Commit { node, .. } => {
                                        json!({"type": "commit", "node": node})
                                    }
                                    sync::Action::UpdateInputs { node, .. } => {
                                        json!({"type": "update-inputs", "node": node})
                                    }
                                    sync::Action::Validate { node } => {
                                        json!({"type": "validate", "node": node})
                                    }
                                    sync::Action::Push { node } => {
                                        json!({"type": "push", "node": node})
                                    }
                                })
                                .collect();
                            let out = ToolResult::ok(
                                json!({
                                    "transaction_id": plan.transaction_id,
                                    "root": plan.root,
                                    "actions": action_list,
                                    "nodes": plan.node_plans.iter().map(|(id, np)| {
                                        json!({
                                            "name": id,
                                            "dirty": np.dirty,
                                            "commit_required": np.needs_code_commit,
                                            "sync_update_required": np.needs_input_sync,
                                            "message": np.message
                                        })
                                    }).collect::<Vec<_>>(),
                                    "blocked_reasons": plan.blocked_reasons
                                }),
                                format!("Sync plan: {} action(s) to process", plan.actions.len()),
                                &audit_id,
                            );
                            return Ok(serde_json::to_value(&out).unwrap_or_default());
                        }

                        if !plan.blocked_reasons.is_empty() && !force {
                            return Err(mk_err(
                                ErrorKind::Conflict,
                                &format!(
                                    "Sync blocked: {}. Use force=true to override",
                                    plan.blocked_reasons.join("; ")
                                ),
                                &audit_id,
                            ));
                        }

                        // Load messages
                        let messages: Option<BTreeMap<String, String>> = input
                            .get("messages")
                            .and_then(|v| v.as_object())
                            .map(|obj| {
                                obj.iter()
                                    .map(|(k, v)| {
                                        let msg =
                                            v.get("subject").and_then(|s| s.as_str()).unwrap_or(k);
                                        (k.clone(), msg.to_string())
                                    })
                                    .collect()
                            });

                        match sync::execute_sync(
                            &plan,
                            &dag,
                            &cfg,
                            no_push,
                            messages.as_ref(),
                            force,
                        ) {
                            Ok(result) => {
                                let out = ToolResult::ok(
                                    json!({
                                        "transaction_id": result.transaction_id,
                                        "phase": result.phase.to_string(),
                                        "created_commits": result.created_commits,
                                        "push_results": result.push_results.iter().map(|(name, r)| {
                                            json!({"node": name, "success": r.is_ok(), "error": r.as_ref().err()})
                                        }).collect::<Vec<_>>()
                                    }),
                                    format!(
                                        "Sync commit {}: {} node(s) committed",
                                        if result.phase == sync::JournalPhase::Completed {
                                            "completed"
                                        } else {
                                            "partial"
                                        },
                                        result.created_commits.len()
                                    ),
                                    &audit_id,
                                );
                                Ok(serde_json::to_value(&out).unwrap_or_default())
                            }
                            Err(e) => Err(mk_err(
                                ErrorKind::Internal,
                                &format!("Sync execution failed: {}", e),
                                &audit_id,
                            )),
                        }
                    }
                    Err(e) => Err(mk_err(
                        ErrorKind::Internal,
                        &format!("Plan failed: {}", e),
                        &audit_id,
                    )),
                }
            }
            Err(e) => Err(mk_err(
                ErrorKind::NotFound,
                &format!("Graph discovery: {}", e),
                &audit_id,
            )),
        }
    }
}

pub struct StitchSyncTool;

impl McpTool for StitchSyncTool {
    fn name(&self) -> &str {
        "stitch.sync"
    }
    fn description(&self) -> &str {
        "Pull/rebase/update-pins/push across repos. Requires apply: true"
    }
    fn metadata(&self) -> ToolMetadata {
        tool_meta(MutationLevel::Network)
    }
    fn input_schema(&self) -> Value {
        json!({
            "apply": { "type": "boolean", "description": "Must be true to execute" },
            "repos": { "type": "array", "items": {"type": "string"} },
            "mode": { "type": "string", "enum": ["pull", "push", "full"] },
            "run_tend": { "type": "boolean" }
        })
    }
    fn call(&self, input: Value, ctx: &ToolContext) -> Result<Value, ToolFailure> {
        let audit_id = ctx.audit.generate_id();
        let apply = input
            .get("apply")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if !apply {
            return Err(mk_err(
                ErrorKind::PolicyDenied,
                "Must set apply=true for sync operations",
                &audit_id,
            ));
        }

        let _mode = input.get("mode").and_then(|v| v.as_str()).unwrap_or("pull");
        let _cfg = match stitch::config::find_and_load() {
            Ok(c) => c,
            Err(e) => {
                return Err(mk_err(
                    ErrorKind::NotFound,
                    &format!("Config: {}", e),
                    &audit_id,
                ))
            }
        };

        let result = ToolResult::ok(
            json!({ "completed": [], "failed": [] }),
            "Use stitch.commit for DAG sync commit operations. This tool is for pull/rebase flows.",
            &audit_id,
        );
        Ok(serde_json::to_value(&result).unwrap_or_default())
    }
}
