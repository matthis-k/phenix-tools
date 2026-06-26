mod prompts;
mod resources;
mod tools;

use std::path::PathBuf;

use phenix_mcp_core::audit::AuditSink;
use phenix_mcp_core::mcp::{McpServer, ToolContext};
use phenix_mcp_core::roots::{McpRoot, RootValidator};
use phenix_mcp_core::runner::CommandRunner;
use phenix_mcp_core::safety::SafetyPolicy;

fn main() {
    let audit_dir = PathBuf::from(
        std::env::var("HOME").unwrap_or_else(|_| ".".to_string()),
    )
    .join(".local")
    .join("share")
    .join("phenix")
    .join("audit")
    .join("tend-mcp");

    let cwd = std::env::current_dir().unwrap_or_default();
    let roots = vec![McpRoot::new(cwd.clone(), false)];
    let validator = RootValidator::new(roots);

    let context = ToolContext {
        roots: validator,
        runner: CommandRunner::new(),
        audit: AuditSink::new(Some(audit_dir)),
        safety: SafetyPolicy::default(),
        server_name: "tend-mcp".to_string(),
        server_version: "0.1.0".to_string(),
    };

    let mut server = McpServer::new(context);

    server.add_tool(Box::new(tools::TendStatusTool));
    server.add_tool(Box::new(tools::TendPlanTool));
    server.add_tool(Box::new(tools::TendRunTool));
    server.add_tool(Box::new(tools::TendExplainTool));

    server.add_resource(Box::new(resources::TendConfigsResource));
    server.add_resource(Box::new(resources::TendRunsResource));

    server.add_prompt(Box::new(prompts::TendDebugFailingCheck));
    server.add_prompt(Box::new(prompts::TendExplainRepoGatePolicy));

    server.run();
}
