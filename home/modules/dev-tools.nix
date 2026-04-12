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
      [neovim gcc15]
      ++ lib.optionals cfg.rust [rustup]
      ++ lib.optionals cfg.python [uv]
      ++ lib.optionals cfg.go [go]
      ++ lib.optionals cfg.bazel [bazelisk starpls buildifier]
      ++ lib.optionals cfg.protobuf [protobuf buf]
      ++ cfg.extraPackages;
  };
}
