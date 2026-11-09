{
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.modules.cosmic;
in {
  options.modules.cosmic = {
    enable = lib.mkEnableOption "COSMIC desktop environment";
  };

  config = lib.mkIf cfg.enable {
    services.desktopManager.cosmic.enable = true;

    services.displayManager.cosmic-greeter.enable = true;

    # Greeter login: fingerprint first, password as fallback. The login keyring
    # password is blank, so neither needs to unlock it.
    security.pam.services.cosmic-greeter.fprintAuth = config.services.fprintd.enable;

    # The COSMIC module would otherwise enable system76-power, which also wants
    # to manage hybrid graphics. power-profiles-daemon is enough here.
    services.power-profiles-daemon.enable = true;

    # Flatpak-only store; ghostty/neovim replace the terminal and editor;
    # COSMIC Settings covers networking, so no nm-connection-editor.
    environment.cosmic.excludePackages = with pkgs; [cosmic-store cosmic-term cosmic-edit networkmanagerapplet];
  };
}
