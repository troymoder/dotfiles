{
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.modules.gnome;
in {
  options.modules.gnome = {
    enable = lib.mkEnableOption "GNOME desktop environment";
  };

  config = lib.mkIf cfg.enable {
    services.xserver = {
      enable = true;
      excludePackages = [pkgs.xterm];
    };

    services.displayManager.gdm = {
      enable = true;
    };

    services.desktopManager.gnome = {
      enable = true;
      extraGSettingsOverrides = ''
        [org.gnome.mutter]
        experimental-features=['scale-monitor-framebuffer', 'xwayland-native-scaling']
      '';
    };

    security.pam.services.gdm.enableGnomeKeyring = false;

    # GDM 50.2's pam_gdm rejects the LUKS passphrase that systemd-cryptsetup
    # caches in the kernel keyring (systemd omits the trailing NUL, pam_gdm now
    # requires it), so the login keyring stopped auto-unlocking on autologin.
    # systemd's own module reads the same key correctly.
    security.pam.services.gdm-autologin.rules.auth.gdm.modulePath =
      lib.mkForce "${config.systemd.package}/lib/security/pam_systemd_loadkey.so";
    security.pam.services.gdm-fingerprint.rules.auth.gdm.modulePath =
      lib.mkIf config.services.fprintd.enable
      (lib.mkForce "${config.systemd.package}/lib/security/pam_systemd_loadkey.so");

    environment = {
      sessionVariables.XDG_DATA_DIRS = ["${pkgs.gdm}/share/gsettings-schemas/gdm-${pkgs.gdm.version}"];

      gnome.excludePackages = with pkgs; [
        epiphany
        geary
        gnome-music
        totem
        cheese
        gnome-contacts
        gnome-maps
        gnome-calendar
        gnome-weather
        gnome-clocks
        gnome-tour
        gnome-characters
        gnome-font-viewer
        gnome-connections
        gnome-terminal
        gnome-console
        snapshot
        gnome-chess
        gnome-mahjongg
        gnome-mines
        gnome-sudoku
        gnome-tetravex
        iagno
        hitori
        atomix
        aisleriot
        tali
      ];
    };
  };
}
