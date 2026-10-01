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

        # Shell helpers
        direnv
        nix-your-shell

        # System info
        fastfetch
        hyfetch
        evil-helix
      ]
      ++ cfg.extraPackages;

    home.sessionVariables = {
      EDITOR = "hx";
      VISUAL = "hx";
    };

    programs.nix-index.enable = true;
    programs.command-not-found.enable = false;
  };
}
