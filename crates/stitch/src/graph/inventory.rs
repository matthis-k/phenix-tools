use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::graph::{NodeKind, WorkspaceNode};

#[derive(Debug, Clone)]
pub struct InventoryOptions {
    pub root: PathBuf,
    pub include_root: bool,
    pub metadata_file: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceDiscovery {
    pub nodes: BTreeMap<String, WorkspaceNode>,
    pub metadata_path: Option<PathBuf>,
}

pub fn discover_inventory(root: &Path, metadata_path: Option<&Path>) -> Result<WorkspaceDiscovery, String> {
    let mut nodes = BTreeMap::new();

    // Parse .gitmodules
    let gitmodules_path = root.join(".gitmodules");
    if gitmodules_path.exists() {
        let content = std::fs::read_to_string(&gitmodules_path)
            .map_err(|e| format!("Read .gitmodules: {e}"))?;
        for entry in parse_gitmodules(&content) {
            let id = entry.name.clone();
            nodes.insert(
                id.clone(),
                WorkspaceNode {
                    id,
                    path: root.join(&entry.path),
                    repo_url: Some(entry.url),
                    kind: NodeKind::Unknown,
                    layer: None,
                    is_root: false,
                },
            );
        }
    }

    // Read metadata file for classification
    if let Some(meta_path) = metadata_path {
        let meta_full = if meta_path.is_relative() {
            root.join(meta_path)
        } else {
            meta_path.to_path_buf()
        };
        if meta_full.exists() {
            apply_metadata(&mut nodes, &meta_full)?;
        }
    }

    // Add root node
    nodes.insert(
        "phenix".to_string(),
        WorkspaceNode {
            id: "phenix".to_string(),
            path: root.to_path_buf(),
            repo_url: None,
            kind: NodeKind::WorkspaceRoot,
            layer: Some(999),
            is_root: true,
        },
    );

    Ok(WorkspaceDiscovery {
        nodes,
        metadata_path: metadata_path.map(|p| p.to_path_buf()),
    })
}

#[derive(Debug)]
struct GitmoduleEntry {
    name: String,
    path: String,
    url: String,
}

fn parse_gitmodules(content: &str) -> Vec<GitmoduleEntry> {
    let mut entries = Vec::new();
    let mut current: Option<GitmoduleEntry> = None;

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            if let Some(entry) = current.take() {
                entries.push(entry);
            }
            let inner = &line[1..line.len() - 1].trim();
            if let Some(name) = inner.strip_prefix("submodule \"") {
                if let Some(name) = name.strip_suffix('"') {
                    current = Some(GitmoduleEntry {
                        name: name.to_string(),
                        path: String::new(),
                        url: String::new(),
                    });
                }
            }
        } else if let Some(ref mut entry) = current {
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim();
                let value = value.trim();
                match key {
                    "path" => entry.path = value.to_string(),
                    "url" => entry.url = value.to_string(),
                    _ => {}
                }
            }
        }
    }

    if let Some(entry) = current {
        entries.push(entry);
    }

    entries
}

fn apply_metadata(
    nodes: &mut BTreeMap<String, WorkspaceNode>,
    meta_path: &Path,
) -> Result<(), String> {
    let content = std::fs::read_to_string(meta_path)
        .map_err(|e| format!("Read metadata {meta_path:?}: {e}"))?;
    let meta: MetadataFile = serde_json::from_str(&content)
        .map_err(|e| format!("Parse metadata {meta_path:?}: {e}"))?;

    for (node_id, meta_node) in &meta.nodes {
        if let Some(node) = nodes.get_mut(node_id) {
            node.kind = meta_node.kind();
            node.layer = Some(meta_node.layer);
            if meta_node.root.unwrap_or(false) {
                node.is_root = true;
            }
        } else {
            nodes.insert(
                node_id.clone(),
                WorkspaceNode {
                    id: node_id.clone(),
                    path: PathBuf::new(),
                    repo_url: None,
                    kind: meta_node.kind(),
                    layer: Some(meta_node.layer),
                    is_root: meta_node.root.unwrap_or(false),
                },
            );
        }
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct MetadataFile {
    version: u32,
    nodes: BTreeMap<String, MetadataNode>,
    rules: Option<MetadataRules>,
}

#[derive(Debug, Deserialize)]
struct MetadataNode {
    kind: String,
    layer: u32,
    root: Option<bool>,
}

impl MetadataNode {
    fn kind(&self) -> NodeKind {
        match self.kind.as_str() {
            "pins" => NodeKind::Pins,
            "packageProvider" => NodeKind::PackageProvider,
            "toolProvider" => NodeKind::ToolProvider,
            "shellProvider" => NodeKind::ShellProvider,
            "desktopProvider" => NodeKind::DesktopProvider,
            "hostConsumer" => NodeKind::HostConsumer,
            "workspaceRoot" => NodeKind::WorkspaceRoot,
            "external" => NodeKind::External,
            _ => NodeKind::Unknown,
        }
    }
}

#[derive(Debug, Deserialize)]
struct MetadataRules {
    #[allow(dead_code)]
    forbid_cycles: Option<bool>,
    #[allow(dead_code)]
    forbid_root_provider_role: Option<bool>,
    #[allow(dead_code)]
    forbid_provider_depends_on_consumer: Option<bool>,
    #[allow(dead_code)]
    require_root_ultimate_consumer: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_gitmodules() {
        let content = r#"
[submodule "phenix-tools"]
  path = flakes/02-producers/phenix-tools
  url = git@github.com:matthis-k/phenix-tools.git
[submodule "phenix-pins"]
  path = flakes/00-pins/phenix-pins
  url = git@github.com:matthis-k/phenix-pins.git
"#;
        let entries = parse_gitmodules(content);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "phenix-tools");
        assert_eq!(entries[0].path, "flakes/02-producers/phenix-tools");
        assert_eq!(entries[0].url, "git@github.com:matthis-k/phenix-tools.git");
        assert_eq!(entries[1].name, "phenix-pins");
    }

    #[test]
    fn test_discover_inventory_no_gitmodules() {
        let dir = std::env::temp_dir().join("test_inventory_no_gm");
        let _ = std::fs::create_dir_all(&dir);
        let result = discover_inventory(&dir, None).unwrap();
        // Should still have root node
        assert!(result.nodes.contains_key("phenix"));
        assert_eq!(result.nodes.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
