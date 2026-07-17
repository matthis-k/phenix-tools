# Phenix tools

This flake aggregates Phenix command-line tools without duplicating their implementations. `phenix-stitch` remains the sole source of repository graph and ordered-execution behavior.

`phenix-workspace` consumes Stitch's read-only workspace inventory and performs the local lifecycle operations that do not belong in Stitch:

```sh
phenix-workspace init
phenix-workspace clean
phenix-workspace clean --apply
phenix-workspace dev
phenix-workspace check
```

The wrapper clones missing repositories into the root-owned workspace, fast-forwards clean repositories, and removes only obsolete clones carrying its private marker. `dev` and `check` invoke Nix with `path:` overrides for every local flake, so uncommitted local source changes are evaluated without changing the production lock file.
