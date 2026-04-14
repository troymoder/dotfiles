{
  config,
  lib,
  ...
}: let
  cfg = config.modules.docker;
in {
  options.modules.docker = {
    enable = lib.mkEnableOption "rootless Docker";

    nvidia = lib.mkOption {
      type = lib.types.bool;
      default =
        if config.modules ? nvidia
        then config.modules.nvidia.enable
        else false;
      description = "Enable nvidia container toolkit";
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = !cfg.nvidia || (config.modules ? nvidia && config.modules.nvidia.enable);
        message = "modules.docker.nvidia requires modules.nvidia.enable = true";
      }
    ];

    virtualisation.docker.rootless = {
      enable = true;
      setSocketVariable = true;
      daemon.settings = {
        registry-mirrors = ["https://mirror.gcr.io"];
        features = {
          buildkit = true;
          containerd-snapshotter = true;
          cdi = true;
        };
      };
    };

    hardware.nvidia-container-toolkit.enable = cfg.nvidia;

    environment.sessionVariables = {
      CONTAINERD_ADDRESS = "$XDG_RUNTIME_DIR/docker/containerd/containerd.sock";
    };
  };
}
