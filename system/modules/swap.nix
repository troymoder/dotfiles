{
  config,
  lib,
  ...
}: let
  cfg = config.modules.swap;
in {
  options.modules.swap = {
    enable = lib.mkEnableOption "zram swap";

    algorithm = lib.mkOption {
      type = lib.types.enum ["zstd" "lz4" "lzo" "lzo-rle"];
      default = "zstd";
      description = "Compression algorithm for zram";
    };

    memoryPercent = lib.mkOption {
      type = lib.types.int;
      default = 50;
      description = "Percentage of RAM to use for zram swap";
    };
  };

  config = lib.mkIf cfg.enable {
    zramSwap = {
      enable = true;
      algorithm = cfg.algorithm;
      memoryPercent = cfg.memoryPercent;
    };
  };
}
