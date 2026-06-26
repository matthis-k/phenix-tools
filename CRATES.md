# Crate Architecture

```
phenix-tools/
  crates/
    tend/          # core library crate — composable task/check/hook harness
    tend-cli/      # CLI frontend, binary: tend
    tend-mcp/      # MCP frontend, binary: tend-mcp

    stitch/        # core library crate — multi-repo changeset coordinator
    stitch-cli/    # CLI frontend, binary: stitch
    stitch-mcp/    # MCP frontend, binary: stitch-mcp

    phenix-mcp-core/  # shared MCP framework (JSON-RPC stdio)
```

## Frontend Rule

Business logic belongs in the library crates (`tend`, `stitch`).

The CLI and MCP crates are frontends only:

- argument parsing
- terminal formatting
- JSON output formatting
- MCP request/response adaptation
- exit codes

**No duplicated business logic** in CLI/MCP crates. If behavior is needed by both, it belongs in the library.

## Responsibilities

| Tool   | Owns                                                                 |
|--------|----------------------------------------------------------------------|
| `tend` | distributed task files, recursive discovery, composable task tree    |
| `stitch` | workspace repo discovery, multi-repo status, changeset planning    |
| `phenix` | reserved for future high-level Phenix workspace/OS commands        |

## Legacy

The top-level `phenix-tools` / `pt` binary is deprecated. It exists as a
compatibility shim. Use `tend` and `stitch` directly.

The old `gate.rs` (`.phenix-checks.json` runner) is also deprecated in favor
of `tend` (`.tend.json`).
