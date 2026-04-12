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
      type = lib.types.str;
      default = null;
      description = "Enable autologin";
    };
  };

  config = lib.mkIf cfg.enable {
    services.xserver = {
      enable = true;
      excludePackages = [pkgs.xterm];
    };

    services.displayManager.autoLogin.user = cfg.autoLoginUser;

    services.displayManager.gdm = {
      enable = true;
      wayland = cfg.wayland;
    };

    services.desktopManager.gnome = {
      enable = true;
      extraGSettingsOverrides = ''
        [org.gnome.mutter]
        experimental-features=['scale-monitor-framebuffer', 'xwayland-native-scaling']
      '';
    };

    services.gnome.gnome-keyring.enable = true;
    security.pam.services.gdm.enableGnomeKeyring = true;

    environment = {
      sessionVariables = lib.mkMerge [
        {
          XDG_RUNTIME_DIR = "/run/user/$UID";
          XDG_DATA_DIRS = ["${pkgs.gdm}/share/gsettings-schemas/gdm-${pkgs.gdm.version}"];
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
