{
  description = "Phenix cross-repo developer and maintenance tooling";

  inputs = {
    phenix-pins.url = "github:matthis-k/phenix-pins";
    nixpkgs.follows = "phenix-pins/nixpkgs";
  };

  outputs = inputs: {
    packages.x86_64-linux.sync = (import inputs.nixpkgs {
      system = "x86_64-linux";
    }).writeShellApplication {
      name = "phenix-sync";
      text = ''
        echo "phenix-sync: TODO implement sync tool"
        echo "Usage: sync [graph|plan|check]"
      '';
    };

    packages.x86_64-linux.default = inputs.self.packages.x86_64-linux.sync;

    apps.x86_64-linux.sync = {
      type = "app";
      program = "${inputs.self.packages.x86_64-linux.sync}/bin/phenix-sync";
    };

    apps.x86_64-linux.default = inputs.self.apps.x86_64-linux.sync;
  };
}
