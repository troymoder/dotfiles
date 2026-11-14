# Desktop-agnostic GUI apps and defaults, used by the cosmic-desktop module.
{
  config,
  lib,
  pkgs,
  osConfig,
  ...
}: let
  cfg = config.modules.desktop-apps;
  browser = "chromium";
  # Kept (they still open their files) but out of the app grid and the
  # launcher: Cabin covers screenshots and night light, and the rest are
  # config tools or viewers only opened through a file.
  hiddenApps = with pkgs; {
    "qt5ct" = libsForQt5.qt5ct;
    "qt6ct" = kdePackages.qt6ct;
    "cups" = cups;
    "io.github.cosmic_nightlight.settings" = osConfig.programs.cosmic-nightlight.package;
    "com.system76.CosmicScreenshot" = cosmic-screenshot;
    "com.system76.CosmicPlayer" = cosmic-player;
    "com.system76.CosmicViewer" = cosmic-viewer;
    "com.system76.CosmicReader" = cosmic-reader;
  };
in {
  options.modules.desktop-apps = {
    enable = lib.mkEnableOption "desktop applications";

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
        obs-studio
      ]
      ++ cfg.extraPackages;

    services.gnome-keyring = {
      enable = true;
      components = ["pkcs11" "secrets" "ssh"];
    };

    xdg.mimeApps = {
      enable = true;
      defaultApplications = {
        "text/html" = "${browser}.desktop";
        "x-scheme-handler/http" = "${browser}.desktop";
        "x-scheme-handler/https" = "${browser}.desktop";
        "x-scheme-handler/terminal" = "com.mitchellh.ghostty.desktop";
      };
    };
    xdg.configFile."mimeapps.list".force = true;

    # Ghostty's own icon is a terminal window; use Papirus's plain blue ghost instead,
    # installed under its own name so any icon theme falls through to it.
    # A copy of each hidden app's entry with NoDisplay=true over the
    # package's own: not Hidden=true, which would count it uninstalled and
    # leave its files with nothing to open them.
    xdg.dataFile =
      lib.mapAttrs' (id: pkg:
        lib.nameValuePair "applications/${id}.desktop" {
          source = pkgs.runCommand "${id}-hidden.desktop" {} ''
            sed -e '/^NoDisplay=/d' -e '/^\[Desktop Entry\]/a NoDisplay=true' \
              ${pkg}/share/applications/${id}.desktop > $out
          '';
          force = true;
        })
      hiddenApps
      // {
        "icons/hicolor/scalable/apps/ghostty-ghost.svg" = {
          source = "${pkgs.papirus-icon-theme}/share/icons/Papirus/64x64/apps/com.mitchellh.ghostty.svg";
          force = true;
        };
        "applications/com.mitchellh.ghostty.desktop" = {
          source = pkgs.runCommand "ghostty-ghost.desktop" {} ''
            # `+new-window`, not `--gtk-single-instance=true`: it asks the running
            # Ghostty for a window and sends the directory it was started in, so
            # COSMIC Files' "Open in terminal" (which runs this in the folder)
            # opens there; single-instance dropped the directory and opened in ~.
            sed -e "s|^Icon=.*|Icon=ghostty-ghost|" \
                -e "s|^Exec=ghostty .*|Exec=ghostty +new-window|" \
              ${pkgs.ghostty}/share/applications/com.mitchellh.ghostty.desktop > $out
          '';
          force = true;
        };
      };
  };
}
