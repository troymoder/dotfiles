{lib, ...}: {
  modules = {
    audio.enable = lib.mkDefault true;
    bluetooth.enable = lib.mkDefault true;
    bootvine.enable = lib.mkDefault true;
    dns.enable = lib.mkDefault true;
    docker.enable = lib.mkDefault true;
    fingerprint.enable = lib.mkDefault true;
    desktop.sessions = lib.mkDefault ["cosmic"];
    home-manager.enable = lib.mkDefault true;
    printing.enable = lib.mkDefault true;
    scheduling.enable = lib.mkDefault true;
    swap.enable = lib.mkDefault true;
    tailscale.enable = lib.mkDefault true;
    yubikey.enable = lib.mkDefault true;
    ssh.enable = lib.mkDefault true;
  };

  # nm-online blocks network-online.target ~28s at boot, holding up tailscaled; laptops don't need it
  systemd.services.NetworkManager-wait-online.enable = lib.mkDefault false;
}
