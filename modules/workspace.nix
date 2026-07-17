{ inputs, ... }:
{
  perSystem =
    { pkgs, system, ... }:
    let
      stitch = inputs.phenix-stitch.packages.${system}.stitch;

      workspace = pkgs.writeShellApplication {
        name = "phenix-workspace";
        runtimeInputs = [
          pkgs.coreutils
          pkgs.findutils
          pkgs.git
          pkgs.jq
          pkgs.nix
          stitch
        ];
        text = ''
          usage() {
            cat <<'EOF'
          usage: phenix-workspace [--root PATH] <command> [options]

          commands:
            init [--dry-run]              clone missing repositories and fast-forward clean ones
            sync [--dry-run]              alias for init
            clean [--apply] [--force]     remove obsolete wrapper-managed clones
            dev [NIX-DEVELOP-ARGS...]     enter the root dev shell with local flake overrides
            check [NIX-CHECK-ARGS...]     run the root flake check with local overrides
            overrides                     print generated --override-input arguments
          EOF
          }

          root_arg=""
          while [[ $# -gt 0 ]]; do
            case "$1" in
              --root)
                [[ $# -ge 2 ]] || { echo "--root requires a path" >&2; exit 2; }
                root_arg="$2"
                shift 2
                ;;
              -h|--help)
                usage
                exit 0
                ;;
              *) break ;;
            esac
          done

          command="''${1:-}"
          [[ -n "$command" ]] || { usage >&2; exit 2; }
          shift

          find_root() {
            local candidate
            if [[ -n "$root_arg" ]]; then
              candidate="$(realpath -m "$root_arg")"
            elif [[ -n "''${PHENIX_WORKSPACE_ROOT:-}" ]]; then
              candidate="$(realpath -m "$PHENIX_WORKSPACE_ROOT")"
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

            echo "cannot find a Phenix workspace root; pass --root PATH" >&2
            return 1
          }

          root="$(find_root)"

          inventory() {
            stitch workspace inventory "$root" --json
          }

          absolute_path() {
            local path="$1"
            if [[ "$path" = /* ]]; then
              realpath -m "$path"
            else
              realpath -m "$root/$path"
            fi
          }

          clone_url() {
            local remote="$1"
            case "$remote" in
              github:*) printf 'https://github.com/%s.git\n' "''${remote#github:}" ;;
              *) printf '%s\n' "$remote" ;;
            esac
          }

          remote_identity() {
            local remote="$1"
            remote="''${remote%.git}"
            remote="''${remote#github:}"
            remote="''${remote#https://github.com/}"
            remote="''${remote#ssh://git@github.com/}"
            remote="''${remote#git@github.com:}"
            printf '%s\n' "$remote"
          }

          marker_path() {
            local path="$1"
            local git_dir
            git_dir="$(git -C "$path" rev-parse --absolute-git-dir)"
            printf '%s/phenix-workspace-managed\n' "$git_dir"
          }

          mark_managed() {
            local path="$1"
            local remote="$2"
            local marker
            marker="$(marker_path "$path")"
            printf '%s\n%s\n' "$root" "$remote" > "$marker"
          }

          update_repository() {
            local name="$1"
            local path="$2"
            local remote="$3"
            local dry_run="$4"
            local expected actual branch upstream

            expected="$(remote_identity "$remote")"
            actual="$(remote_identity "$(git -C "$path" remote get-url origin)")"
            if [[ "$actual" != "$expected" ]]; then
              echo "$name: origin is $actual, expected $expected; skipped" >&2
              return 1
            fi

            if [[ -n "$(git -C "$path" status --porcelain=v1)" ]]; then
              echo "$name: dirty; skipped update"
              return 0
            fi

            if [[ "$dry_run" == true ]]; then
              echo "$name: would fetch and fast-forward"
              return 0
            fi

            git -C "$path" fetch --prune origin
            branch="$(git -C "$path" symbolic-ref --quiet --short HEAD || true)"
            if [[ -z "$branch" ]]; then
              echo "$name: detached HEAD; fetched only"
              return 0
            fi
            upstream="$(git -C "$path" rev-parse --abbrev-ref --symbolic-full-name '@{upstream}' 2>/dev/null || true)"
            if [[ -z "$upstream" ]]; then
              echo "$name: branch $branch has no upstream; fetched only"
              return 0
            fi
            git -C "$path" merge --ff-only "$upstream"
            echo "$name: updated"
          }

          init_workspace() {
            local dry_run=false
            if [[ "''${1:-}" == "--dry-run" ]]; then
              dry_run=true
              shift
            fi
            [[ $# -eq 0 ]] || { echo "unexpected init arguments: $*" >&2; exit 2; }

            local data name relative remote path url
            data="$(inventory)"
            while IFS=$'\t' read -r name relative remote; do
              [[ -n "$name" ]] || continue
              path="$(absolute_path "$relative")"
              url="$(clone_url "$remote")"

              if [[ ! -e "$path" ]]; then
                if [[ "$dry_run" == true ]]; then
                  echo "$name: would clone into $path"
                  continue
                fi
                mkdir -p "$(dirname "$path")"
                git clone --origin origin -- "$url" "$path"
                mark_managed "$path" "$remote"
                echo "$name: cloned"
              elif [[ ! -d "$path/.git" && ! -f "$path/.git" ]]; then
                echo "$name: $path exists but is not a Git repository" >&2
                return 1
              else
                update_repository "$name" "$path" "$remote" "$dry_run"
              fi
            done < <(jq -r '.[] | [.name, .path, .remote] | @tsv' <<< "$data")
          }

          clean_workspace() {
            local apply=false
            local force=false
            while [[ $# -gt 0 ]]; do
              case "$1" in
                --apply) apply=true ;;
                --force) force=true ;;
                *) echo "unknown clean option: $1" >&2; exit 2 ;;
              esac
              shift
            done

            local data relative path search_root candidate marker marker_root marker_remote actual
            data="$(inventory)"
            declare -A desired=()
            while IFS= read -r relative; do
              path="$(absolute_path "$relative")"
              desired["$path"]=1
            done < <(jq -r '.[].path' <<< "$data")

            while IFS= read -r search_root; do
              search_root="$(absolute_path "$search_root")"
              [[ -d "$search_root" ]] || continue
              while IFS= read -r candidate; do
                candidate="$(realpath -m "$candidate")"
                [[ -z "''${desired[$candidate]+x}" ]] || continue
                [[ -d "$candidate/.git" || -f "$candidate/.git" ]] || continue
                marker="$(marker_path "$candidate")"
                [[ -f "$marker" ]] || continue
                marker_root="$(sed -n '1p' "$marker")"
                marker_remote="$(sed -n '2p' "$marker")"
                [[ "$marker_root" == "$root" ]] || continue

                actual="$(remote_identity "$(git -C "$candidate" remote get-url origin)")"
                if [[ "$actual" != "$(remote_identity "$marker_remote")" ]]; then
                  echo "$candidate: origin changed; refusing removal" >&2
                  continue
                fi
                if [[ -n "$(git -C "$candidate" status --porcelain=v1)" && "$force" != true ]]; then
                  echo "$candidate: dirty; use --force with --apply" >&2
                  continue
                fi

                if [[ "$apply" == true ]]; then
                  rm -rf --one-file-system -- "$candidate"
                  echo "$candidate: removed"
                else
                  echo "$candidate: would remove"
                fi
              done < <(find "$search_root" -mindepth 1 -maxdepth 1 -type d -print)
            done < <(jq -r '.search_roots[]' "$root/.stitch-workspace.json")
          }

          local_overrides() {
            local data name relative path
            data="$(inventory)"
            while IFS=$'\t' read -r name relative; do
              [[ -n "$name" ]] || continue
              path="$(absolute_path "$relative")"
              if [[ ! -f "$path/flake.nix" ]]; then
                echo "missing local flake $name at $path; run init-workspace" >&2
                return 1
              fi
              printf '%s\0%s\0%s\0' --override-input "$name" "path:$path"
            done < <(jq -r '.[] | [.name, .path] | @tsv' <<< "$data")
          }

          run_local_nix() {
            local mode="$1"
            shift
            local -a overrides=()
            while IFS= read -r -d $'\0' value; do
              overrides+=("$value")
            done < <(local_overrides)

            case "$mode" in
              dev)
                exec nix develop "path:$root" "''${overrides[@]}" "$@"
                ;;
              check)
                exec nix flake check "path:$root" "''${overrides[@]}" "$@"
                ;;
              overrides)
                printf '%q ' "''${overrides[@]}"
                printf '\n'
                ;;
            esac
          }

          case "$command" in
            init|sync) init_workspace "$@" ;;
            clean) clean_workspace "$@" ;;
            dev) run_local_nix dev "$@" ;;
            check) run_local_nix check "$@" ;;
            overrides) run_local_nix overrides "$@" ;;
            *) usage >&2; exit 2 ;;
          esac
        '';
      };
    in
    {
      packages.phenix-workspace = workspace;
      apps.phenix-workspace = {
        type = "app";
        program = "${workspace}/bin/phenix-workspace";
      };
    };
}
