{
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.modules.yubikey;
in {
  options.modules.yubikey = {
    enable = lib.mkEnableOption "enable yubikey";
  };

  config = lib.mkIf cfg.enable {
    home.packages = with pkgs; [
      yubioath-flutter
      yubikey-manager
      yubikey-personalization
    ];

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
  };
}
