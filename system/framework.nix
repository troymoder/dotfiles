{pkgs, ...}: {
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
    raid = {
      enable = true;
      efiMdUuid = "1be8102d:cb57e0cc:5e8def92:a98a58e5";
      rootMdUuid = "cb37143d:d9cb8ef8:f0081246:a0716c23";
    };
  };

  services.colord.enable = true;
  environment.etc."color/icc/BOE_CQ_______NE160QDM_NZ6.icc".source = ../static/BOE_CQ_______NE160QDM_NZ6.icc;

  services.fwupd.enable = true;

  fileSystems."/" = {
    device = "/dev/disk/by-label/nixos";
    fsType = "xfs";
  };

  boot.initrd.luks.devices.cryptroot = {
    device = "/dev/md0";
    allowDiscards = true;
  };

  system.stateVersion = "25.11";
}
