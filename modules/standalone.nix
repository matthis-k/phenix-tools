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
  in {
    packages.sync = tools;
    packages.gate = tools;
    packages.tend = tendPkg;
    packages.default = tools;

    apps.sync = {
      type = "app";
      program = "${pkgs.writeShellScriptBin "phenix-sync" ''
        exec ${tools}/bin/pt sync "$@"
      ''}/bin/phenix-sync";
    };
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
    apps.default = {
      type = "app";
      program = "${tools}/bin/pt";
    };
  };
}
