{
  pkgs,
  variables,
  ...
}: {
  imports = [
    ./presets/desktop.nix
  ];

  modules = {
    gnome = {
      autoLoginUser = variables.username;
    };
    nvidia.enable = true;
    raid = {
      enable = true;
      rootMdUuid = "cb37143d:d9cb8ef8:f0081246:a0716c23";
    };
    grub = {
      enable = true;
      efiDirectories = ["/boot/efi1"];
    };
  };

  services.colord.enable = true;
  environment.etc."color/icc/BOE_CQ_______NE160QDM_NZ6.icc".source = ../static/BOE_CQ_______NE160QDM_NZ6.icc;

  programs.steam.enable = true;

  services.ollama = {
    enable = true;
    package = pkgs.ollama-cuda;
  };

  boot.initrd.services.lvm.enable = true;
  boot.initrd.luks.devices.cryptroot = {
    device = "/dev/md0";
    allowDiscards = true;
  };

  fileSystems."/" = {
    device = "/dev/disk/by-label/nixos";
    fsType = "xfs";
  };

  fileSystems."/boot/efi1" = {
    device = "/dev/disk/by-label/efi1";
    fsType = "vfat";
  };

  system.stateVersion = "25.11";
}
