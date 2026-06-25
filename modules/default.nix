{ ... }: {
  perSystem = { config, pkgs, ... }: let
    tools = pkgs.rustPlatform.buildRustPackage {
      pname = "phenix-tools";
      version = "0.1.0";
      src = ../.;
      cargoLock.lockFile = ../Cargo.lock;
    };
  in {
    packages.sync = tools;
    packages.default = tools;

    apps.sync = {
      type = "app";
      program = "${pkgs.writeShellScriptBin "phenix-sync" ''
        exec ${tools}/bin/pt sync "$@"
      ''}/bin/phenix-sync";
    };
    apps.default = {
      type = "app";
      program = "${tools}/bin/pt";
    };
  };
}
