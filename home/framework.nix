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
      extraPackages = with pkgs; [
        technorino.packages.${stdenv.hostPlatform.system}.package
        spotify
        vlc
        slack
        zoom-us
        yubioath-flutter
        yubikey-manager
        yubikey-personalization
      ];
    };
  };

  systemd.user.services.yubikey-touch-detector = {
    Unit = {
      Description = "YubiKey touch detector";
      After = ["graphical-session.target"];
      PartOf = ["graphical-session.target"];
    };
    Service = {
      ExecStart = "${pkgs.yubikey-touch-detector}/bin/yubikey-touch-detector --libnotify";
      Restart = "on-failure";
    };
    Install.WantedBy = ["graphical-session.target"];
  };

  programs.home-manager.enable = true;
  home.stateVersion = "25.11";
}
