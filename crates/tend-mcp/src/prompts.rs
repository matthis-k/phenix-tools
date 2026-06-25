use std::collections::HashMap;

use phenix_mcp_core::mcp::{McpPrompt, PromptArgument, ToolContext};
use phenix_mcp_core::result::ToolFailure;
use serde_json::{json, Value};

pub struct TendDebugFailingCheck;

impl McpPrompt for TendDebugFailingCheck {
    fn name(&self) -> &str { "tend/debug-failing-check" }
    fn description(&self) -> &str { "Debug a failing tend check with step-by-step guidance" }
    fn arguments(&self) -> Vec<PromptArgument> {
        vec![
            PromptArgument { name: "run_id".to_string(), description: "Run ID from tend.run".to_string(), required: true },
            PromptArgument { name: "check_id".to_string(), description: "Check ID to debug".to_string(), required: true },
        ]
    }
    fn get(&self, _args: HashMap<String, Value>, _ctx: &ToolContext) -> Result<Value, ToolFailure> {
        Ok(json!({"messages": [{"role": "user", "content": "I need to debug a failing tend check."}]}))
    }
}

pub struct TendExplainRepoGatePolicy;

impl McpPrompt for TendExplainRepoGatePolicy {
    fn name(&self) -> &str { "tend/explain-repo-gate-policy" }
    fn description(&self) -> &str { "Explain how the repo gate policy works" }
    fn arguments(&self) -> Vec<PromptArgument> { vec![] }
    fn get(&self, _args: HashMap<String, Value>, _ctx: &ToolContext) -> Result<Value, ToolFailure> {
        Ok(json!({"messages": [{"role": "user", "content": "Explain the tend gate policy."}]}))
    }
}
