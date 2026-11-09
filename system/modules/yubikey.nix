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
    services.dbus.packages = [pkgs.gcr_4];

    # sudo factors, in order: fingerprint, then a touch on the YubiKey (pam_u2f),
    # then the password. NixOS puts u2f before fprintd; move it after.
    # Register the key once with: pamu2fcfg > ~/.config/Yubico/u2f_keys
    security.pam.u2f.settings.cue = true;
    security.pam.services.sudo = {
      u2f.enable = true;
      rules.auth.u2f.order = config.security.pam.services.sudo.rules.auth.fprintd.order + 50;
    };
    environment.systemPackages = [pkgs.pam_u2f];
  };
}
