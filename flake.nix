{
  description = "Thin aggregation of Phenix command-line tools";

  inputs = {
    flake-parts.url = "github:hercules-ci/flake-parts";
    phenix-flake-ci.url = "github:matthis-k/phenix-flake-ci";
    phenix-pins = {
      url = "github:matthis-k/phenix-pins";
      inputs.phenix-flake-ci.follows = "phenix-flake-ci";
    };
    nixpkgs.follows = "phenix-pins/nixpkgs";
    phenix-stitch = {
      url = "github:matthis-k/phenix-stitch";
      inputs = {
        flake-parts.follows = "flake-parts";
        phenix-flake-ci.follows = "phenix-flake-ci";
        phenix-pins.follows = "phenix-pins";
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
      imports = [
        ./modules/standalone.nix
        ./modules/workspace.nix
        ./modules/development.nix
      ];
      flake.flakeModules.default = import ./modules/flake-module.nix;
    };
}
