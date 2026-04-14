# Example: what framework.nix looks like now
{
  pkgs,
  technorino,
  ...
}: {
  imports = [
    ./profiles/base.nix
  ];

  modules = {
    dev-tools.enable = true;
    yubikey.enable = true;

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
