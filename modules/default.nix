{ ... }: {
  perSystem = { config, pkgs, ... }: let
    stitchCliPkg = pkgs.rustPlatform.buildRustPackage {
      pname = "stitch";
      version = "0.1.0";
      src = ../.;
      cargoLock.lockFile = ../Cargo.lock;
      cargoBuildFlags = "-p stitch-cli";
    };
  in {
    packages.stitch = stitchCliPkg;

    apps.stitch = {
      type = "app";
      program = "${stitchCliPkg}/bin/stitch";
    };
    apps.default = {
      type = "app";
      program = "${stitchCliPkg}/bin/stitch";
    };
  };
}
