{lib, ...}: {
  modules = {
    audio.enable = lib.mkDefault true;
    bluetooth.enable = lib.mkDefault true;
    dns.enable = lib.mkDefault true;
    docker.enable = lib.mkDefault true;
    fingerprint.enable = lib.mkDefault true;
    gnome.enable = lib.mkDefault true;
    home-manager.enable = lib.mkDefault true;
    plymouth.enable = lib.mkDefault true;
    printing.enable = lib.mkDefault true;
    swap.enable = lib.mkDefault true;
    tailscale.enable = lib.mkDefault true;
    yubikey.enable = lib.mkDefault true;
    ssh.enable = lib.mkDefault true;
  };
}
