{ inputs, ... }:
{
  perSystem =
    {
      config,
      pkgs,
      system,
      ...
    }:
    let
      stitch = inputs.phenix-stitch.packages.${system}.stitch;
      stitchMcp = inputs.phenix-stitch.packages.${system}.stitch-mcp;
      opencode = inputs.phenix-opencode.packages.${system}.default;
      workspace = config.packages.phenix-workspace;
      workspaceInterface = pkgs.runCommand "phenix-workspace-interface" { nativeBuildInputs = [ workspace ]; } ''
        phenix-workspace --help > "$out"
        grep -q 'nix NIX-COMMAND' "$out"
        grep -q 'alias for: nix develop' "$out"
        grep -q 'alias for: nix flake check' "$out"
      '';
    in
    {
      packages = {
        inherit stitch opencode;
        stitch-mcp = stitchMcp;
        default = stitch;
      };

      apps = {
        stitch = inputs.phenix-stitch.apps.${system}.stitch;
        stitch-mcp = inputs.phenix-stitch.apps.${system}.stitch-mcp;
        opencode = {
          type = "app";
          program = "${opencode}/bin/opencode";
        };
        default = inputs.phenix-stitch.apps.${system}.stitch;
      };

      checks = {
        stitch-package = stitch;
        stitch-mcp-package = stitchMcp;
        phenix-workspace-package = workspace;
        phenix-workspace-interface = workspaceInterface;
      };

      devShells.default = pkgs.mkShell {
        name = "phenix-tools-dev";
        packages = [
          pkgs.devenv
          pkgs.git
          pkgs.nix
          stitch
          stitchMcp
          workspace
        ];
        shellHook = ''
          echo "phenix-tools thin aggregator"
          echo "  maintenance: devenv test"
          echo "  fixes:       devenv tasks run maintenance:fix"
          echo "  stitch:      $(stitch --version 2>/dev/null || echo '?')"
          echo "  workspace:   phenix-workspace --help"
        '';
      };
    };
}
