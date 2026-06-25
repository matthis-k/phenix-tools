use std::collections::HashMap;

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

pub struct StitchStatusTool;

impl McpTool for StitchStatusTool {
    fn name(&self) -> &str { "stitch.status" }
    fn description(&self) -> &str { "Show multi-repo git status across all configured repos" }
    fn metadata(&self) -> ToolMetadata { tool_meta(MutationLevel::ReadOnly) }
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
        let repo_filter: Vec<String> = input.get("repos")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str()).map(|s| s.to_string()).collect())
            .unwrap_or_default();
        let dirty_only = input.get("dirty_only").and_then(|v| v.as_bool()).unwrap_or(false);
        let short = input.get("short").and_then(|v| v.as_bool()).unwrap_or(false);
        let include_untracked = input.get("include_untracked").and_then(|v| v.as_bool()).unwrap_or(true);

        let cfg = match stitch::config::find_and_load() {
            Ok(c) => c,
            Err(e) => return Err(mk_err(ErrorKind::NotFound, &format!("Config: {}", e), &audit_id)),
        };

        let statuses = stitch::status::collect_all(&cfg).unwrap_or_default();
        let mut repo_statuses: Vec<Value> = Vec::new();
        let mut short_lines: Vec<String> = Vec::new();
        let mut dirty_repos: Vec<String> = Vec::new();

        for s in &statuses {
            if !repo_filter.is_empty() && !repo_filter.contains(&s.name) { continue; }
            if dirty_only && !s.is_dirty { continue; }
            if s.is_dirty { dirty_repos.push(s.name.clone()); }

            let changes = if include_untracked {
                let repo_cfg = cfg.repos.iter().find(|r| r.name == s.name);
                let diff = repo_cfg.map(|r| {
                    let p = r.resolved_path(&cfg);
                    stitch::git::git_diff_names(&p).unwrap_or_default()
                }).unwrap_or_default();
                diff
            } else { vec![] };

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
    fn name(&self) -> &str { "stitch.diff" }
    fn description(&self) -> &str { "Show diffs across repos" }
    fn metadata(&self) -> ToolMetadata { tool_meta(MutationLevel::ReadOnly) }
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
        let staged = input.get("staged").and_then(|v| v.as_bool()).unwrap_or(false);

        let cfg = match stitch::config::find_and_load() {
            Ok(c) => c,
            Err(e) => return Err(mk_err(ErrorKind::NotFound, &format!("Config: {}", e), &audit_id)),
        };

        let target_repos: Vec<_> = if repo_name.is_empty() {
            cfg.repos.iter().collect()
        } else {
            let r = cfg.repos.iter().find(|r| r.name == repo_name);
            match r { Some(r) => vec![r], None => return Err(mk_err(ErrorKind::NotFound, &format!("Repo '{}' not found", repo_name), &audit_id)) }
        };

        let mut diffs: Vec<Value> = Vec::new();
        for repo in &target_repos {
            let path = repo.resolved_path(&cfg);
            if !path.join(".git").exists() { continue; }
            let mut args = vec!["diff"];
            if staged { args.push("--cached"); }

            if let Ok(output) = std::process::Command::new("git").args(&args).current_dir(&path).output() {
                if output.status.success() {
                    diffs.push(json!({
                        "repo": repo.name,
                        "diff": String::from_utf8_lossy(&output.stdout).to_string()
                    }));
                }
            }

            if !staged {
                let name_args = vec!["diff", "--name-only"];
                if let Ok(output) = std::process::Command::new("git").args(&name_args).current_dir(&path).output() {
                    if output.status.success() {
                        let files: Vec<String> = String::from_utf8_lossy(&output.stdout).lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
                        if let Some(d) = diffs.last_mut() {
                            d["files"] = json!(files);
                        }
                    }
                }
            }
        }

        let result = ToolResult::ok(
            json!({ "diffs": diffs, "total": diffs.len() }),
            format!("{} diff(s)", diffs.len()), &audit_id,
        );
        Ok(serde_json::to_value(&result).unwrap_or_default())
    }
}

pub struct StitchDagTool;

