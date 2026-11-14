{
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.modules.cli-tools;
in {
  options.modules.cli-tools = {
    enable = lib.mkEnableOption "common CLI tools";
  };

  config = lib.mkIf cfg.enable {
    home.packages = with pkgs; [
      # Editor
      (noDesktopEntry neovim)

      # System monitoring (no launcher entries for terminal tools, but btop
      # keeps its own: it opens in a terminal from the launcher)
      (noDesktopEntry htop)
      btop

      # Modern replacements
      eza
      bat
      ripgrep
      fd

      # CLI tools
      fzf
      jq
      tree
      wget
      curl
      file
      which
      gnupg

      # Archive tools
      unzip
      zip
      xz
      zstd
      gzip

      # System tools
      iperf3
      iftop
      iotop
      strace
      # Test suite fails under the nix sandbox (ptrace); binary builds fine.
      (ltrace.overrideAttrs (_: {doCheck = false;}))
      lsof
      ethtool
      pciutils
      usbutils
      dig

      # System info
      fastfetch
    ];

    home.sessionVariables = {
      EDITOR = "nvim";
      VISUAL = "nvim";
    };

    programs.nix-index.enable = true;
    programs.command-not-found.enable = false;
  };
}
