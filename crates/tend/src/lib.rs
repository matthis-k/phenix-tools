pub mod checks;
pub mod config;
pub mod discover;
pub mod execute;
pub mod flake {
    use std::path::Path;

    const SOURCE_FILE: &str = "flake.nix.in";
    const GENERATED_FILE: &str = "flake.nix";

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct FlakeStatus {
        pub flake_nix_exists: bool,
        pub flake_lock_exists: bool,
        pub source_exists: bool,
        pub up_to_date: Option<bool>,
    }

    pub fn status(root: &Path) -> FlakeStatus {
        let generated = root.join(GENERATED_FILE);
        let source = root.join(SOURCE_FILE);
        let source_exists = source.exists();
        let flake_nix_exists = generated.exists();
        let up_to_date = if source_exists && flake_nix_exists {
            match (std::fs::read(&source), std::fs::read(&generated)) {
                (Ok(source_bytes), Ok(generated_bytes)) => Some(source_bytes == generated_bytes),
                _ => Some(false),
            }
        } else if source_exists {
            Some(false)
        } else {
            None
        };

        FlakeStatus {
            flake_nix_exists,
            flake_lock_exists: root.join("flake.lock").exists(),
            source_exists,
            up_to_date,
        }
    }

    pub fn check(root: &Path) -> Result<FlakeStatus, String> {
        let status = status(root);
        if !status.source_exists {
            return Err(format!(
                "missing generated flake source {} in {}",
                SOURCE_FILE,
                root.display()
            ));
        }
        if !status.flake_nix_exists {
            return Err(format!("missing {} in {}", GENERATED_FILE, root.display()));
        }
        if status.up_to_date != Some(true) {
            return Err(format!(
                "{} is not up to date with {} in {}",
                GENERATED_FILE,
                SOURCE_FILE,
                root.display()
            ));
        }
        Ok(status)
    }

    pub fn write(root: &Path) -> Result<FlakeStatus, String> {
        let source = root.join(SOURCE_FILE);
        let generated = root.join(GENERATED_FILE);
        if !source.exists() {
            return Err(format!(
                "missing generated flake source {} in {}; refusing placeholder generation",
                SOURCE_FILE,
                root.display()
            ));
        }
        std::fs::copy(&source, &generated)
            .map_err(|e| format!("write {} from {}: {e}", generated.display(), source.display()))?;
        Ok(status(root))
    }
}
pub mod model;
pub mod planner;
pub mod profiles;
pub mod report;
pub mod workspace;

#[cfg(test)]
mod tests {
    use super::flake;

    #[test]
    fn flake_write_refuses_placeholder_without_source() {
        let temp = tempfile::tempdir().expect("tempdir");
        let err = flake::write(temp.path()).expect_err("missing source must fail");
        assert!(err.contains("refusing placeholder generation"));
    }

    #[test]
    fn flake_write_copies_repo_owned_source() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("flake.nix.in");
        let generated = temp.path().join("flake.nix");
        std::fs::write(&source, "{ outputs = { self }: {}; }\n").expect("write source");

        let status = flake::write(temp.path()).expect("write generated flake");

        assert!(status.flake_nix_exists);
        assert!(status.source_exists);
        assert_eq!(status.up_to_date, Some(true));
        assert_eq!(
            std::fs::read_to_string(generated).expect("read generated"),
            "{ outputs = { self }: {}; }\n"
        );
    }
}
