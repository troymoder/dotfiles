{
  config,
  lib,
  ...
}: let
  cfg = config.modules.networking;
in {
  options.modules.networking = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Enable networking";
    };
  };

  config = lib.mkIf cfg.enable {
    networking.networkmanager.enable = true;
  };
}
