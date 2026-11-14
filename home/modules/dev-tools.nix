{
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.modules.dev-tools;
in {
  options.modules.dev-tools = {
    enable = lib.mkEnableOption "development tools";

    rust = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Install Rust toolchain";
    };

    python = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Install Python (uv)";
    };

    node = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Install Node.js (pnpm)";
    };

    go = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Install Go";
    };

    bazel = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Install Bazel toolchain";
    };

    protobuf = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Install protobuf tools";
    };

    extraPackages = lib.mkOption {
      type = lib.types.listOf lib.types.package;
      default = [];
      description = "Additional dev packages";
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = with pkgs;
    # clang's wrapper also ships cc/c++/cpp. Both wrappers are already lowPrio (10) in
    # nixpkgs, so hiPrio gcc to keep it as `cc`.
      [(lib.hiPrio gcc16) llvmPackages_23.clang ffmpeg-full]
      ++ lib.optionals cfg.rust [rustup]
      ++ lib.optionals cfg.python [uv]
      ++ lib.optionals cfg.node [nodejs pnpm]
      ++ lib.optionals cfg.go [go]
      ++ lib.optionals cfg.bazel [bazelisk starpls buildifier]
      ++ lib.optionals cfg.protobuf [protobuf buf]
      ++ cfg.extraPackages;

    # The REPLs in the launcher: each opens in a terminal.
    xdg.desktopEntries = let
      papirus = "${pkgs.papirus-icon-theme}/share/icons/Papirus/64x64";
    in
      lib.optionalAttrs cfg.python {
        python3 = {
          name = "Python";
          comment = "Python shell in a terminal";
          exec = "python3";
          terminal = true;
          icon = "${papirus}/apps/python.svg";
          categories = ["Development"];
        };
      }
      // lib.optionalAttrs cfg.node {
        node = {
          name = "Node.js";
          comment = "JavaScript shell in a terminal";
          exec = "node";
          terminal = true;
          icon = "${papirus}/mimetypes/application-javascript.svg";
          categories = ["Development"];
        };
      };

    home.sessionPath =
      # `cargo install` binaries.
      lib.optionals cfg.rust ["$HOME/.cargo/bin"]
      # uv installs managed Pythons (`uv python install --default`) and `uv tool` binaries here.
      ++ lib.optionals cfg.python ["$HOME/.local/bin"]
      # `pnpm add -g` binaries.
      ++ lib.optionals cfg.node ["$HOME/.local/share/pnpm"];

    home.sessionVariables = lib.mkIf cfg.node {PNPM_HOME = "$HOME/.local/share/pnpm";};
  };
}
