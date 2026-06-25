{
  description = "Phenix cross-repo developer and maintenance tooling";

  inputs = {
    phenix-pins.url = "github:matthis-k/phenix-pins";
    nixpkgs.follows = "phenix-pins/nixpkgs";
  };

  outputs = { self, nixpkgs, ... }: let
    systems = [ "x86_64-linux" "aarch64-linux" ];
    forAllSystems = f:
      builtins.listToAttrs (map (sys: {
        name = sys;
        value = f sys;
      }) systems);
    pkgsFor = system: import nixpkgs { inherit system; };
  in {
    packages = forAllSystems (system: let
      pkgs = pkgsFor system;
      tools = pkgs.rustPlatform.buildRustPackage {
        pname = "phenix-tools";
        version = "0.1.0";
        src = ./.;
        cargoLock.lockFile = ./Cargo.lock;
      };
    in {
      default = tools;
      sync = tools;
    });

    apps = forAllSystems (system: {
      sync = {
        type = "app";
        program = "${self.packages.${system}.default}/bin/pt";
      };
      default = {
        type = "app";
        program = "${self.packages.${system}.default}/bin/pt";
      };
    });
  };
}
