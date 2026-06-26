{ ... }: {
  perSystem = { config, pkgs, ... }: let
    tools = pkgs.rustPlatform.buildRustPackage {
      pname = "phenix-tools";
      version = "0.1.0";
      src = ../.;
      cargoLock.lockFile = ../Cargo.lock;
    };
    tendCliPkg = pkgs.rustPlatform.buildRustPackage {
      pname = "tend";
      version = "0.1.0";
      src = ../.;
      cargoLock.lockFile = ../Cargo.lock;
      cargoBuildFlags = "-p tend-cli";
    };
    stitchCliPkg = pkgs.rustPlatform.buildRustPackage {
      pname = "stitch";
      version = "0.1.0";
      src = ../.;
      cargoLock.lockFile = ../Cargo.lock;
      cargoBuildFlags = "-p stitch-cli";
    };

    # Legacy packages (deprecated)
    tendPkg = pkgs.rustPlatform.buildRustPackage {
      pname = "tend-legacy";
      version = "0.1.0";
      src = ../.;
      cargoLock.lockFile = ../Cargo.lock;
      cargoBuildFlags = "-p tend-cli";
    };
    stitchPkg = pkgs.rustPlatform.buildRustPackage {
      pname = "stitch-legacy";
      version = "0.1.0";
      src = ../.;
      cargoLock.lockFile = ../Cargo.lock;
      cargoBuildFlags = "-p stitch-cli";
    };
  in {
    packages.gate = tools;
    packages.tend = tendCliPkg;
    packages.stitch = stitchCliPkg;
    packages.default = tools;

    apps.gate = {
      type = "app";
      program = "${pkgs.writeShellScriptBin "phenix-gate" ''
        exec ${tools}/bin/pt gate "$@"
      ''}/bin/phenix-gate";
    };
    apps.tend = {
      type = "app";
      program = "${tendCliPkg}/bin/tend";
    };
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
