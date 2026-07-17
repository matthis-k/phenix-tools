# Phenix tools

This flake aggregates Phenix command-line tools without duplicating their implementations. `phenix-stitch` remains the sole source of repository graph and ordered-execution behavior.

`phenix-workspace` consumes Stitch's read-only workspace inventory and performs the local lifecycle operations that do not belong in Stitch:

```sh
phenix-workspace init
phenix-workspace clean
phenix-workspace clean --apply
phenix-workspace nix flake check
phenix-workspace nix develop
```

The wrapper clones missing repositories into the root-owned workspace, fast-forwards clean repositories, and removes only obsolete clones carrying its private marker.

The `nix` command changes to the workspace root and forwards the requested Nix subcommand with `git+file:` overrides for every local Phenix flake. Dirty tracked changes are evaluated immediately without changing the production lock file. Newly created source files must be added to Git's index, but do not need to be committed.

`dev` and `check` remain convenience aliases for `nix develop` and `nix flake check`.
