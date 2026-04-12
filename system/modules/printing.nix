{
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.modules.printing;
in {
  options.modules.printing = {
    enable = lib.mkEnableOption "printing support";
  };

  config = lib.mkIf cfg.enable {
    services.avahi = {
      enable = true;
      nssmdns4 = true;
      openFirewall = true;
    };

    services.printing = {
      enable = true;
      drivers = with pkgs; [
        cups-filters
        cups-browsed
      ];
    };
  };
}
