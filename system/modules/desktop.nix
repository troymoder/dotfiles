{
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.modules.desktop;
  has = session: lib.elem session cfg.sessions;
in {
  options.modules.desktop = {
    sessions = lib.mkOption {
      type = lib.types.listOf (lib.types.enum ["gnome"]);
      default = [];
      description = ''
        Desktop sessions to install. Every listed session is offered at the
        login screen, so several can be enabled side by side to trial one.
      '';
    };

    autoLoginUser = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "User to log in automatically at the display manager";
    };
  };

  config = lib.mkIf (cfg.sessions != []) {
    modules.gnome.enable = lib.mkIf (has "gnome") true;

    services.displayManager.autoLogin.user = lib.mkIf (cfg.autoLoginUser != null) cfg.autoLoginUser;

    services.gnome.gnome-keyring.enable = true;

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
      sessionVariables = {
        XDG_RUNTIME_DIR = "/run/user/$UID";
        GTK_USE_PORTAL = "1";
        NIXOS_OZONE_WL = "1";
      };

      systemPackages = with pkgs; [
        alsa-utils
        alsa-ucm-conf
        ghostty
        xdg-terminal-exec
        libinput
      ];

      etc."xdg/xdg-terminals.list".text = ''
        ghostty
      '';
    };
  };
}
