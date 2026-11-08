{
  config,
  lib,
  ...
}: let
  cfg = config.modules.plymouth;
in {
  options.modules.plymouth = {
    enable = lib.mkEnableOption "enable plymouth";
  };

  config = lib.mkIf cfg.enable {
    boot.plymouth = {
      enable = true;
      theme = "bgrt";
    };

    boot.kernelParams = ["quiet" "splash" "loglevel=3" "rd.udev.log_level=3"];
    boot.consoleLogLevel = 0;
  };
}
