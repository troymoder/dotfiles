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

    cosmic-desktop.enable = true;
    cabin.enable = true;
    desktop-apps.extraPackages = [
      technorino.packages.${pkgs.stdenv.hostPlatform.system}.package
    ];
  };

  programs.home-manager.enable = true;
  home.stateVersion = "25.11";
}
