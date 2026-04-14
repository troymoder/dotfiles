{lib, ...}: {
  modules = {
    dns.enable = lib.mkDefault true;
    home-manager.enable = lib.mkDefault true;
    tailscale.enable = lib.mkDefault true;
    ssh.enable = lib.mkDefault true;
  };
}
