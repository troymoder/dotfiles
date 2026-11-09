# Desktop-agnostic GUI apps and defaults, used by the cosmic-desktop module.
{
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.modules.desktop-apps;
  browser = "chromium";
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
    xdg.dataFile = {
      "icons/hicolor/scalable/apps/ghostty-ghost.svg" = {
        source = "${pkgs.papirus-icon-theme}/share/icons/Papirus/64x64/apps/com.mitchellh.ghostty.svg";
        force = true;
      };
      "applications/com.mitchellh.ghostty.desktop" = {
        source = pkgs.runCommand "ghostty-ghost.desktop" {} ''
          sed "s|^Icon=.*|Icon=ghostty-ghost|" ${pkgs.ghostty}/share/applications/com.mitchellh.ghostty.desktop > $out
        '';
        force = true;
      };
    };
  };
}
