{...}: {
  modules = {
    audio.enable = true;
    bluetooth.enable = true;
    dns.enable = true;
    docker.enable = true;
    fingerprint.enable = true;
    gnome.enable = true;
    home-manager.enable = true;
    networking.enable = true;
    printing.enable = true;
    swap.enable = true;
    systemd-boot.enable = true;
    tailscale.enable = true;
    nvidia.enable = true;
  };

  services.colord.enable = true;
  environment.etc."color/icc/BOE_CQ_______NE160QDM_NZ6.icc".source = ../static/BOE_CQ_______NE160QDM_NZ6.icc;

  fileSystems."/" = {
    device = "/dev/disk/by-label/nixos";
    fsType = "xfs";
  };
  fileSystems."/boot" = {
    device = "/dev/disk/by-label/boot";
    fsType = "vfat";
    options = ["umask=0077"];
  };

  system.stateVersion = "25.11";
}
