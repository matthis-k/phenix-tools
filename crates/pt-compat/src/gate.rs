// pt-compat gate shim — delegates to `tend`
// See: docs/tend.md, docs/agents/tool-routing.md
//
// This file intentionally contains no parser, runner, or discovery logic.
// The legacy gate implementation was migrated to `tend`.

use std::path::{Path, PathBuf};

pub fn dispatch_stub(
    workspace_root: &Path,
    _config_path: Option<PathBuf>,
) -> Result<(), String> {
    eprintln!("note: pt gate is deprecated. Use `tend` instead:");
    eprintln!("  tend plan    -- show which checks would run");
    eprintln!("  tend run     -- execute checks");
    eprintln!("  tend explain -- explain failures");
    eprintln!();
    eprintln!("Or use convenience aliases if available:");
    eprintln!("  tend verify changed   -- non-mutating checks (was: pt gate changed)");
    eprintln!("  tend verify full      -- run all non-mutating checks");
    eprintln!("  tend gate             -- alias for `verify changed`");
    eprintln!();

    // Attempt to delegate to `tend` if on PATH
    let status = std::process::Command::new("tend")
        .args(["verify", "changed"])
        .current_dir(workspace_root)
        .status();

    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("tend verify changed exited with code {}", s)),
        Err(_) => {
            eprintln!("warning: `tend` not found on PATH. Install it or run from the dev shell.");
            eprintln!("  nix run .#tend -- verify changed");
            Ok(())
        }
    }
}
