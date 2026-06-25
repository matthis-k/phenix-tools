{ ... }: {
  perSystem = { config, pkgs, ... }: let
    tools = pkgs.rustPlatform.buildRustPackage {
      pname = "phenix-tools";
      version = "0.1.0";
      src = ../.;
      cargoLock.lockFile = ../Cargo.lock;
    };
    tendPkg = pkgs.rustPlatform.buildRustPackage {
      pname = "tend";
      version = "0.1.0";
      src = ../.;
      cargoLock.lockFile = ../Cargo.lock;
      cargoBuildFlags = "-p tend";
    };
    stitchPkg = pkgs.rustPlatform.buildRustPackage {
      pname = "stitch";
      version = "0.1.0";
      src = ../.;
      cargoLock.lockFile = ../Cargo.lock;
      cargoBuildFlags = "-p stitch";
    };
  in {
    packages.gate = tools;
    packages.tend = tendPkg;
    packages.stitch = stitchPkg;
    packages.default = tools;

    apps.gate = {
      type = "app";
      program = "${pkgs.writeShellScriptBin "phenix-gate" ''
        exec ${tools}/bin/pt gate "$@"
      ''}/bin/phenix-gate";
    };
    apps.tend = {
      type = "app";
      program = "${tendPkg}/bin/tend";
    };
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
