# phenix-tools

Tools for Phenix workspace automation.

## Tend shell discovery

Tend can automatically run command checks in a sibling `tend-shell.nix` when the
file is next to a discovered `.tend.json`.

Shell precedence is:

1. task `context.shell`
2. node `context.shell`
3. local sibling `tend-shell.nix`
4. inherited root-context sibling `tend-shell.nix`
5. no shell

`tend-shell.nix` files run with `nix-shell <path> --run <command>`. Flake shells
configured explicitly in `context.shell` continue to run through `nix develop`.

To opt out of automatic or inherited shell use for a config context, set an empty
shell with `auto` disabled:

```json
{
  "context": {
    "shell": { "auto": false }
  }
}
```
