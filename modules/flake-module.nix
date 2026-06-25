{ ... }: {
  phenix.overlays = [(final: prev: {
    phenix = (prev.phenix or {}) // {
      hello-tools = final.writeShellScriptBin "hello-tools" ''
        echo "hello from tools"
      '';
    };
  })];
}
