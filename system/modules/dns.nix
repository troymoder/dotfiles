{
  config,
  lib,
  ...
}: let
  cfg = config.modules.dns;
  nameservers = ["1.1.1.1" "8.8.8.8"];
in {
  options.modules.dns = {
    enable = lib.mkEnableOption "DNS with resolved";
  };

  config = lib.mkIf cfg.enable {
    networking.nameservers = nameservers;
    services.resolved = {
      enable = true;
      settings.Resolve = {
        DNSOverTLS = "opportunistic";
        dnssec = "true";
        domains = ["~."];
        fallbackDns = nameservers;
      };
    };
  };
}
