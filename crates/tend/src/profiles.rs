use crate::model::{ResolvedNode, ResolvedTask};

#[derive(Debug, Clone)]
pub struct ProfileViolation {
    pub task_id: String,
    pub profile: String,
    pub message: String,
}

impl std::fmt::Display for ProfileViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "task '{}' in profile '{}': {}",
            self.task_id, self.profile, self.message
        )
    }
}

pub fn validate_profiles(nodes: &[ResolvedNode]) -> Result<(), Vec<ProfileViolation>> {
    let mut violations = Vec::new();

    for node in nodes {
        for task in &node.tasks {
            let profiles = match &task.config.profiles {
                Some(p) => p.clone(),
                None => continue, // Will be validated separately or default to manual
            };

            let cmd_string = command_string(task);

            for profile in &profiles {
                // Rule: nix-check must not include tasks that invoke `nix flake check`
                if profile == "nix-check" && cmd_string.contains("nix flake check") {
                    violations.push(ProfileViolation {
                        task_id: task.config.id.clone(),
                        profile: profile.clone(),
                        message: "must not invoke 'nix flake check' (would cause recursion)"
                            .to_string(),
                    });
                }

                // Rule: mutating tasks are forbidden in nix-check and git-hook
                if (profile == "nix-check" || profile == "git-hook")
                    && task
                        .config
                        .mutates
                        .unwrap_or_else(|| crate::config::default_mutates(&task.config.phase))
                {
                    violations.push(ProfileViolation {
                        task_id: task.config.id.clone(),
                        profile: profile.clone(),
                        message: "mutating tasks are not allowed in this profile".to_string(),
                    });
                }

                // Rule: interactive tasks are forbidden in nix-check and git-hook
                if (profile == "nix-check" || profile == "git-hook")
                    && task.config.interactive.unwrap_or(false)
                {
                    violations.push(ProfileViolation {
                        task_id: task.config.id.clone(),
                        profile: profile.clone(),
                        message: "interactive tasks are not allowed in this profile".to_string(),
                    });
                }

                // Rule: test-tagged tasks are forbidden in git-hook
                if profile == "git-hook" {
                    if let Some(ref tags) = task.config.tags {
                        if tags.iter().any(|t| t == "test") {
                            violations.push(ProfileViolation {
                                task_id: task.config.id.clone(),
                                profile: profile.clone(),
                                message: "test tasks are not allowed in git-hook profile"
                                    .to_string(),
                            });
                        }
                        if tags.iter().any(|t| t == "slow") {
                            violations.push(ProfileViolation {
                                task_id: task.config.id.clone(),
                                profile: profile.clone(),
                                message: "slow tasks are not allowed in git-hook profile"
                                    .to_string(),
                            });
                        }
                        if tags.iter().any(|t| t == "network") {
                            violations.push(ProfileViolation {
                                task_id: task.config.id.clone(),
                                profile: profile.clone(),
                                message: "network tasks are not allowed in git-hook profile"
                                    .to_string(),
                            });
                        }
                    }
                }

                // Rule: network-tagged tasks are forbidden in nix-check
                if profile == "nix-check" {
                    if let Some(ref tags) = task.config.tags {
                        if tags.iter().any(|t| t == "network") {
                            violations.push(ProfileViolation {
                                task_id: task.config.id.clone(),
                                profile: profile.clone(),
                                message: "network tasks are not allowed in nix-check profile"
                                    .to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

fn command_string(task: &ResolvedTask) -> String {
    match &task.config.kind {
        crate::model::TaskKind::Command { command, .. } => command.join(" "),
        _ => String::new(),
    }
}

pub fn is_safe_generated_source_prerequisite(task: &ResolvedTask) -> bool {
    task.config.phase == crate::model::Phase::Generate
        && task.config.mutates.unwrap_or(true)
        && task.config.sandbox_safe.unwrap_or(false)
        && !task.config.interactive.unwrap_or(false)
        && !task.config.network.unwrap_or(false)
        && task
            .config
            .tags
            .as_ref()
            .is_some_and(|tags| tags.iter().any(|tag| tag == "generated-source"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use std::path::Path;

    fn make_verify_task(
        id: &str,
        profiles: Vec<&str>,
        tags: Vec<&str>,
        mutates: bool,
        interactive: bool,
        command: Vec<&str>,
    ) -> ResolvedTask {
        ResolvedTask {
            config: TaskConfig {
                id: id.to_string(),
                description: None,
                phase: Phase::Verify,
                kind: TaskKind::Command {
                    command: command.iter().map(|s| s.to_string()).collect(),
                    expect: None,
                },
                context: None,
                tags: Some(tags.iter().map(|s| s.to_string()).collect()),
                profiles: Some(profiles.iter().map(|s| s.to_string()).collect()),
                mutates: Some(mutates),
                interactive: Some(interactive),
                network: Some(false),
                sandbox_safe: Some(true),
                when: None,
                always: None,
                requires: None,
                before: None,
                after: None,
            },
            parent_node_path: Path::new(".").to_path_buf(),
        }
    }

    fn make_safe_generated_task(id: &str) -> ResolvedTask {
        let mut task = make_verify_task(
            id,
            vec!["nix-check"],
            vec!["generated-source"],
            true,
            false,
            vec!["tend", "flake", "write"],
        );
        task.config.phase = Phase::Generate;
        task.config.sandbox_safe = Some(true);
        task.config.network = Some(false);
        task
    }

    fn make_node(id: &str, tasks: Vec<ResolvedTask>) -> ResolvedNode {
        ResolvedNode {
            config_path: Path::new(".tend.json").to_path_buf(),
            node_path: Path::new(".").to_path_buf(),
            id: id.to_string(),
            description: String::new(),
            tags: vec![],
            when: None,
            context: ContextConfig {
                workdir: None,
                env: None,
                shell: None,
            },
            before: vec![],
            after: vec![],
            tasks,
        }
    }

    #[test]
    fn test_fails_if_test_task_in_git_hook() {
        let task = make_verify_task(
            "cargo-test",
            vec!["git-hook"],
            vec!["test"],
            false,
            false,
            vec!["cargo", "test"],
        );
        let node = make_node("root", vec![task]);
        let result = validate_profiles(&[node]);
        assert!(result.is_err());
        let msgs: Vec<String> = result.unwrap_err().iter().map(|v| v.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("test")));
    }

    #[test]
    fn test_generated_source_tag_does_not_globally_allow_strict_profile() {
        let task = make_safe_generated_task("generate-flake");
        let node = make_node("root", vec![task]);
        let result = validate_profiles(&[node]);
        assert!(result.is_err());
        let msgs: Vec<String> = result.unwrap_err().iter().map(|v| v.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("mutating tasks")));
    }

    #[test]
    fn test_fails_if_slow_task_in_git_hook() {
        let task = make_verify_task(
            "slow-task",
            vec!["git-hook"],
            vec!["slow"],
            false,
            false,
            vec!["cargo", "test"],
        );
        let node = make_node("root", vec![task]);
        let result = validate_profiles(&[node]);
        assert!(result.is_err());
    }

    #[test]
    fn test_fails_if_network_task_in_nix_check() {
        let task = make_verify_task(
            "network-task",
            vec!["nix-check"],
            vec!["network"],
            false,
            false,
            vec!["curl"],
        );
        let node = make_node("root", vec![task]);
        let result = validate_profiles(&[node]);
        assert!(result.is_err());
    }

    #[test]
    fn test_fails_if_mutating_in_nix_check() {
        let task = make_verify_task(
            "fmt-fix",
            vec!["nix-check"],
            vec!["format"],
            true,
            false,
            vec!["cargo", "fmt"],
        );
        let node = make_node("root", vec![task]);
        let result = validate_profiles(&[node]);
        assert!(result.is_err());
    }

    #[test]
    fn test_fails_if_interactive_in_nix_check() {
        let task = make_verify_task(
            "interactive-task",
            vec!["nix-check"],
            vec![],
            false,
            true,
            vec!["vim"],
        );
        let node = make_node("root", vec![task]);
        let result = validate_profiles(&[node]);
        assert!(result.is_err());
    }

    #[test]
    fn test_fails_if_nix_flake_check_in_nix_check() {
        let task = make_verify_task(
            "nix-flake-check",
            vec!["nix-check"],
            vec!["nix"],
            false,
            false,
            vec!["nix", "flake", "check"],
        );
        let node = make_node("root", vec![task]);
        let result = validate_profiles(&[node]);
        assert!(result.is_err());
    }

    #[test]
    fn test_fails_if_mutating_in_git_hook() {
        let task = make_verify_task(
            "fmt-fix",
            vec!["git-hook"],
            vec!["format"],
            true,
            false,
            vec!["cargo", "fmt"],
        );
        let node = make_node("root", vec![task]);
        let result = validate_profiles(&[node]);
        assert!(result.is_err());
    }

    #[test]
    fn test_fails_if_interactive_in_git_hook() {
        let task = make_verify_task(
            "interactive-task",
            vec!["git-hook"],
            vec![],
            false,
            true,
            vec!["vim"],
        );
        let node = make_node("root", vec![task]);
        let result = validate_profiles(&[node]);
        assert!(result.is_err());
    }

    #[test]
    fn test_fails_if_network_in_git_hook() {
        let task = make_verify_task(
            "network-task",
            vec!["git-hook"],
            vec!["network"],
            false,
            false,
            vec!["curl"],
        );
        let node = make_node("root", vec![task]);
        let result = validate_profiles(&[node]);
        assert!(result.is_err());
    }

    #[test]
    fn test_actionable_error_messages() {
        let task = make_verify_task(
            "cargo-test",
            vec!["git-hook"],
            vec!["test"],
            false,
            false,
            vec!["cargo", "test"],
        );
        let node = make_node("root", vec![task]);
        let result = validate_profiles(&[node]);
        assert!(result.is_err());
        let msg = result.unwrap_err()[0].to_string();
        assert!(msg.contains("cargo-test"));
        assert!(msg.contains("git-hook"));
        assert!(msg.contains("test"));
    }

    #[test]
    fn test_nix_check_excludes_nix_flake_check_task() {
        let bad = make_verify_task(
            "nix-flake-check",
            vec!["nix-check"],
            vec!["nix"],
            false,
            false,
            vec!["nix", "flake", "check"],
        );
        let good = make_verify_task(
            "cargo-test",
            vec!["nix-check"],
            vec!["test"],
            false,
            false,
            vec!["cargo", "test"],
        );
        let node = make_node("root", vec![bad, good]);
        let result = validate_profiles(&[node]);
        assert!(result.is_err());
        let msgs: Vec<String> = result.unwrap_err().iter().map(|v| v.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("nix-flake-check")));
        // The good task should not appear in violations
        assert!(!msgs.iter().any(|m| m.contains("cargo-test")));
    }

    #[test]
    fn test_network_task_in_git_hook_fails() {
        let task = make_verify_task(
            "net-task",
            vec!["git-hook"],
            vec!["network"],
            false,
            false,
            vec!["curl"],
        );
        let node = make_node("root", vec![task]);
        let result = validate_profiles(&[node]);
        assert!(result.is_err());
    }

    #[test]
    fn test_slow_task_in_git_hook_fails() {
        let task = make_verify_task(
            "slow-task",
            vec!["git-hook"],
            vec!["slow"],
            false,
            false,
            vec!["cargo", "test"],
        );
        let node = make_node("root", vec![task]);
        let result = validate_profiles(&[node]);
        assert!(result.is_err());
    }
}
