{
  config,
  lib,
  ...
}: let
  cfg = config.modules.touchpad;
in {
  options.modules.touchpad = {
    enable = lib.mkEnableOption "touchpad support";

    tapping = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Enable tap-to-click";
    };

    disableWhileTyping = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Disable touchpad while typing";
    };
  };

  config = lib.mkIf cfg.enable {
    services.libinput = {
      enable = true;
      touchpad = {
        tapping = cfg.tapping;
        disableWhileTyping = cfg.disableWhileTyping;
      };
    };
  };
}
