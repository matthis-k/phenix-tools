_: {
  perSystem =
    {
      config,
      pkgs,
      ...
    }:
    let
      workspace = config.packages.phenix-workspace;
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
      interfaceCheck = pkgs.runCommand "phenix-dev-interface" { } ''
        grep -Fq 'PHENIX_ROOT="$(find_root)"' ${phenixDev}/bin/phenix-dev
        grep -Fq 'export PHENIX_ROOT' ${phenixDev}/bin/phenix-dev
        grep -Fq 'export PHENIX_DEV=1' ${phenixDev}/bin/phenix-dev
        grep -Fq 'phenix-workspace --root "$PHENIX_ROOT" dev' ${phenixDev}/bin/phenix-dev
        touch "$out"
      '';
    in
    {
      packages.phenix-dev = phenixDev;
      apps.phenix-dev = {
        type = "app";
        program = "${phenixDev}/bin/phenix-dev";
      };
      checks.phenix-dev-interface = interfaceCheck;
    };
}
