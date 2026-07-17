{ inputs, ... }:
{
  perSystem =
    { system, ... }:
    {
      phenixWrapped.stitch = inputs.phenix-tools.packages.${system}.stitch;
    };
}
