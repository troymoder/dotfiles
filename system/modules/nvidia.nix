{
  config,
  lib,
  ...
}: let
  cfg = config.modules.nvidia;
in {
  options.modules.nvidia = {
    enable = lib.mkEnableOption "nvidia GPU support";

    open = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Use open source nvidia kernel modules";
    };

    datacenter = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Use datacenter mode";
    };

    package = lib.mkOption {
      type = lib.types.package;
      default = config.boot.kernelPackages.nvidiaPackages.stable;
      description = "Which nvidia driver package to use";
    };
  };

  config = lib.mkIf cfg.enable {
    hardware.nvidia = {
      open = cfg.open;
      package = cfg.package;
      datacenter.enable = cfg.datacenter;
      nvidiaPersistenced = true;
      nvidiaSettings = true;
      modesetting.enable = true;
    };

    services.xserver.videoDrivers = ["nvidia"];
  };
}
