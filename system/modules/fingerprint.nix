{
  config,
  lib,
  ...
}: let
  cfg = config.modules.fingerprint;
in {
  options.modules.fingerprint = {
    enable = lib.mkEnableOption "fingerprint reader support";
  };

  config = lib.mkIf cfg.enable {
    services.fprintd.enable = true;
    security.pam.services.sudo.fprintAuth = true;
  };
}
