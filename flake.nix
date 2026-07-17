{
  description = "Thin aggregation of Phenix command-line tools";

  inputs = {
    flake-parts.url = "github:hercules-ci/flake-parts";
    phenix-pins.url = "github:matthis-k/phenix-pins";
    nixpkgs.follows = "phenix-pins/nixpkgs";
    phenix-stitch = {
      url = "github:matthis-k/phenix-stitch";
      inputs = {
        phenix-pins.follows = "phenix-pins";
        flake-parts.follows = "flake-parts";
      };
    };
    phenix-opencode.url = "github:matthis-k/phenix-opencode";
  };

  outputs =
    inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      imports = [ ./modules/standalone.nix ];
      flake.flakeModules.default = import ./modules/flake-module.nix;
    };
}
