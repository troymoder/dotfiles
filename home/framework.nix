{
  pkgs,
  technorino,
  ...
}: let
  bunOptions = "--preload=${./no-agent-summary.js}";
in {
  imports = [
    ./profiles/base.nix
  ];

  modules = {
    dev-tools.enable = true;
    yubikey.enable = true;

    cosmic-desktop.enable = true;
    cabin.enable = true;

    desktop-apps.extraPackages = with pkgs; [
      technorino.packages.${pkgs.stdenv.hostPlatform.system}.package
      spotifast
      vlc
      slack
      zoom-us
      claude-code
    ];
  };

  # Bun preload that disables Claude Code's agent_summary side-queries (~15-20% of token spend).
  # https://gist.github.com/flamerix-ai/c3905705cc4205e5de3011fa2e74efce — re-verify after claude-code bumps.
  # Set in both so fish terminals and GUI-launched apps (VS Code) get it.
  home.sessionVariables.BUN_OPTIONS = bunOptions;
  systemd.user.sessionVariables.BUN_OPTIONS = bunOptions;

  programs.home-manager.enable = true;
  home.stateVersion = "25.11";
}
