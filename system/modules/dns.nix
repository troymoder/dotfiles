{
  config,
  lib,
  ...
}: let
  cfg = config.modules.dns;
in {
  options.modules.dns = {
    enable = lib.mkEnableOption "DNS with resolved";

    nameservers = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = ["1.1.1.1" "8.8.8.8"];
      description = "Primary DNS nameservers";
    };

    dnssec = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Enable DNSSEC validation";
    };

    dnsovertls = lib.mkOption {
      type = lib.types.enum ["yes" "no" "opportunistic"];
      default = "opportunistic";
      description = "DNS-over-TLS mode";
    };
  };

  config = lib.mkIf cfg.enable {
    networking.nameservers = cfg.nameservers;
    services.resolved = {
      enable = true;
      dnssec =
        if cfg.dnssec
        then "true"
        else "false";
      domains = ["~."];
      fallbackDns = cfg.nameservers;
      dnsovertls = cfg.dnsovertls;
    };
  };
}
