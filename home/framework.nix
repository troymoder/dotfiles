# Example: what framework.nix looks like now
{
  pkgs,
  technorino,
  ...
}: {
  modules = {
    cli-tools.enable = true;
    dev-tools.enable = true;
    fish.enable = true;
    direnv.enable = true;
    git.enable = true;
    ssh.enable = true;

    gnome-desktop = {
      enable = true;
      extraPackages = [
        technorino.packages.${pkgs.stdenv.hostPlatform.system}.package
      ];
    };
  };

  programs.home-manager.enable = true;
  home.stateVersion = "25.11";
}
