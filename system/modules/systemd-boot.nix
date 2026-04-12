{
  config,
  lib,
  ...
}: let
  cfg = config.modules.systemd-boot;
in {
  options.modules.systemd-boot = {
    enable = lib.mkEnableOption "systemd-boot bootloader";

    configurationLimit = lib.mkOption {
      type = lib.types.int;
      default = 20;
      description = "Maximum number of boot configurations to keep";
    };
  };

  config = lib.mkIf cfg.enable {
    boot = {
      loader.systemd-boot = {
        enable = true;
        configurationLimit = cfg.configurationLimit;
      };
      loader.efi.canTouchEfiVariables = true;
      initrd.systemd.enable = true;
    };
  };
}
