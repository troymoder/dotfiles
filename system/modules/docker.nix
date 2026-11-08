{
  config,
  lib,
  ...
}: let
  cfg = config.modules.docker;
in {
  options.modules.docker = {
    enable = lib.mkEnableOption "rootless Docker";
  };

  config = lib.mkIf cfg.enable {
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

    hardware.nvidia-container-toolkit.enable = config.modules.nvidia.enable;

    environment.sessionVariables = {
      CONTAINERD_ADDRESS = "$XDG_RUNTIME_DIR/docker/containerd/containerd.sock";
    };
  };
}
