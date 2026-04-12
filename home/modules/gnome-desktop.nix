{
  config,
  lib,
  pkgs,
  variables,
  ...
}: let
  cfg = config.modules.gnome-desktop;
  inherit (lib.hm.gvariant) mkArray mkUint32 mkTuple type;

  # Single source of truth for GNOME shell extensions.
  # Add or remove an extension here and it will automatically be:
  #   - installed into home.packages
  #   - linked into ~/.local/share/gnome-shell/extensions
  #   - enabled in org/gnome/shell/enabled-extensions
  shellExtensions = with pkgs.gnomeExtensions; [
    user-themes
    blur-my-shell
    bluetooth-battery-meter
    do-not-disturb-while-screen-sharing-or-recording
    just-perfection
    open-bar
    no-overview
    quick-settings-audio-panel
    appindicator
    tailscale-status
    steal-my-focus-window
    status-area-horizontal-spacing
  ];
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

    wallpaper = lib.mkOption {
      type = lib.types.path;
      default = variables.wallpaper;
      description = "Wallpaper file to use";
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
        papirus-icon-theme
        nightfox-gtk-theme
        obs-studio
      ]
      ++ shellExtensions
      ++ cfg.extraPackages;

    # Link each extension into the user's gnome-shell extensions directory.
    xdg.dataFile = builtins.listToAttrs (map (ext: {
        name = "gnome-shell/extensions/${ext.extensionUuid}";
        value.source = "${ext}/share/gnome-shell/extensions/${ext.extensionUuid}";
      })
      shellExtensions);

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
      ## --- Background -------------------------------------------------------
      "org/gnome/desktop/background" = {
        picture-uri = "file://${cfg.wallpaper}";
        picture-uri-dark = "file://${cfg.wallpaper}";
        picture-options = "zoom";
        primary-color = "#000000";
        secondary-color = "#000000";
      };

      ## --- Lock screen background ------------------------------------------
      "org/gnome/desktop/screensaver" = {
        picture-uri = "file://${cfg.wallpaper}";
        picture-options = "zoom";
        primary-color = "#000000";
      };

      ## --- Window manager ---------------------------------------------------
      "org/gnome/desktop/wm/keybindings" = {
        switch-windows = ["<Alt>Tab"];
        switch-applications = ["<Super>Tab"];
      };

      "org/gnome/desktop/wm/preferences" = {
        theme = cfg.theme;
        button-layout = ":minimize,maximize,close";
        resize-with-right-button = false;
      };

      ## --- Shell keybindings ------------------------------------------------
      "org/gnome/shell/keybindings" = {
        show-screenshot-ui = ["<Super><Shift>s"];
        screenshot = [];
      };

      "org/gnome/settings-daemon/plugins/media-keys" = {
        screenshot = [];
        area-screenshot = [];
      };

      ## --- Interface / appearance -------------------------------------------
      "org/gnome/desktop/interface" = {
        icon-theme = cfg.iconTheme;
        gtk-theme = cfg.theme;
        color-scheme = "prefer-dark";
        accent-color = "teal";
        enable-animations = true;
        clock-format = "24h";
        clock-show-seconds = false;
        clock-show-weekday = true;
        monospace-font-name = "Monospace 11";
        show-battery-percentage = true;
      };

      "org/gnome/desktop/calendar".show-weekdate = false;
      "org/gnome/shell/extensions/user-theme".name = cfg.theme;

      ## --- Default apps -----------------------------------------------------
      "org/gnome/desktop/default-applications/terminal" = {
        exec = "ghostty";
        exec-arg = "-e";
      };

      ## --- Input ------------------------------------------------------------
      "org/gnome/desktop/input-sources" = {
        sources = [(mkTuple ["xkb" "us"])];
        xkb-options = mkArray type.string [];
        per-window = false;
      };

      "org/gnome/desktop/peripherals/touchpad".two-finger-scrolling-enabled = true;

      ## --- Notifications ----------------------------------------------------
      "org/gnome/desktop/notifications".show-banners = true;

      ## --- Search -----------------------------------------------------------
      "org/gnome/desktop/search-providers" = {
        disabled = ["org.gnome.Epiphany.desktop"];
        sort-order = [
          "org.gnome.Settings.desktop"
          "org.gnome.Contacts.desktop"
          "org.gnome.Nautilus.desktop"
        ];
      };

      ## --- Session / power --------------------------------------------------
      "org/gnome/desktop/session".idle-delay = mkUint32 0;

      "org/gnome/settings-daemon/plugins/power" = {
        power-button-action = "interactive";
        sleep-inactive-ac-timeout = 7200;
        sleep-inactive-battery-timeout = 1800;
      };

      ## --- Night light (disabled) -------------------------------------------
      "org/gnome/settings-daemon/plugins/color" = {
        night-light-enabled = false;
        night-light-schedule-automatic = false;
        night-light-temperature = mkUint32 4700;
      };

      ## --- Sound ------------------------------------------------------------
      "org/gnome/desktop/sound" = {
        event-sounds = false;
        theme-name = "__custom";
      };

      ## --- Datetime / location ----------------------------------------------
      "org/gnome/desktop/datetime".automatic-timezone = true;
      "org/gnome/system/location".enabled = true;

      ## --- Mutter -----------------------------------------------------------
      "org/gnome/mutter" = {
        attach-modal-dialogs = false;
        experimental-features = [
          "scale-monitor-framebuffer"
          "xwayland-native-scaling"
        ];
      };

      ## --- Nautilus ---------------------------------------------------------
      "org/gnome/nautilus/preferences" = {
        default-folder-viewer = "icon-view";
        migrated-gtk-settings = true;
      };

      ## --- Epiphany ---------------------------------------------------------
      "org/gnome/epiphany".ask-for-default = false;

      ## --- Tweaks -----------------------------------------------------------
      "org/gnome/tweaks".show-extensions-notice = false;

      ## --- Shell ------------------------------------------------------------
      "org/gnome/shell" = {
        disable-user-extensions = false;
        disabled-extensions = mkArray type.string [];
        enabled-extensions = map (ext: ext.extensionUuid) shellExtensions;
        favorite-apps = [
          "org.gnome.Nautilus.desktop"
          "chromium-browser.desktop"
          "code.desktop"
          "vesktop.desktop"
          "com.mitchellh.ghostty.desktop"
          "chrome-pjibgclleladliembfgfagdaldikeohf-Default.desktop"
        ];
      };

      "org/gnome/shell/extensions" = {
        disable-extension-updates = true;
      };

      ## --- Blur My Shell ----------------------------------------------------
      "org/gnome/shell/extensions/blur-my-shell".settings-version = 2;

      "org/gnome/shell/extensions/blur-my-shell/appfolder" = {
        brightness = 0.6;
        sigma = 30;
      };

      "org/gnome/shell/extensions/blur-my-shell/dash-to-dock" = {
        blur = true;
        brightness = 0.6;
        sigma = 30;
        static-blur = true;
        style-dash-to-dock = 0;
        pipeline = "pipeline_default_rounded";
      };

      "org/gnome/shell/extensions/blur-my-shell/dash-to-panel".blur-original-panel = true;

      "org/gnome/shell/extensions/blur-my-shell/panel" = {
        blur = false;
        brightness = 0.6;
        sigma = 30;
        static-blur = true;
        override-background = true;
        force-light-text = false;
        pipeline = "pipeline_default";
      };

      "org/gnome/shell/extensions/blur-my-shell/window-list" = {
        brightness = 0.6;
        sigma = 30;
      };

      "org/gnome/shell/extensions/blur-my-shell/overview".pipeline = "pipeline_default";
      "org/gnome/shell/extensions/blur-my-shell/lockscreen".pipeline = "pipeline_default";
      "org/gnome/shell/extensions/blur-my-shell/screenshot".pipeline = "pipeline_default";
      "org/gnome/shell/extensions/blur-my-shell/coverflow-alt-tab".pipeline = "pipeline_default";

      ## --- Do Not Disturb While Screen Sharing ------------------------------
      "org/gnome/shell/extensions/do-not-disturb-while-screen-sharing-or-recording".is-wayland = true;

      ## --- libpanel (Quick Settings Audio Panel layout) ---------------------
      "org/gnome/shell/extensions/libpanel".layout = [
        ["quick-settings-audio-panel@rayzeq.github.io/main"]
        ["gnome@main"]
      ];

      ## --- Open Bar ---------------------------------------------------------
      "org/gnome/shell/extensions/openbar" = {
        bartype = "Mainland";
        bg-change = false;
        bgpalette = true;
        gradient = true;
        height = 30.0;
        margin = 0.0;
        bradius = 0.0;
        bwidth = 0.0;
        balpha = 0.0;
        bgalpha = 0.0;
        bgalpha2 = 0.0;
        bgalpha-wmax = 0.0;
        boxalpha = 0.0;
        candyalpha = 0.0;
        fgalpha = 1.0;
        halpha = 0.12;
        isalpha = 0.0;
        mbalpha = 0.0;
        mbgalpha = 0.0;
        mfgalpha = 0.0;
        mhalpha = 0.0;
        msalpha = 0.0;
        mshalpha = 0.0;
        shalpha = 0.0;
        accent-color = ["0" "0.75" "0.75"];
        bcolor = ["1.0" "1.0" "1.0"];
        bgcolor = ["0.125" "0.125" "0.125"];
        bgcolor-wmax = ["0.384" "0.627" "0.918"];
        bgcolor2 = ["1.000" "1.000" "1.000"];
        boxcolor = ["0.125" "0.125" "0.125"];
        dbgcolor = ["0.137" "0.208" "0.298"];
        fgcolor = ["1.0" "1.0" "1.0"];
        hcolor = ["0" "0.7" "0.9"];
        hscd-color = ["0" "0.7" "0.75"];
        iscolor = ["1.000" "1.000" "1.000"];
        mbcolor = ["1.0" "1.0" "1.0"];
        mbgcolor = ["0.137" "0.208" "0.298"];
        mfgcolor = ["1.0" "1.0" "1.0"];
        mhcolor = ["1.000" "1.000" "1.000"];
        mscolor = ["0" "0.7" "0.75"];
        mshcolor = ["1.0" "1.0" "1.0"];
        shcolor = ["0" "0" "0"];
        smbgcolor = ["0.125" "0.125" "0.125"];
        vw-color = ["0" "0.7" "0.75"];
        winbcolor = ["0" "0.7" "0.75"];
        dark-bgcolor-wmax = ["0.384" "0.627" "0.918"];
        dark-bgcolor2 = ["1.000" "1.000" "1.000"];
        dark-dbgcolor = ["0.137" "0.208" "0.298"];
        dark-iscolor = ["1.000" "1.000" "1.000"];
        dark-mbgcolor = ["0.137" "0.208" "0.298"];
        dark-mhcolor = ["1.000" "1.000" "1.000"];
        color-scheme = "prefer-dark";
        apply-accent-shell = false;
        apply-all-shell = false;
        apply-menu-notif = false;
        apply-menu-shell = false;
        auto-bgalpha = false;
        autofg-bar = false;
        autofg-menu = false;
        autohg-bar = true;
        autohg-menu = false;
        autotheme-refresh = false;
        cust-margin-wmax = false;
        dashdock-style = "Default";
        dborder = false;
        default-font = "Sans 12";
        dshadow = false;
        fitts-widgets = false;
        handle-border = 0.0;
        heffect = false;
        import-export = false;
        margin-wmax = 0.0;
        mbg-gradient = false;
        menu-radius = 0.0;
        menustyle = false;
        neon = false;
        notif-radius = 0.0;
        pause-reload = false;
        qtoggle-radius = 0.0;
        reloadstyle = true;
        set-bottom-margin = false;
        set-fullscreen = false;
        slider-height = 1.0;
        smbgoverride = false;
        trigger-reload = true;
        vpad = 0.0;
        wmaxbar = false;
        bottom-margin = 0.0;
      };

      ## --- AppIndicator -----------------------------------------------------
      "org/gnome/shell/extensions/appindicator" = {
        icon-brightness = 0.0;
        icon-contrast = 0.0;
        icon-opacity = 240;
        icon-saturation = 0.0;
        icon-size = 0;
      };

      ## --- Quick Settings Audio Panel ---------------------------------------
      "org/gnome/shell/extensions/quick-settings-audio-panel".version = 2;

      "org/gnome/desktop/notifications/application/org-gnome-extensions" = {
        enable = false;
      };
    };
  };
}
