use std::collections::HashSet;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::config;
use crate::config::ConfigError;
use crate::model::{NodeConfig, ResolvedNode, TendConfig};

const IGNORED_DIRS: &[&str] = &[
    ".git",
    ".direnv",
    "result",
    "node_modules",
    "vendor",
    "target",
    "dist",
    "build",
    ".cache",
    ".nix",
];

#[derive(Debug)]
pub enum DiscoverError {
    Io(std::io::Error),
    Config(ConfigError),
    Serde(String),
    ConfigNotFound,
    DuplicateNodePath(PathBuf),
    NoConfigsFound,
}

impl std::fmt::Display for DiscoverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO error: {e}"),
            Self::Config(e) => write!(f, "config error: {e}"),
            Self::Serde(e) => write!(f, "JSON error: {e}"),
            Self::ConfigNotFound => write!(f, "config file not found"),
            Self::DuplicateNodePath(p) => {
                write!(f, "duplicate node path: {}", p.display())
            }
            Self::NoConfigsFound => write!(f, "no .tend.json files found"),
        }
    }
}

impl std::error::Error for DiscoverError {}

impl From<std::io::Error> for DiscoverError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<ConfigError> for DiscoverError {
    fn from(e: ConfigError) -> Self {
        Self::Config(e)
    }
}

fn is_ignored_dir(name: &str) -> bool {
    IGNORED_DIRS.contains(&name) || name.starts_with("result-")
}

pub struct DiscoveredNode {
    pub config_path: PathBuf,
    pub node_path: PathBuf,
    pub node_config: NodeConfig,
}

pub fn discover_configs(
    root: &Path,
    explicit: Option<&[PathBuf]>,
) -> Result<Vec<DiscoveredNode>, DiscoverError> {
    let root = root
        .canonicalize()
        .map_err(|e| DiscoverError::Io(e))?;
    let mut config_files = Vec::new();

    if let Some(paths) = explicit {
        for p in paths {
            let canonical = if p.is_relative() {
                root.join(p)
            } else {
                p.to_path_buf()
            };
            if !canonical.exists() {
                return Err(DiscoverError::ConfigNotFound);
            }
            let canonical = canonical
                .canonicalize()
                .map_err(|e| DiscoverError::Io(e))?;
            config_files.push(canonical);
        }
    } else {
        for entry in WalkDir::new(&root)
            .into_iter()
            .filter_entry(|e| {
                if e.depth() == 0 {
                    return true;
                }
                let name = e.file_name().to_str().unwrap_or("");
                if e.file_type().is_dir() {
                    !is_ignored_dir(name)
                } else {
                    true
                }
            })
        {
            let entry = entry.map_err(|e| DiscoverError::Io(e.into()))?;
            if entry.file_type().is_file() && entry.file_name() == ".tend.json" {
                config_files.push(entry.path().to_path_buf());
            }
        }
    }

    if config_files.is_empty() {
        return Err(DiscoverError::NoConfigsFound);
    }

    config_files.sort();

    let mut nodes = Vec::new();
    let mut seen_paths = HashSet::new();

    for path in &config_files {
        let content =
            std::fs::read_to_string(path).map_err(|e| DiscoverError::Io(e))?;

        let parsed: TendConfig =
            serde_json::from_str(&content).map_err(|e| {
                DiscoverError::Serde(format!("{}: {}", path.display(), e))
            })?;

        if parsed.version != 1 {
            return Err(DiscoverError::Config(ConfigError::InvalidVersion(
                parsed.version,
            )));
        }

        let node_dir = path.parent().unwrap_or(&root);
        let rel = node_dir
            .strip_prefix(&root)
            .map(|p| if p.as_os_str().is_empty() { Path::new(".") } else { p })
            .unwrap_or_else(|_| Path::new("."));

        if !seen_paths.insert(rel.to_path_buf()) {
            return Err(DiscoverError::DuplicateNodePath(rel.to_path_buf()));
        }

        if let Some(ref tasks) = parsed.node.tasks {
            config::validate_tasks(tasks)?;
        }

        nodes.push(DiscoveredNode {
            config_path: path.to_path_buf(),
            node_path: rel.to_path_buf(),
            node_config: parsed.node,
        });
    }

    Ok(nodes)
}

pub fn resolve_nodes(root: &Path, discovered: Vec<DiscoveredNode>) -> Vec<ResolvedNode> {
    discovered
        .into_iter()
        .map(|d| config::resolve_node(&d.config_path, &d.node_path, d.node_config))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_is_ignored_dir() {
        assert!(is_ignored_dir(".git"));
        assert!(is_ignored_dir("target"));
        assert!(is_ignored_dir("node_modules"));
        assert!(is_ignored_dir("result-abc"));
        assert!(!is_ignored_dir("src"));
        assert!(!is_ignored_dir("docs"));
        assert!(!is_ignored_dir(".opencode"));
    }

    #[test]
    fn test_discovery_skips_ignored_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Create valid .tend.json files
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("target")).unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();

        let root_config = r#"{"version":1,"node":{"id":"root","tasks":[]}}"#;
        let src_config = r#"{"version":1,"node":{"id":"src","tasks":[]}}"#;
        let docs_config = r#"{"version":1,"node":{"id":"docs","tasks":[]}}"#;

        fs::write(root.join(".tend.json"), root_config).unwrap();
        fs::write(root.join("src/.tend.json"), src_config).unwrap();
        fs::write(root.join("target/.tend.json"), src_config).unwrap();
        fs::write(root.join(".git/.tend.json"), src_config).unwrap();
        fs::write(root.join("docs/.tend.json"), docs_config).unwrap();

        let nodes = discover_configs(root, None).unwrap();
        let paths: Vec<_> = nodes.iter().map(|n| n.node_path.clone()).collect();

        assert!(paths.contains(&PathBuf::from(".")));
        assert!(paths.contains(&PathBuf::from("src")));
        assert!(paths.contains(&PathBuf::from("docs")));
        assert!(!paths.contains(&PathBuf::from("target")));
        assert!(!paths.contains(&PathBuf::from(".git")));
        assert_eq!(paths.len(), 3);
    }
}
