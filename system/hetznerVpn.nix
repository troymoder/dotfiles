{
  pkgs,
  modulesPath,
  lib,
  ...
}: {
  imports = [
    "${modulesPath}/profiles/qemu-guest.nix"
  ];

  boot.loader.systemd-boot.enable = true;
  boot.loader.efi.canTouchEfiVariables = true;

  networking.useDHCP = true;

  fileSystems."/" =
    { device = "/dev/disk/by-uuid/f755e68f-b67f-4f90-b54c-fee08b09348a";
      fsType = "ext4";
    };

  fileSystems."/boot" =
    { device = "/dev/disk/by-uuid/D60D-40CF";
      fsType = "vfat";
      options = [ "fmask=0022" "dmask=0022" ];
    };

  boot.initrd.availableKernelModules = ["ahci" "xhci_pci" "virtio_pci" "virtio_scsi" "sd_mod" "sr_mod"];
  system.stateVersion = "25.11";

  nixpkgs.hostPlatform = lib.mkDefault "x86_64-linux";
}
