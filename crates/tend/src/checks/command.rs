use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use crate::model::{ExpectConfig, ShellConfig};

use super::CheckResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedCommand {
    pub program: String,
    pub args: Vec<String>,
}

pub fn build_shell_ref(shell: &ShellConfig) -> String {
    let flake = shell.flake.as_deref().unwrap_or(".");
    let name = shell.name.as_deref().unwrap_or("default");
    if name == "default" {
        flake.to_string()
    } else {
        format!("{flake}#{name}")
    }
}

pub fn prepare_command(
    command: &[String],
    shell: Option<&ShellConfig>,
) -> PreparedCommand {
    if command.is_empty() {
        return PreparedCommand {
            program: String::new(),
            args: Vec::new(),
        };
    }

    if let Some(shell) = shell {
        let mut args: Vec<String> = Vec::new();

        args.push("develop".to_string());

        if shell.impure.unwrap_or(false) {
            args.push("--impure".to_string());
        }

        if shell.accept_flake_config.unwrap_or(false) {
            args.push("--accept-flake-config".to_string());
        }

        if let Some(extra) = &shell.extra_args {
            args.extend(extra.iter().cloned());
        }

        args.push(build_shell_ref(shell));
        args.push("--command".to_string());
        args.extend(command.iter().cloned());

        PreparedCommand {
            program: "nix".to_string(),
            args,
        }
    } else {
        PreparedCommand {
            program: command[0].clone(),
            args: command[1..].to_vec(),
        }
    }
}

pub fn run_command(
    command: &[String],
    expect: Option<&ExpectConfig>,
    workdir: &Path,
    env: Option<&HashMap<String, String>>,
    shell: Option<&ShellConfig>,
) -> CheckResult {
    if command.is_empty() {
        return CheckResult::skip();
    }

    let prepared = prepare_command(command, shell);

    let mut cmd = Command::new(&prepared.program);
    cmd.args(&prepared.args).current_dir(workdir);

    if let Some(env_vars) = env {
        cmd.envs(env_vars);
    }

    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => {
            let hint = if shell.is_some() {
                ". Is `nix` available on PATH and does the requested dev shell exist?"
            } else {
                ""
            };
            return CheckResult::error(format!("failed to execute command: {e}{hint}"));
        }
    };

    let status = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    let expected = expect.and_then(|e| e.status).unwrap_or(0);

    if status == expected {
        CheckResult::pass_with(stdout, stderr)
    } else {
        let extra = if shell.is_some() && status != 0 {
            format!(
                "\n--- nix develop stderr ---\n{stderr}\n--- end ---\nIf the named dev shell does not exist, ensure it is defined in the flake."
            )
        } else {
            String::new()
        };
        CheckResult {
            outcome: crate::checks::CheckOutcome::Failed {
                reason: format!(
                    "command exited with status {status} (expected {expected}){extra}"
                ),
            },
            stdout,
            stderr,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_shell(flake: Option<&str>, name: Option<&str>) -> ShellConfig {
        ShellConfig {
            flake: flake.map(|s| s.to_string()),
            name: name.map(|s| s.to_string()),
            impure: None,
            accept_flake_config: None,
            extra_args: None,
        }
    }

    #[test]
    fn test_prepare_command_no_shell() {
        let cmd = vec!["cargo".to_string(), "test".to_string()];
        let prepared = prepare_command(&cmd, None);
        assert_eq!(prepared.program, "cargo");
        assert_eq!(prepared.args, vec!["test"]);
    }

    #[test]
    fn test_prepare_command_with_shell_named() {
        let cmd = vec!["stitch".to_string(), "graph".to_string(), "verify".to_string()];
        let shell = make_shell(Some("."), Some("test"));
        let prepared = prepare_command(&cmd, Some(&shell));
        assert_eq!(prepared.program, "nix");
        assert_eq!(
            prepared.args,
            vec!["develop", ".#test", "--command", "stitch", "graph", "verify"]
        );
    }

    #[test]
    fn test_prepare_command_with_shell_default() {
        let cmd = vec!["cargo".to_string(), "check".to_string()];
        let shell = make_shell(Some("."), Some("default"));
        let prepared = prepare_command(&cmd, Some(&shell));
        assert_eq!(prepared.program, "nix");
        assert_eq!(
            prepared.args,
            vec!["develop", ".", "--command", "cargo", "check"]
        );
    }

    #[test]
    fn test_prepare_command_shell_default_flake_omitted() {
        let cmd = vec!["cargo".to_string(), "check".to_string()];
        let shell = make_shell(None, None);
        let prepared = prepare_command(&cmd, Some(&shell));
        assert_eq!(prepared.program, "nix");
        assert_eq!(
            prepared.args,
            vec!["develop", ".", "--command", "cargo", "check"]
        );
    }

    #[test]
    fn test_prepare_command_with_impure() {
        let cmd = vec!["echo".to_string(), "hi".to_string()];
        let mut shell = make_shell(Some("."), Some("test"));
        shell.impure = Some(true);
        let prepared = prepare_command(&cmd, Some(&shell));
        assert_eq!(prepared.program, "nix");
        assert_eq!(
            prepared.args,
            vec!["develop", "--impure", ".#test", "--command", "echo", "hi"]
        );
    }

    #[test]
    fn test_prepare_command_with_extra_args() {
        let cmd = vec!["echo".to_string(), "hi".to_string()];
        let mut shell = make_shell(Some("."), Some("test"));
        shell.extra_args = Some(vec!["-v".to_string()]);
        let prepared = prepare_command(&cmd, Some(&shell));
        assert_eq!(prepared.program, "nix");
        assert_eq!(
            prepared.args,
            vec!["develop", "-v", ".#test", "--command", "echo", "hi"]
        );
    }

    #[test]
    fn test_build_shell_ref_named() {
        let shell = make_shell(Some("."), Some("test"));
        assert_eq!(build_shell_ref(&shell), ".#test");
    }

    #[test]
    fn test_build_shell_ref_default() {
        let shell = make_shell(Some("."), Some("default"));
        assert_eq!(build_shell_ref(&shell), ".");
    }

    #[test]
    fn test_build_shell_ref_custom_flake() {
        let shell = make_shell(Some("./phenix-tools"), Some("test"));
        assert_eq!(build_shell_ref(&shell), "./phenix-tools#test");
    }

    #[test]
    fn test_prepare_command_empty() {
        let cmd: Vec<String> = vec![];
        let prepared = prepare_command(&cmd, None);
        assert_eq!(prepared.program, "");
        assert!(prepared.args.is_empty());
    }
}
