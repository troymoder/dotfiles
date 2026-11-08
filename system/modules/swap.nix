{
  config,
  lib,
  ...
}: let
  cfg = config.modules.swap;
in {
  options.modules.swap = {
    enable = lib.mkEnableOption "zram swap";
  };

  config = lib.mkIf cfg.enable {
    zramSwap = {
      enable = true;
      algorithm = "zstd";
      memoryPercent = 50;
    };
  };
}
