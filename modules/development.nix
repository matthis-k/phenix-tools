{ inputs, ... }:
{
  perSystem =
    {
      config,
      pkgs,
      ...
    }:
    let
      maintenanceLib = inputs.phenix-flake-ci.lib;
      workspace = config.packages.phenix-workspace;
      repositoryRoot = ''
        repo_root="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
        cd "$repo_root"
      '';

      phenixDev = pkgs.writeShellApplication {
        name = "phenix-dev";
        runtimeInputs = [
          pkgs.coreutils
          workspace
        ];
        text = ''
          find_root() {
            local candidate
            if [[ -n "''${PHENIX_ROOT:-}" ]]; then
              candidate="$(realpath -m "$PHENIX_ROOT")"
            else
              candidate="$(realpath -m "$PWD")"
            fi

            while [[ "$candidate" != "/" ]]; do
              if [[ -f "$candidate/.stitch-workspace.json" && -f "$candidate/flake.nix" ]]; then
                printf '%s\n' "$candidate"
                return 0
              fi
              candidate="$(dirname "$candidate")"
            done

            echo "cannot find a Phenix workspace root; set PHENIX_ROOT or run inside the workspace" >&2
            return 1
          }

          PHENIX_ROOT="$(find_root)"
          export PHENIX_ROOT
          export PHENIX_DEV=1

          exec phenix-workspace --root "$PHENIX_ROOT" dev "$@"
        '';
      };

      sourceCi = {
        enable = true;
        stage = "source";
        name = "Source";
        timeoutMinutes = 30;
      };
      productCi = {
        enable = true;
        stage = "product";
        name = "Product";
        timeoutMinutes = 45;
        needs = [ "source" ];
      };

      maintenance = maintenanceLib.mkMaintenance {
        name = "maintenance";
        description = "Phenix tools maintenance";
        ci.github = {
          enable = true;
          outputName = "phenix-maintenance";
        };
        gitHooks = {
          enable = true;
          preCommit = [ "fix" ];
        };
        commands = {
          all = {
            description = "Run the complete validation graph";
            exec = ''
              "$0" check
              "$0" test
            '';
          };

          check = {
            description = "Run source validation";
            order = [
              "nix-format"
              "statix"
              "deadnix"
              "actionlint"
              "boundary"
              "workflow-sync"
            ];
            commands = {
              nix-format = {
                description = "Nix formatting";
                ci = sourceCi // { stepName = "Nix formatting"; };
                runtimeInputs = pkgs: [ pkgs.findutils pkgs.git pkgs.nixfmt ];
                exec = ''
                  ${repositoryRoot}
                  find . -type f -name '*.nix' -not -path './.git/*' -print0 |
                    xargs -0 -r nixfmt --check
                '';
              };
              statix = {
                description = "Nix static analysis";
                ci = sourceCi // { stepName = "Statix"; };
                runtimeInputs = pkgs: [ pkgs.git pkgs.statix ];
                exec = ''
                  ${repositoryRoot}
                  statix check --ignore '.git/**'
                '';
              };
              deadnix = {
                description = "Unused Nix code";
                ci = sourceCi // { stepName = "Deadnix"; };
                runtimeInputs = pkgs: [ pkgs.deadnix pkgs.git ];
                exec = ''
                  ${repositoryRoot}
                  deadnix --fail --no-lambda-arg --no-lambda-pattern-names
                '';
              };
              actionlint = {
                description = "GitHub Actions syntax";
                ci = sourceCi // { stepName = "Actionlint"; };
                runtimeInputs = pkgs: [ pkgs.actionlint pkgs.findutils pkgs.git ];
                exec = ''
                  ${repositoryRoot}
                  find .github/workflows -type f \( -name '*.yml' -o -name '*.yaml' \) -print0 |
                    xargs -0 -r actionlint
                '';
              };
              boundary = {
                description = "Keep phenix-tools a thin Nix aggregator";
                ci = sourceCi // { stepName = "Repository boundary"; };
                runtimeInputs = pkgs: [ pkgs.coreutils pkgs.git ];
                exec = ''
                  ${repositoryRoot}
                  test ! -d crates
                  test ! -e Cargo.toml
                  test ! -e Cargo.lock
                '';
              };
              workflow-sync = {
                description = "Committed workflow matches the maintenance declaration";
                ci = sourceCi // { stepName = "Generated workflow"; };
                runtimeInputs = pkgs: [ pkgs.diffutils pkgs.git pkgs.nix ];
                exec = ''
                  ${repositoryRoot}
                  system="$(nix eval --impure --raw --expr builtins.currentSystem)"
                  generated="$(mktemp)"
                  trap 'rm -f "$generated"' EXIT
                  nix eval --raw ".#packages.$system.phenix-maintenance.phenixMaintenance.ci.github.workflow" > "$generated"
                  diff -u .github/workflows/ci.yml "$generated"
                '';
              };
            };
          };

          test = {
            description = "Run functional tool integration tests";
            order = [ "phenix-dev" ];
            commands.phenix-dev = {
              description = "Exercise workspace discovery and nix develop through phenix-dev";
              ci = productCi // { stepName = "Phenix dev workspace flow"; };
              runtimeInputs = pkgs: [ phenixDev pkgs.git pkgs.nix ];
              exec = ''
                tmp="$(mktemp -d)"
                trap 'rm -rf "$tmp"' EXIT
                workspace_root="$tmp/workspace"
                mkdir -p "$workspace_root/nested" "$workspace_root/repos"

                cat > "$workspace_root/.stitch-workspace.json" <<'EOF'
                {
                  "owner": "matthis-k",
                  "repository_pattern": "functional-test-no-match-*",
                  "search_roots": ["repos"]
                }
                EOF

                cat > "$workspace_root/flake.nix" <<EOF
                {
                  inputs.nixpkgs.url = "path:${pkgs.path}";
                  outputs = { nixpkgs, ... }: {
                    devShells.${pkgs.system}.default = nixpkgs.legacyPackages.${pkgs.system}.mkShell {
                      shellHook = ''
                        test "\$PHENIX_DEV" = 1
                        test "\$PHENIX_ROOT" = "$workspace_root"
                        touch "$workspace_root/entered"
                      '';
                    };
                  };
                }
                EOF

                (cd "$workspace_root" && nix flake lock)
                (cd "$workspace_root/nested" && phenix-dev --command true)
                test -e "$workspace_root/entered"

                rm "$workspace_root/entered"
                (cd "$tmp" && PHENIX_ROOT="$workspace_root" phenix-dev --command true)
                test -e "$workspace_root/entered"

                if (cd "$tmp" && unset PHENIX_ROOT && phenix-dev --command true >/dev/null 2>&1); then
                  echo "phenix-dev unexpectedly succeeded outside a workspace" >&2
                  exit 1
                fi
              '';
            };
          };

          fix = {
            description = "Apply deterministic Nix normalization";
            runtimeInputs = pkgs: [ pkgs.deadnix pkgs.findutils pkgs.git pkgs.nixfmt pkgs.statix ];
            exec = ''
              ${repositoryRoot}
              statix fix
              deadnix --edit --no-lambda-arg --no-lambda-pattern-names
              find . -type f -name '*.nix' -not -path './.git/*' -print0 |
                xargs -0 -r nixfmt
            '';
          };
        };
      };

      maintenancePackage = maintenanceLib.mkMaintenancePackage {
        inherit pkgs maintenance;
      };
    in
    {
      packages = {
        phenix-dev = phenixDev;
        phenix-maintenance = maintenancePackage.package;
      };
      apps = {
        phenix-dev = {
          type = "app";
          program = "${phenixDev}/bin/phenix-dev";
        };
        phenix-maintenance = maintenancePackage.app;
      };

      devShells.maintenance = pkgs.mkShell {
        name = "phenix-tools-maintenance";
        packages = [ pkgs.git pkgs.nix maintenancePackage.package ];
        shellHook = maintenancePackage.shellHook;
      };
    };
}
