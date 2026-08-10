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

      devShells.default = pkgs.mkShell {
        name = "phenix-tools-dev";
        packages = [
          pkgs.git
          pkgs.nix
          stitch
          stitchMcp
          workspace
        ];
        shellHook = ''
          echo "phenix-tools thin aggregator"
          echo "  stitch:    $(stitch --version 2>/dev/null || echo '?')"
          echo "  workspace: phenix-workspace --help"
        '';
      };
    };
}
