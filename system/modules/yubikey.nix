{
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.modules.yubikey;
in {
  options.modules.yubikey = {
    enable = lib.mkEnableOption "Enable yubikey";
  };

  config = lib.mkIf cfg.enable {
    services.pcscd.enable = true;
    services.udev.packages = [pkgs.yubikey-personalization];
    services.dbus.packages = [pkgs.gcr];
  };
}
