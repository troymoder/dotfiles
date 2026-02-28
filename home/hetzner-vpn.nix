{
  config,
  pkgs,
  ...
}: let
  variables = import ../variables.nix;
in {
  imports = [
    ./common.nix
  ];

  home.packages = with pkgs; [];

  home.stateVersion = "25.11";
}