impl McpTool for StitchDagTool {
    fn name(&self) -> &str { "stitch.dag" }
    fn description(&self) -> &str { "Show ordered operation DAG for commit or sync (read-only)" }
    fn metadata(&self) -> ToolMetadata { tool_meta(MutationLevel::ReadOnly) }
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
        let mode = input.get("mode").and_then(|v| v.as_str()).unwrap_or("commit");
        let split = input.get("split").and_then(|v| v.as_str()).unwrap_or("by-repo");
        let staged = input.get("staged").and_then(|v| v.as_bool()).unwrap_or(false);
        let run_tend = input.get("run_tend").and_then(|v| v.as_bool()).unwrap_or(true);
        let repo_filter: Vec<String> = input.get("repos")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str()).map(|s| s.to_string()).collect())
            .unwrap_or_default();

        let cfg = match stitch::config::find_and_load() {
            Ok(c) => c,
            Err(e) => return Err(mk_err(ErrorKind::NotFound, &format!("Config: {}", e), &audit_id)),
        };

        let statuses = stitch::status::collect_all(&cfg).unwrap_or_default();
        let mut nodes: Vec<Value> = Vec::new();

        for s in &statuses {
            if !repo_filter.is_empty() && !repo_filter.contains(&s.name) { continue; }
            if !s.is_dirty { continue; }

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
                        let dir = f.rfind('/').map(|i| f[..i].to_string()).unwrap_or_else(|| "root".to_string());
                        by_dir.entry(dir).or_default().push(f.clone());
                    }
                    for (dir, files) in &by_dir {
                        let deps = if run_tend && mode != "sync" { vec![format!("{}:precheck", s.name)] } else { vec![] };
                        nodes.push(json!({
                            "id": format!("{}:{}", s.name, dir.replace('/', "_")),
                            "kind": "commit",
                            "repo": s.name,
                            "files": files,
                            "depends_on": deps
                        }));
                    }
                },
                _ => {
                    let deps = if run_tend && mode != "sync" { vec![format!("{}:precheck", s.name)] } else { vec![] };
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
            let commit_ids: Vec<String> = nodes.iter()
                .filter(|n| n["kind"] == "commit")
                .filter_map(|n| n["id"].as_str().map(|s| s.to_string()))
                .collect();

            if !commit_ids.is_empty() {
                let root_repo = cfg.repos.iter().find(|r| r.name.contains("root") || r.name == "phenix");
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
    fn name(&self) -> &str { "stitch.commit_template" }
    fn description(&self) -> &str { "Generate a JSON message template for all commit nodes (read-only)" }
    fn metadata(&self) -> ToolMetadata { tool_meta(MutationLevel::ReadOnly) }
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
        let staged = input.get("staged").and_then(|v| v.as_bool()).unwrap_or(false);

        let cfg = stitch::config::find_and_load().unwrap_or(
            stitch::model::WorkspaceConfig { version: 1, workspace: ".".to_string(), repos: vec![] }
        );
        let statuses = stitch::status::collect_all(&cfg).unwrap_or_default();

        let mut messages = serde_json::Map::new();
        for s in &statuses {
            if !s.is_dirty { continue; }
            let repo_cfg = match cfg.repos.iter().find(|r| r.name == s.name) { Some(r) => r, None => continue };
            let repo_path = repo_cfg.resolved_path(&cfg);
            let diff = if staged {
                stitch::git::git_diff_cached_names(&repo_path).unwrap_or_default()
            } else {
                stitch::git::git_diff_names(&repo_path).unwrap_or_default()
            };

            messages.insert(
                format!("{}:commit", s.name),
                json!({ "subject": "", "body": "", "files": diff })
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
    fn name(&self) -> &str { "stitch.commit" }
    fn description(&self) -> &str { "Create exact-file commits across repos. Requires apply: true" }
    fn metadata(&self) -> ToolMetadata { tool_meta(MutationLevel::CreatesCommit) }
    fn input_schema(&self) -> Value {
        json!({
            "apply": { "type": "boolean", "description": "Must be true to execute" },
            "staged": { "type": "boolean", "description": "Only commit previously staged files" },
            "dag_id": { "type": "string" },
            "messages": {
                "type": "object",
                "description": "Keyed by node ID, each with subject, body, files",
                "additionalProperties": {
                    "type": "object",
                    "properties": {
                        "subject": { "type": "string" },
                        "body": { "type": "string" },
                        "files": { "type": "array", "items": { "type": "string" } }
                    }
                }
            },
            "repo": { "type": "string" },
            "message": { "type": "string" },
            "run_tend": { "type": "boolean" },
            "dry_run": { "type": "boolean" }
        })
    }
    fn call(&self, input: Value, ctx: &ToolContext) -> Result<Value, ToolFailure> {
        let audit_id = ctx.audit.generate_id();
        let apply = input.get("apply").and_then(|v| v.as_bool()).unwrap_or(false);
        let dry_run = input.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(false);
        let staged = input.get("staged").and_then(|v| v.as_bool()).unwrap_or(false);
        let run_tend = input.get("run_tend").and_then(|v| v.as_bool()).unwrap_or(true);

        if !apply && !dry_run {
            return Err(mk_err(ErrorKind::PolicyDenied, "Must set apply=true to execute, or use dry_run=true", &audit_id));
        }

        let messages = input.get("messages").and_then(|v| v.as_object());
        let single_message = input.get("message").and_then(|v| v.as_str());
        let single_repo = input.get("repo").and_then(|v| v.as_str());

        let cfg = match stitch::config::find_and_load() {
            Ok(c) => c,
            Err(e) => return Err(mk_err(ErrorKind::NotFound, &format!("Config: {}", e), &audit_id)),
        };

        let statuses = stitch::status::collect_all(&cfg).unwrap_or_default();

        if run_tend && apply {
            let root = std::env::current_dir().unwrap_or_default();
            if let Ok(discovered) = tend::discover::discover_configs(&root, None) {
                let nodes = tend::discover::resolve_nodes(&root, discovered);
                if let Ok(plan) = tend::planner::build_plan(&nodes, "verify", "changed", None) {
                    let results = tend::execute::execute_plan(&plan.items, &root);
                    let failures: Vec<_> = results.iter().filter(|r| !r.passed && !r.skipped).collect();
                    if !failures.is_empty() {
                        return Err(ToolFailure::new(ErrorKind::Conflict,
                            format!("Tend gate blocked commit: {} check(s) failed", failures.len()), &audit_id));
                    }
                }
            }
        }

        let mut created: Vec<Value> = Vec::new();
        let mut failed: Vec<Value> = Vec::new();

        for s in &statuses {
            if !s.is_dirty { continue; }
            if let Some(repo) = single_repo { if s.name != repo { continue; } }

            let repo_cfg = match cfg.repos.iter().find(|r| r.name == s.name) { Some(r) => r, None => continue };
            let repo_path = repo_cfg.resolved_path(&cfg);
            let diff = if staged {
                stitch::git::git_diff_cached_names(&repo_path).unwrap_or_default()
            } else {
                stitch::git::git_diff_names(&repo_path).unwrap_or_default()
            };

            let node_key = format!("{}:commit", s.name);
            let mut files_to_stage = diff.clone();

            if let Some(msgs) = messages {
                if let Some(node_msg) = msgs.get(&node_key) {
                    if let Some(files) = node_msg.get("files").and_then(|v| v.as_array()) {
                        files_to_stage = files.iter().filter_map(|v| v.as_str()).map(|s| s.to_string()).collect();
                    }
                }
            }

            if files_to_stage.is_empty() { continue; }

            if dry_run {
                created.push(json!({
                    "repo": s.name,
                    "node": node_key,
                    "would_stage": files_to_stage,
                    "would_message": single_message.unwrap_or(&format!("Apply changes to {}", s.name))
                }));
                continue;
            }

            let msg = if let Some(msgs) = messages {
                if let Some(node_msg) = msgs.get(&node_key) {
                    node_msg.get("subject").and_then(|v| v.as_str()).unwrap_or(&format!("Apply changes to {}", s.name)).to_string()
                } else { format!("Apply changes to {}", s.name) }
            } else if let Some(m) = single_message {
                m.to_string()
            } else {
                format!("Apply changes to {}", s.name)
            };

            if let Err(e) = stitch::git::git_add(&repo_path, &files_to_stage) {
                failed.push(json!({"repo": s.name, "error": e}));
                continue;
            }

            let trailed = stitch::model::add_trailers(&msg, &audit_id[..8], &cfg.workspace);
            match stitch::git::git_commit(&repo_path, &trailed) {
                Ok(()) => {
                    let hash = stitch::git::git_short_head(&repo_path).ok();
                    created.push(json!({"repo": s.name, "hash": hash, "message": msg, "files": files_to_stage}));
                },
                Err(e) => { failed.push(json!({"repo": s.name, "error": e})); }
            }
        }

        let result = ToolResult::ok(
            json!({ "created_commits": created, "failed": failed }),
            format!("{} commit(s) created, {} failed", created.len(), failed.len()),
            &audit_id,
        );
        Ok(serde_json::to_value(&result).unwrap_or_default())
    }
}

pub struct StitchSyncTool;

impl McpTool for StitchSyncTool {
    fn name(&self) -> &str { "stitch.sync" }
    fn description(&self) -> &str { "Pull/rebase/update-pins/push across repos. Requires apply: true" }
    fn metadata(&self) -> ToolMetadata { tool_meta(MutationLevel::Network) }
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
        let apply = input.get("apply").and_then(|v| v.as_bool()).unwrap_or(false);

        if !apply {
            return Err(mk_err(ErrorKind::PolicyDenied, "Must set apply=true for sync operations", &audit_id));
        }

        let _mode = input.get("mode").and_then(|v| v.as_str()).unwrap_or("pull");
        let _cfg = match stitch::config::find_and_load() {
            Ok(c) => c,
            Err(e) => return Err(mk_err(ErrorKind::NotFound, &format!("Config: {}", e), &audit_id)),
        };

        let result = ToolResult::ok(
            json!({ "completed": [], "failed": [] }),
            "Sync operations require multi-repo remote coordination — implement per-command flow for pull/push",
            &audit_id,
        );
        Ok(serde_json::to_value(&result).unwrap_or_default())
    }
}
