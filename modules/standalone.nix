{ lib, ... }: {
  perSystem =
    { config, pkgs, ... }:
    let
      filteredSrc = lib.cleanSource ../.;

      tendCliPkg = pkgs.rustPlatform.buildRustPackage {
        pname = "tend";
        version = "0.1.0";
        src = filteredSrc;
        cargoLock.lockFile = ../Cargo.lock;
        cargoBuildFlags = "-p tend-cli";
        nativeBuildInputs = [ pkgs.git ];
      };
      stitchCliPkg = pkgs.rustPlatform.buildRustPackage {
        pname = "stitch";
        version = "0.1.0";
        src = filteredSrc;
        cargoLock.lockFile = ../Cargo.lock;
        cargoBuildFlags = "-p stitch-cli";
        nativeBuildInputs = [ pkgs.git ];
      };

      tendMcpPkg = pkgs.rustPlatform.buildRustPackage {
        pname = "tend-mcp";
        version = "0.1.0";
        src = filteredSrc;
        cargoLock.lockFile = ../Cargo.lock;
        cargoBuildFlags = "-p tend-mcp";
        nativeBuildInputs = [ pkgs.git ];
      };
      stitchMcpPkg = pkgs.rustPlatform.buildRustPackage {
        pname = "stitch-mcp";
        version = "0.1.0";
        src = filteredSrc;
        cargoLock.lockFile = ../Cargo.lock;
        cargoBuildFlags = "-p stitch-mcp";
        nativeBuildInputs = [ pkgs.git ];
      };

      # Reuse vendored crate dependencies from any buildRustPackage.
      cargoDeps = tendCliPkg.cargoDeps or (throw "cargoDeps not found");

      mkCargoCheck =
        name: description: cargoArgs: extraNativeBuildInputs:
        pkgs.runCommand name
          {
            nativeBuildInputs = extraNativeBuildInputs ++ [ pkgs.stdenv.cc ];
            inherit cargoDeps;
            src = filteredSrc;
          }
          ''
            export HOME=$TMPDIR/home
            mkdir -p $HOME
            export CARGO_HOME=$TMPDIR/cargo
            export CARGO_TARGET_DIR=$TMPDIR/target
            mkdir -p $CARGO_HOME $CARGO_TARGET_DIR

            cp -rT $src source
            chmod -R u+w source
            cd source

            # Point cargo at the vendored dependencies
            mkdir -p .cargo
            cat > .cargo/config.toml <<EOF
            [source.crates-io]
            replace-with = "vendored-sources"

            [source.vendored-sources]
            directory = "${cargoDeps}"
            EOF

            ${cargoArgs}

            touch $out
          '';
    in
    {
      packages = {
        inherit
          tendCliPkg
          stitchCliPkg
          tendMcpPkg
          stitchMcpPkg
          ;
        tend = tendCliPkg;
        stitch = stitchCliPkg;
        tend-mcp = tendMcpPkg;
        stitch-mcp = stitchMcpPkg;
        default = tendCliPkg;

        # Wrapper package with all tools on PATH for the local gate
        check = pkgs.writeShellApplication {
          name = "phenix-tools-check";
          runtimeInputs = [
            pkgs.cargo
            pkgs.rustc
            pkgs.rustfmt
            pkgs.clippy
            pkgs.git
            tendCliPkg
          ];
          text = ''
            set -euo pipefail
            echo "=== cargo fmt ==="
            cargo fmt --all --check
            echo "=== cargo check ==="
            cargo check --workspace --all-targets
            echo "=== cargo clippy ==="
            cargo clippy --quiet --workspace --all-targets -- -D warnings
            echo "=== cargo test ==="
            cargo test --workspace
            echo "=== tend gate ==="
            tend run --mode full --phase verify
            echo "=== ALL CHECKS PASSED ==="
          '';
        };
      };

      checks = {
        cargo-check =
          mkCargoCheck "phenix-tools-cargo-check" "cargo check --workspace --all-targets"
            "cargo check --workspace --all-targets"
            [
              pkgs.cargo
              pkgs.rustc
            ];

        cargo-test =
          mkCargoCheck "phenix-tools-cargo-test" "cargo test --workspace" "cargo test --workspace"
            [
              pkgs.cargo
              pkgs.rustc
              pkgs.git
            ];

        cargo-fmt =
          mkCargoCheck "phenix-tools-cargo-fmt" "cargo fmt --all --check" "cargo fmt --all --check"
            [
              pkgs.cargo
              pkgs.rustfmt
            ];

        cargo-clippy =
          mkCargoCheck "phenix-tools-cargo-clippy"
            "cargo clippy --quiet --workspace --all-targets -- -D warnings"
            "cargo clippy --quiet --workspace --all-targets -- -D warnings"
            [
              pkgs.cargo
              pkgs.clippy
              pkgs.rustc
            ];

        tend-gate =
          pkgs.runCommand "phenix-tools-tend-gate"
            {
              nativeBuildInputs = [
                tendCliPkg
                pkgs.git
                pkgs.cargo
                pkgs.rustc
                pkgs.rustfmt
                pkgs.clippy
                pkgs.nixfmt
                pkgs.statix
                pkgs.deadnix
                pkgs.stdenv.cc
              ];
              inherit cargoDeps;
              src = filteredSrc;
            }
            ''
              export HOME=$TMPDIR/home
              mkdir -p $HOME
              export CARGO_HOME=$TMPDIR/cargo
              export CARGO_TARGET_DIR=$TMPDIR/target
              mkdir -p $CARGO_HOME $CARGO_TARGET_DIR

              cp -rT $src source
              chmod -R u+w source
              cd source

              # Point cargo at vendored dependencies
              mkdir -p .cargo
              cat > .cargo/config.toml <<EOF
              [source.crates-io]
              replace-with = "vendored-sources"

              [source.vendored-sources]
              directory = "${cargoDeps}"
              EOF

              # git is needed by tend for changed-file detection
              git init && git add -A

              tend run --mode full --phase verify --profile nix-check

              touch $out
            '';
      };

      apps = {
        tend = {
          type = "app";
          program = "${tendCliPkg}/bin/tend";
        };
        stitch = {
          type = "app";
          program = "${stitchCliPkg}/bin/stitch";
        };
        tend-mcp = {
          type = "app";
          program = "${tendMcpPkg}/bin/tend-mcp";
        };
        stitch-mcp = {
          type = "app";
          program = "${stitchMcpPkg}/bin/stitch-mcp";
        };
        default = {
          type = "app";
          program = "${tendCliPkg}/bin/tend";
        };
        check =
          let
            checkApp = pkgs.writeShellApplication {
              name = "phenix-tools-check-app";
              runtimeInputs = [
                pkgs.cargo
                pkgs.rustc
                pkgs.rustfmt
                pkgs.clippy
                pkgs.git
                tendCliPkg
              ];
              text = ''
                set -euo pipefail
                echo "=== cargo fmt ==="
                cargo fmt --all --check
                echo "=== cargo check ==="
                cargo check --workspace --all-targets
                echo "=== cargo clippy ==="
                cargo clippy --quiet --workspace --all-targets -- -D warnings
                echo "=== cargo test ==="
                cargo test --workspace
                echo "=== tend gate ==="
                tend run --mode full --phase verify
                echo "=== ALL CHECKS PASSED ==="
              '';
            };
          in
          {
            type = "app";
            program = "${checkApp}/bin/phenix-tools-check-app";
          };
      };

      devShells.default = pkgs.mkShell {
        name = "phenix-tools-dev";
        packages = [
          pkgs.cargo
          pkgs.rustc
          pkgs.rustfmt
          pkgs.clippy
          pkgs.rust-analyzer
          pkgs.git
          pkgs.nix
          tendCliPkg
          stitchCliPkg
        ];
        shellHook = ''
          echo "phenix-tools dev shell"
          echo "  cargo: $(cargo --version 2>/dev/null || echo '?')"
          echo "  rustc: $(rustc --version 2>/dev/null || echo '?')"
          echo "  tend:  $(tend --version 2>/dev/null || echo '?')"
          echo "  stitch: $(stitch --version 2>/dev/null || echo '?')"
        '';
      };

      devShells.test = pkgs.mkShell {
        name = "phenix-tools-test";

        packages = [
          pkgs.git
          pkgs.nix
          pkgs.jq
          tendCliPkg
          stitchCliPkg
        ];
      };
    };
}
