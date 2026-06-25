use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpRoot {
    pub uri: String,
    pub path: PathBuf,
    pub writable: bool,
}

impl McpRoot {
    pub fn new(path: PathBuf, writable: bool) -> Self {
        let canonical = if path.is_absolute() {
            dunce::canonicalize(&path).unwrap_or(path.clone())
        } else {
            path.clone()
        };
        let uri = format!("file://{}", canonical.display());
        Self { uri, path: canonical, writable }
    }

    pub fn contains(&self, target: &Path) -> bool {
        if let Ok(canonical) = dunce::canonicalize(target) {
            canonical.starts_with(&self.path)
        } else {
            target.starts_with(&self.path)
        }
    }
}

pub struct RootValidator {
    roots: Vec<McpRoot>,
}

impl RootValidator {
    pub fn new(roots: Vec<McpRoot>) -> Self {
        Self { roots }
    }

    pub fn validate_path(&self, path: &Path) -> Result<(), String> {
        if path.is_absolute() {
            for root in &self.roots {
                if root.contains(path) {
                    return Ok(());
                }
            }
            return Err(format!(
                "Path '{}' is outside all declared roots",
                path.display()
            ));
        }
        Ok(())
    }

    pub fn resolve_in_root(&self, path: &Path, root: &McpRoot) -> Result<PathBuf, String> {
        let resolved = if path.is_relative() {
            root.path.join(path)
        } else {
            path.to_path_buf()
        };

        if !root.contains(&resolved) {
            return Err(format!(
                "Path '{}' resolves outside root '{}'",
                path.display(),
                root.path.display()
            ));
        }

        Ok(resolved)
    }

    pub fn roots(&self) -> &[McpRoot] {
        &self.roots
    }
}

mod dunce {
    use std::path::{Path, PathBuf};

    pub fn canonicalize(path: &Path) -> Result<PathBuf, std::io::Error> {
        if path.exists() {
            path.canonicalize()
        } else {
            Ok(path.to_path_buf())
        }
    }
}
