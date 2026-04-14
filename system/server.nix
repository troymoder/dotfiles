{
  config,
  lib,
  pkgs,
  ...
}: {
  imports = [
    ./presets/server.nix
  ];

  modules = {
    docker.enable = true;
    nvidia.enable = true;
    vscode-server.enable = true;
    grub = {
      enable = true;
      efiDirectories = ["/boot/efi"];
    };
    raid = {
      enable = true;
      rootMdUuid = "2a4d8a6f:3c6b02d7:3f22d8e0:3b2b18bd";
    };
  };

  # Hardware detection
  boot = {
    initrd = {
      availableKernelModules = ["xhci_pci" "nvme" "ahci" "usbhid" "usb_storage" "sr_mod"];
      kernelModules = ["dm-snapshot"];
    };
    kernelModules = ["kvm-amd"];
    kernelParams = ["transparent_hugepage=madvise"];
    supportedFilesystems = ["xfs" "fat32"];
  };

  # CPU
  hardware.cpu.amd.updateMicrocode = true;

  # This system has a raid on the root and /boot/efi partitions.
  # Now the root is fine but raid on EFI can be tricky.
  # Since the /boot/efi partition can be changed outside of the linux os
  # meaning outside of our software raid. So we need to manually construct the
  # array after boot to ensure its correctly synced.

  fileSystems."/" = {
    device = "/dev/disk/by-label/NIXROOT";
    fsType = "xfs";
  };

  # This server has a BOND LCAP connection
  networking.networkmanager = {
    ensureProfiles.profiles = {
      "Bond connection 1" = {
        bond = {
          miimon = "100";
          mode = "802.3ad";
          xmit_hash_policy = "layer3+4";
        };
        connection = {
          id = "Bond connection 1";
          interface-name = "bond0";
          type = "bond";
        };
        ipv4 = {
          method = "manual";
          addresses = "192.168.1.2/24";
          gateway = "192.168.1.10";
        };
      };
      "bond0 port 1" = {
        connection = {
          id = "bond0 port 1";
          type = "ethernet";
          interface-name = "enp133s0f0np0";
          controller = "bond0";
          port-type = "bond";
        };
      };
      "bond0 port 2" = {
        connection = {
          id = "bond0 port 2";
          type = "ethernet";
          interface-name = "enp133s0f1np1";
          controller = "bond0";
          port-type = "bond";
        };
      };
      "bond0 port 3" = {
        connection = {
          id = "bond0 port 3";
          type = "ethernet";
          interface-name = "enp133s0f2np2";
          controller = "bond0";
          port-type = "bond";
        };
      };
      "bond0 port 4" = {
        connection = {
          id = "bond0 port 4";
          type = "ethernet";
          interface-name = "enp133s0f3np3";
          controller = "bond0";
          port-type = "bond";
        };
      };
    };
  };

  system.stateVersion = "25.05";
}
