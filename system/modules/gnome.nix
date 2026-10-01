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

    wayland = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Use Wayland instead of X11";
    };

    extraSystemPackages = lib.mkOption {
      type = lib.types.listOf lib.types.package;
      default = [];
      description = "Additional system packages to install";
    };

    autoLoginUser = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Enable autologin";
    };
  };

  config = lib.mkIf cfg.enable {
    services.xserver = {
      enable = true;
      excludePackages = [pkgs.xterm];
    };

    services.displayManager.autoLogin.user = lib.mkIf (cfg.autoLoginUser != null) cfg.autoLoginUser;

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

    services.gnome.gnome-keyring.enable = true;
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

    # Broad font coverage so Chromium and other apps can render
    # non-Latin scripts (CJK, Arabic, Hebrew, Thai, emoji, ...).
    fonts = {
      enableDefaultPackages = true;
      packages = with pkgs; [
        nerd-fonts.jetbrains-mono
        # CJK and emoji coverage that Nerd Fonts don't provide.
        noto-fonts-cjk-sans
        noto-fonts-cjk-serif
        noto-fonts-color-emoji
      ];
      fontconfig.defaultFonts = {
        sansSerif = ["Noto Sans CJK SC"];
        serif = ["Noto Serif CJK SC"];
        monospace = ["JetBrainsMono Nerd Font" "Noto Sans Mono CJK SC"];
        emoji = ["Noto Color Emoji"];
      };
    };

    environment = {
      sessionVariables = lib.mkMerge [
        {
          XDG_RUNTIME_DIR = "/run/user/$UID";
          XDG_DATA_DIRS = ["${pkgs.gdm}/share/gsettings-schemas/gdm-${pkgs.gdm.version}"];
          GTK_USE_PORTAL = "1";
        }
        (lib.mkIf cfg.wayland {
          NIXOS_OZONE_WL = "1";
        })
      ];

      systemPackages = with pkgs;
        [
          neovim
          alsa-utils
          alsa-ucm-conf
          ghostty
          xdg-terminal-exec
          libinput
        ]
        ++ cfg.extraSystemPackages;

      etc."xdg/xdg-terminals.list".text = ''
        ghostty
      '';

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
