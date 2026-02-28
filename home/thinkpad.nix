{
  config,
  pkgs,
  technorino,
  ...
}: let
  variables = import ../variables.nix;
  theme = "Nightfox-Dark";
  iconPack = "Papirus-Dark";
in {
  imports = [
    ./common.nix
  ];

  services.gnome-keyring = {
    enable = true;
    components = ["pkcs11" "secrets" "ssh"];
  };

  home.packages = with pkgs; [
    (pkgs.chromium.override {
      commandLineArgs = "--force-dark-mode";
      enableWideVine = true;
    })
    code-cursor
    vesktop
    gnome-tweaks
    gnomeExtensions.user-themes
    papirus-icon-theme
    nightfox-gtk-theme
    obs-studio
    google-cloud-sdk
    technorino.packages.${stdenv.hostPlatform.system}.package
  ];

  gtk = {
    enable = true;
    theme = {
      name = theme;
    };
  };

  xdg.mimeApps = {
    enable = true;
    defaultApplications = {
      "text/html" = "chromium.desktop";
      "x-scheme-handler/http" = "chromium.desktop";
      "x-scheme-handler/https" = "chromium.desktop";
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
      # Disable defaults if they conflict
      screenshot = [];
    };

    "org/gnome/settings-daemon/plugins/media-keys" = {
      screenshot = [];
      area-screenshot = [];
    };

    "org/gnome/desktop/wm/preferences" = {
      theme = theme;
    };

    "org/gnome/desktop/interface" = {
      icon-theme = iconPack;
    };

    "org/gnome/shell/extensions/user-theme" = {
      name = theme;
    };

    "org/gnome/desktop/default-applications/terminal" = {
      exec = "ghostty";
      exec-arg = "-e";
    };

    "org/gnome/shell" = {
      disable-user-extensions = false;
      enabled-extensions = [
        pkgs.gnomeExtensions.user-themes.extensionUuid
        "blur-my-shell@aunetx"
        "just-perfection-desktop@just-perfection"
        "no-overview@fthx"
        "openbar@neuromorph"
        "tailscale-status@maxgallup.github.com"
        "Bluetooth-Battery-Meter@maniacx.github.com"
        "color-picker@tuberry"
        "do-not-disturb-while-screen-sharing-or-recording@marcinjahn.com"
        "quick-settings-audio-panel@rayzeq.github.io"
        "status-area-horizontal-spacing@mathematical.coffee.gmail.com"
        "steal-my-focus-window@steal-my-focus-window"
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

  home.stateVersion = "25.11";
}
