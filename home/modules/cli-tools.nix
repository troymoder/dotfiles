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

    extraPackages = lib.mkOption {
      type = lib.types.listOf lib.types.package;
      default = [];
      description = "Additional CLI packages to install";
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = with pkgs;
      [
        # System monitoring
        htop
        btop

        # Modern replacements
        eza
        bat
        ripgrep
        fd
        ldns

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
        ltrace
        lsof
        ethtool
        pciutils
        usbutils

        # Shell helpers
        direnv
        nix-your-shell

        # System info
        fastfetch
        hyfetch
      ]
      ++ cfg.extraPackages;

    home.sessionVariables = {
      EDITOR = "nvim";
      VISUAL = "nvim";
    };

    programs.nix-index.enable = true;
    programs.command-not-found.enable = false;
  };
}
