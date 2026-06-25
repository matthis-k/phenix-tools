{ ... }: {
  perSystem = { phenixPackages, ... }: {
    phenix.overlays = [(final: prev: {
      phenix = (prev.phenix or {}) // {
        hello-tools = final.writeShellScriptBin "hello-tools" ''
          echo "hello from tools"
        '';
      };
    })];

    packages.hello-all-shell = phenixPackages.hello-shell or null;
  };
}
