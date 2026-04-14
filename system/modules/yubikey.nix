{
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.modules.yubikey;
in {
  options.modules.yubikey = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Enable yubikey";
    };
  };

  config = lib.mkIf cfg.enable {
    services.fwupd.enable = true;
    services.pcscd.enable = true;
    services.udev.packages = [pkgs.yubikey-personalization];
    services.dbus.packages = [pkgs.gcr];
  };
}
