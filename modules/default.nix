{ ... }: {
  perSystem = { config, pkgs, ... }: let
    tools = pkgs.rustPlatform.buildRustPackage {
      pname = "phenix-tools";
      version = "0.1.0";
      src = ../.;
      cargoLock.lockFile = ../Cargo.lock;
    };
    stitchPkg = pkgs.rustPlatform.buildRustPackage {
      pname = "stitch";
      version = "0.1.0";
      src = ../.;
      cargoLock.lockFile = ../Cargo.lock;
      cargoBuildFlags = "-p stitch";
    };
  in {
    packages.stitch = stitchPkg;
    packages.default = tools;

    apps.stitch = {
      type = "app";
      program = "${stitchPkg}/bin/stitch";
    };
    apps.default = {
      type = "app";
      program = "${tools}/bin/pt";
    };
  };
}
