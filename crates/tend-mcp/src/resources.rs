use phenix_mcp_core::mcp::{McpResource, ToolContext};
use phenix_mcp_core::result::ToolFailure;
use serde_json::{json, Value};

pub struct TendConfigsResource;

impl McpResource for TendConfigsResource {
    fn uri(&self) -> &str {
        "tend://configs"
    }
    fn name(&self) -> &str {
        "Tend Configs"
    }
    fn description(&self) -> &str {
        "List all discovered .tend.json configs"
    }
    fn mime_type(&self) -> &str {
        "application/json"
    }
    fn read(&self, _ctx: &ToolContext) -> Result<Value, ToolFailure> {
        let root = std::env::current_dir().unwrap_or_default();
        match tend::discover::discover_configs(&root, None) {
            Ok(nodes) => {
                let configs: Vec<Value> = nodes
                    .iter()
                    .map(|n| {
                        json!({
                            "config_path": n.config_path.to_string_lossy(),
                            "node_path": n.node_path.to_string_lossy(),
                            "id": n.node_config.id,
                            "description": n.node_config.description
                        })
                    })
                    .collect();
                Ok(json!({ "configs": configs, "total": configs.len() }))
            }
            Err(e) => Ok(json!({ "error": e.to_string(), "configs": [] })),
        }
    }
}

pub struct TendRunsResource;

impl McpResource for TendRunsResource {
    fn uri(&self) -> &str {
        "tend://runs"
    }
    fn name(&self) -> &str {
        "Tend Runs"
    }
    fn description(&self) -> &str {
        "Recent tend run summaries"
    }
    fn mime_type(&self) -> &str {
        "application/json"
    }
    fn read(&self, _ctx: &ToolContext) -> Result<Value, ToolFailure> {
        Ok(json!({ "runs": [], "total": 0 }))
    }
}
