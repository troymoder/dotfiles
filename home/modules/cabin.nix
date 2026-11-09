# Cabin: Troy's panels for COSMIC (quick settings, the date menu, the System
# panel), laid out once and for all. Built from ./cabin, this repo's own
# crate. cabind runs with the graphical session and draws every panel; the
# panel's buttons (cabin-applet) and cabinctl reach it on the session bus.
{
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.modules.cabin;
  cabin = pkgs.callPackage ./cabin/package.nix {};
in {
  options.modules.cabin = {
    enable = lib.mkEnableOption "Cabin, the COSMIC panels";
    package = lib.mkOption {
      type = lib.types.package;
      default = cabin;
      readOnly = true;
      description = "The built crate: the daemon, the panel buttons, cabinctl, the graphics logout dialog and the pinentry.";
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = [cabin];

    systemd.user.services.cabind = {
      Unit = {
        Description = "Cabin: the COSMIC panels' daemon";
        PartOf = ["graphical-session.target"];
        After = ["graphical-session.target"];
      };
      Service = {
        Type = "dbus";
        BusName = "dev.troy.Cabin";
        ExecStart = "${cabin}/bin/cabind";
        Restart = "on-failure";
        RestartSec = 2;
      };
      Install.WantedBy = ["graphical-session.target"];
    };

    # The session bus starts it (through systemd) on the first call, should
    # a panel button ask before the session has.
    xdg.dataFile."dbus-1/services/dev.troy.Cabin.service".source = "${cabin}/share/dbus-1/services/dev.troy.Cabin.service";
  };
}
