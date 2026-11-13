{
  config,
  lib,
  ...
}: let
  cfg = config.modules.bootvine;
in {
  options.modules.bootvine = {
    enable = lib.mkEnableOption "bootvine, a live tree of systemd jobs on the boot console";
  };

  config = lib.mkIf cfg.enable {
    # Both take the boot console: Plymouth's splash covers bootvine's tty12 and
    # its password agent takes the LUKS prompt first, leaving bootvine waiting.
    assertions = [
      {
        assertion = !config.modules.plymouth.enable;
        message = "modules.bootvine and modules.plymouth both take the boot console; enable only one.";
      }
    ];

    services.bootvine.enable = true;

    # bootvine silences the kernel console once it starts, but messages from
    # before that still reach the screen: keep those to errors.
    boot.consoleLogLevel = 3;
  };
}
