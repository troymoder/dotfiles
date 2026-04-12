{
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.modules.gnome-desktop;
in {
  options.modules.gnome-desktop = {
    enable = lib.mkEnableOption "GNOME desktop home config";

    theme = lib.mkOption {
      type = lib.types.str;
      default = "Nightfox-Dark";
      description = "GTK and GNOME shell theme name";
    };

    iconTheme = lib.mkOption {
      type = lib.types.str;
      default = "Papirus-Dark";
      description = "Icon theme name";
    };

    browser = lib.mkOption {
      type = lib.types.str;
      default = "chromium";
      description = "Default browser desktop file name (without .desktop)";
    };

    extraPackages = lib.mkOption {
      type = lib.types.listOf lib.types.package;
      default = [];
      description = "Additional desktop packages";
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = with pkgs;
      [
        (chromium.override {
          commandLineArgs = "--force-dark-mode";
          enableWideVine = true;
        })
        vscode
        vesktop
        gnome-tweaks
        gnomeExtensions.user-themes
        gnomeExtensions.blur-my-shell
        gnomeExtensions.bluetooth-battery-meter
        gnomeExtensions.do-not-disturb-while-screen-sharing-or-recording
        gnomeExtensions.just-perfection
        gnomeExtensions.open-bar
        gnomeExtensions.no-overview
        gnomeExtensions.quick-settings-audio-panel
        gnomeExtensions.appindicator
        gnomeExtensions.tailscale-status
        papirus-icon-theme
        nightfox-gtk-theme
        obs-studio
      ]
      ++ cfg.extraPackages;

    xdg.dataFile = let
      extensionPkgs = with pkgs; [
        gnomeExtensions.user-themes
        gnomeExtensions.blur-my-shell
        gnomeExtensions.bluetooth-battery-meter
        gnomeExtensions.do-not-disturb-while-screen-sharing-or-recording
        gnomeExtensions.just-perfection
        gnomeExtensions.open-bar
        gnomeExtensions.no-overview
        gnomeExtensions.quick-settings-audio-panel
        gnomeExtensions.appindicator
        gnomeExtensions.tailscale-status
      ];
    in
      builtins.listToAttrs (map (ext: {
          name = "gnome-shell/extensions/${ext.extensionUuid}";
          value = {source = "${ext}/share/gnome-shell/extensions/${ext.extensionUuid}";};
        })
        extensionPkgs);

    services.gnome-keyring = {
      enable = true;
      components = ["pkcs11" "secrets" "ssh"];
    };

    gtk = {
      enable = true;
      theme.name = cfg.theme;
    };

    xdg.mimeApps = {
      enable = true;
      defaultApplications = {
        "text/html" = "${cfg.browser}.desktop";
        "x-scheme-handler/http" = "${cfg.browser}.desktop";
        "x-scheme-handler/https" = "${cfg.browser}.desktop";
      };
    };

    xdg.configFile."mimeapps.list".force = true;
    xdg.configFile."gtk-3.0/settings.ini".force = true;
    xdg.configFile."gtk-4.0/settings.ini".force = true;

    dconf.settings = {
      "org/gnome/desktop/wm/keybindings" = {
        switch-windows = ["<Alt>Tab"];
        switch-applications = ["<Super>Tab"];
      };

      "org/gnome/shell/keybindings" = {
        show-screenshot-ui = ["<Super><Shift>s"];
        screenshot = [];
      };

      "org/gnome/settings-daemon/plugins/media-keys" = {
        screenshot = [];
        area-screenshot = [];
      };

      "org/gnome/desktop/wm/preferences" = {
        theme = cfg.theme;
      };

      "org/gnome/desktop/interface" = {
        icon-theme = cfg.iconTheme;
      };

      "org/gnome/shell/extensions/user-theme" = {
        name = cfg.theme;
      };

      "org/gnome/desktop/default-applications/terminal" = {
        exec = "ghostty";
        exec-arg = "-e";
      };

      "org/gnome/shell" = {
        disable-user-extensions = false;
        enabled-extensions = lib.hm.gvariant.mkArray lib.hm.gvariant.type.string [
          pkgs.gnomeExtensions.user-themes.extensionUuid
          pkgs.gnomeExtensions.blur-my-shell.extensionUuid
          pkgs.gnomeExtensions.bluetooth-battery-meter.extensionUuid
          pkgs.gnomeExtensions.do-not-disturb-while-screen-sharing-or-recording.extensionUuid
          pkgs.gnomeExtensions.just-perfection.extensionUuid
          pkgs.gnomeExtensions.open-bar.extensionUuid
          pkgs.gnomeExtensions.no-overview.extensionUuid
          pkgs.gnomeExtensions.quick-settings-audio-panel.extensionUuid
          pkgs.gnomeExtensions.appindicator.extensionUuid
          pkgs.gnomeExtensions.tailscale-status.extensionUuid
        ];
      };

      "org/gnome/desktop/sound" = {
        event-sounds = false;
      };

      "org/gnome/desktop/datetime" = {
        automatic-timezone = true;
      };

      "org/gnome/system/location" = {
        enabled = true;
      };
    };
  };
}
