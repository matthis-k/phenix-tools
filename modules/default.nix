{ ... }: {
  perSystem = { config, pkgs, ... }: let
    tools = pkgs.rustPlatform.buildRustPackage {
      pname = "phenix-tools";
      version = "0.1.0";
      src = ../.;
      cargoLock.lockFile = ../Cargo.lock;
    };
    stitchCliPkg = pkgs.rustPlatform.buildRustPackage {
      pname = "stitch";
      version = "0.1.0";
      src = ../.;
      cargoLock.lockFile = ../Cargo.lock;
      cargoBuildFlags = "-p stitch-cli";
    };
  in {
    packages.stitch = stitchCliPkg;
    packages.default = tools;

    apps.stitch = {
      type = "app";
      program = "${stitchCliPkg}/bin/stitch";
    };
    apps.default = {
      type = "app";
      program = "${tools}/bin/pt";
    };
  };
}
