use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct SyncConfig {
    #[serde(rename = "dependsOn")]
    pub depends_on: Vec<String>,
    #[serde(rename = "updateInputs")]
    pub update_inputs: Vec<String>,
    pub checks: Vec<String>,
}
