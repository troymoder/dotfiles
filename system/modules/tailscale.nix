{
  config,
  lib,
  ...
}: let
  cfg = config.modules.tailscale;
in {
  options.modules.tailscale = {
    enable = lib.mkEnableOption "Tailscale VPN";
  };

  config = lib.mkIf cfg.enable {
    services.tailscale.enable = true;

    networking.firewall = {
      trustedInterfaces = ["tailscale0"];
      allowedUDPPorts = [41641 1900 5351];
    };
  };
}
