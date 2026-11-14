{
  config,
  lib,
  pkgs,
  variables,
  ...
}: let
  cfg = config.modules.cosmic-desktop;

  # COSMIC stores settings as RON files under ~/.config/cosmic/<component>/v1/<key>.
  # cosmic-settings rewrites these in place, so they are forced back on every
  # home-manager activation.
  cosmicConfig = lib.mapAttrs' (name: text: {
    name = "cosmic/${name}";
    value = {
      inherit text;
      force = true;
    };
  });
in {
  options.modules.cosmic-desktop = {
    enable = lib.mkEnableOption "COSMIC desktop home config";
  };

  config = lib.mkIf cfg.enable {
    modules.desktop-apps.enable = true;

    xdg.configFile =
      cosmicConfig {
        "com.system76.CosmicTheme.Mode/v1/is_dark" = "true";
        # libcosmic only follows one level of icon-theme inheritance (Cosmic -> Pop), so
        # Adwaita-only icons never resolve. A shim theme lists all three as direct parents.
        "com.system76.CosmicTk/v1/icon_theme" = ''"CosmicAdwaita"'';
        # The top panel only: the dock is Cabin's, in the workspace overview
        # (pinned apps from CosmicAppList's favorites below, then running ones).
        "com.system76.CosmicPanel/v1/entries" = ''["Panel"]'';
        # Center: our clock applet (date menu with notifications, calendar, weather).
        "com.system76.CosmicPanel.Panel/v1/plugins_center" = ''
          Some([
              "dev.troy.Cabin.Clock",
          ])
        '';
        # Left: the workspace dots (click for the overview, scroll to switch).
        # Right: tray, the System button (live CPU and memory bars), then the
        # status icons, whose panels hold network, Bluetooth, power, graphics,
        # brightness, sound with per-app mixer, media. Cabin's daemon draws
        # every panel; these are its buttons.
        "com.system76.CosmicPanel.Panel/v1/plugins_wings" = ''
          Some((["dev.troy.Cabin.Workspaces"], [
              "com.system76.CosmicAppletStatusArea",
              "dev.troy.Cabin.System",
              "dev.troy.Cabin",
          ]))
        '';
        # Transparent panel background; the wallpaper shows through.
        "com.system76.CosmicPanel.Panel/v1/opacity" = "0.0";
        # Otherwise the panel turns solid whenever a window is maximized.
        "com.system76.CosmicPanel.Panel/v1/keep_style_on_maximize" = "true";
        # Bare Super opens Cabin's workspace overview (WorkspaceOverview in
        # system_actions below): typing there searches. Super+/ (COSMIC's
        # Launcher action) opens the same search on its own, over the windows,
        # listing them first. Either takes pop-launcher's prefixes ("= 2*3",
        # "~/", "find", "recent", "run"/"t:"/":", "?") and the web shortcuts
        # in cabin/data/web-shortcuts.ron ("g rust"). Super+A (AppLibrary)
        # opens it as the app library: every app's icon in a grid, no windows.
        # COSMIC's launcher, app library and overview aren't installed
        # (system/modules/cosmic.nix).
        # Super+S keeps COSMIC's ToggleStacking; Super+Shift+S opens
        # Cabin's capture overlay (snip or record a region, window or screen), and
        # pressed again stops a recording.
        # Super+L locks (replaces Focus(Right); arrows cover focus).
        # Super+Arrow moves the window and Super+Shift+Arrow moves focus (defaults swapped).
        # Super+Tab is Cabin's switcher with a picture of each window; Alt+Tab keeps
        # the icons (WindowSwitcher in system_actions below).
        "com.system76.CosmicSettings.Shortcuts/v1/custom" = ''
          {
              (modifiers: [Super]): System(WorkspaceOverview),

              (modifiers: [Super], key: "Tab"): Spawn("${config.modules.cabin.package}/bin/cabinctl switch next --previews"),
              (modifiers: [Super, Shift], key: "Tab"): Spawn("${config.modules.cabin.package}/bin/cabinctl switch prev --previews"),

              (modifiers: [Super, Shift], key: "s"): Spawn("${config.modules.cabin.package}/bin/cabin-capture"),
              (modifiers: [Super], key: "l"): System(LockScreen),

              (modifiers: [Super], key: "Left"): Move(Left),
              (modifiers: [Super], key: "Right"): Move(Right),
              (modifiers: [Super], key: "Up"): Move(Up),
              (modifiers: [Super], key: "Down"): Move(Down),
              (modifiers: [Super, Shift], key: "Left"): Focus(Left),
              (modifiers: [Super, Shift], key: "Right"): Focus(Right),
              (modifiers: [Super, Shift], key: "Up"): Focus(Up),
              (modifiers: [Super, Shift], key: "Down"): Focus(Down),
          }
        '';
        # Theme: blue accent (#5a8ab9) and a 1px active-window outline. These builder
        # keys keep Settings in sync; the built theme the desktop reads is generated
        # from them with `cosmic-ext-ctl build-theme` and stored in ./cosmic-theme/dark-v2
        # (only keys that differ from the shipped default).
        "com.system76.CosmicTheme.Dark.Builder/v2/accent" = ''Some("#5A8AB9FF")'';
        "com.system76.CosmicTheme.Dark.Builder/v2/active_hint" = "1";
        # Pinned to the dock in Cabin's overview, in this order.
        "com.system76.CosmicAppList/v1/favorites" = ''
          [
              "chromium-browser",
              "code",
              "vesktop",
              "com.mitchellh.ghostty",
          ]
        '';
        "com.system76.CosmicComp/v1/workspaces" = ''
          (
              workspace_mode: OutputBound,
              workspace_layout: Vertical,
          )
        '';
        # Super+T opens a ghostty window in ~ instead of cosmic-term, Alt/Super+Tab
        # bring up Cabin's window switcher, and the overview is Cabin's too
        # (cosmic-workspaces was slow to show and crashed when toggled fast).
        # The file replaces the shipped map wholesale, so every other action is
        # copied from it.
        "com.system76.CosmicSettings.Shortcuts/v1/system_actions" = ''
          {
              /// Opens the application library: Cabin's launcher, every app listed
              AppLibrary: "${config.modules.cabin.package}/bin/cabinctl launcher --apps",
              /// Decreases screen brightness
              BrightnessDown: "busctl --user call com.system76.CosmicSettingsDaemon /com/system76/CosmicSettingsDaemon com.system76.CosmicSettingsDaemon DecreaseDisplayBrightness",
              /// Increases screen brightness
              BrightnessUp: "busctl --user call com.system76.CosmicSettingsDaemon /com/system76/CosmicSettingsDaemon com.system76.CosmicSettingsDaemon IncreaseDisplayBrightness",
              /// Toggles display mode
              DisplayToggle: "cosmic-osd display",
              /// Switch between input sources
              InputSourceSwitch: "busctl --user call com.system76.CosmicSettingsDaemon /com/system76/CosmicSettingsDaemon com.system76.CosmicSettingsDaemon InputSourceSwitch",
              /// Opens the home folder in a system default file browser
              HomeFolder: "xdg-open ~",
              /// Logs out
              LogOut: "cosmic-osd log-out",
              /// Decreases keyboard brightness
              // KeyboardBrightnessDown,
              /// Increases keyboard brightness
              // KeyboardBrightnessUp,
              /// Opens the launcher: Cabin's search, over the windows
              Launcher: "${config.modules.cabin.package}/bin/cabinctl launcher",
              /// Locks the screen
              LockScreen: "loginctl lock-session",
              /// Mutes the active output device
              Mute: "wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle",
              /// Mutes the active microphone
              MuteMic: "wpctl set-mute @DEFAULT_AUDIO_SOURCE@ toggle",
              /// Plays and Pauses audio
              PlayPause: "playerctl play-pause",
              /// Goes to the next track
              PlayNext: "playerctl next",
              /// Goes to the previous track
              PlayPrev: "playerctl previous",
              /// Power off handler
              PowerOff: "cosmic-osd shutdown",
              /// Takes a screenshot
              Screenshot: "cosmic-screenshot",
              /// Suspend the system
              Suspend: "systemctl suspend",
              /// Toggles the screen reader
              ScreenReader: "busctl --user call com.system76.CosmicSettingsDaemon /com/system76/CosmicSettingsDaemon com.system76.CosmicSettingsDaemon ScreenReader",
              /// Opens the system default terminal
              Terminal: "ghostty +new-window --working-directory=home",
              /// Toggles touchpad on/off
              TouchpadToggle: "cosmic-osd touchpad",
              /// Lowers the volume of the active output device
              VolumeLower: "busctl --user call com.system76.CosmicSettingsDaemon /com/system76/CosmicSettingsDaemon com.system76.CosmicSettingsDaemon VolumeDown",
              /// Raises the volume of the active output device
              VolumeRaise: "busctl --user call com.system76.CosmicSettingsDaemon /com/system76/CosmicSettingsDaemon com.system76.CosmicSettingsDaemon VolumeUp",
              /// Opens the system default web browser
              WebBrowser: "xdg-open http://",
              /// Opens the (alt+tab) window switcher
              WindowSwitcher: "${config.modules.cabin.package}/bin/cabinctl switch next",
              /// Opens the (alt+shift+tab) window switcher
              WindowSwitcherPrevious: "${config.modules.cabin.package}/bin/cabinctl switch prev",
              /// Opens the workspace overview
              WorkspaceOverview: "${config.modules.cabin.package}/bin/cabinctl overview",
          }
        '';
        "com.system76.CosmicBackground/v1/same-on-all" = "true";
        "com.system76.CosmicBackground/v1/all" = ''
          (
              output: "all",
              source: Path("${variables.wallpaper}"),
              filter_by_theme: true,
              rotation_frequency: 3600,
              filter_method: Lanczos,
              scaling_mode: Zoom,
              sampling_method: Alphanumeric,
          )
        '';
      }
      // {
        "cosmic/com.system76.CosmicTheme.Dark/v2" = {
          source = ./cosmic-theme/dark-v2;
          recursive = true;
          force = true;
        };
      };

    # Shim icon theme referenced by CosmicTk icon_theme above.
    xdg.dataFile."icons/CosmicAdwaita/index.theme".text = ''
      [Icon Theme]
      Name=CosmicAdwaita
      Comment=COSMIC icons with Adwaita reachable as a direct parent
      Inherits=Cosmic,Pop,Adwaita,hicolor
      Directories=
    '';
  };
}
