{ pkgs, ... }:
let
  root = ''repo_root="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"; cd "$repo_root"'';
  nixSources = "find . -type f -name '*.nix' -not -path './.git/*' -not -path './.devenv/*'";
in
{
  scripts = {
    "maintenance-check-format" = {
      packages = [
        pkgs.findutils
        pkgs.git
        pkgs.nixfmt
      ];
      exec = "${root}; ${nixSources} -exec nixfmt --check {} +";
    };
    "maintenance-check-statix" = {
      packages = [
        pkgs.git
        pkgs.statix
      ];
      exec = "${root}; statix check --ignore '.git/**' ";
    };
    "maintenance-check-deadnix" = {
      packages = [
        pkgs.deadnix
        pkgs.git
      ];
      exec = "${root}; deadnix --fail --no-lambda-arg --no-lambda-pattern-names";
    };
    "maintenance-check-flake" = {
      packages = [
        pkgs.git
        pkgs.nix
      ];
      exec = "${root}; nix flake check --print-build-logs --keep-going";
    };
    "maintenance-check-boundary" = {
      packages = [
        pkgs.git
        pkgs.coreutils
      ];
      exec = ''
        ${root}
        test ! -d crates
        test ! -e Cargo.toml
        test ! -e Cargo.lock
      '';
    };
    "maintenance-fix-statix" = {
      packages = [
        pkgs.git
        pkgs.statix
      ];
      exec = "${root}; statix fix";
    };
    "maintenance-fix-deadnix" = {
      packages = [
        pkgs.deadnix
        pkgs.git
      ];
      exec = "${root}; deadnix --edit --no-lambda-arg --no-lambda-pattern-names";
    };
    "maintenance-fix-format" = {
      packages = [
        pkgs.findutils
        pkgs.git
        pkgs.nixfmt
      ];
      exec = "${root}; ${nixSources} -exec nixfmt {} +";
    };
  };

  tasks = {
    "maintenance:format".exec = "maintenance-check-format";
    "maintenance:statix".exec = "maintenance-check-statix";
    "maintenance:deadnix".exec = "maintenance-check-deadnix";
    "maintenance:flake".exec = "maintenance-check-flake";
    "maintenance:boundary".exec = "maintenance-check-boundary";
    "maintenance:check" = {
      exec = "true";
      after = [
        "maintenance:format"
        "maintenance:statix"
        "maintenance:deadnix"
        "maintenance:flake"
        "maintenance:boundary"
      ];
      before = [ "devenv:enterTest" ];
    };
    "maintenance:fix:statix".exec = "maintenance-fix-statix";
    "maintenance:fix:deadnix" = {
      exec = "maintenance-fix-deadnix";
      after = [ "maintenance:fix:statix" ];
    };
    "maintenance:fix:format" = {
      exec = "maintenance-fix-format";
      after = [ "maintenance:fix:deadnix" ];
    };
    "maintenance:fix" = {
      exec = "true";
      after = [ "maintenance:fix:format" ];
    };
  };
}
