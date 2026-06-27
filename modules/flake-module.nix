{ inputs, ... }: {
  perSystem = { system, ... }: {
    phenixWrapped = {
      tend = inputs.phenix-tools.packages.${system}.tend;
      stitch = inputs.phenix-tools.packages.${system}.stitch;
    };
  };
}
