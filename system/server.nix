{
  config,
  lib,
  pkgs,
  ...
}: {
  modules = {
    dns.enable = true;
    docker.enable = true;
    home-manager.enable = true;
    networking.enable = true;
    tailscale.enable = true;
    nvidia.enable = true;
    vscode-server.enable = true;
  };

  # Hardware detection
  boot = {
    initrd = {
      availableKernelModules = ["xhci_pci" "nvme" "ahci" "usbhid" "usb_storage" "sr_mod" "md_mod" "raid1" "raid10"];
      kernelModules = ["dm-snapshot" "md_mod"];
    };
    kernelModules = ["kvm-amd" "md_mod"];
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

  # RAID Configuration
  boot.swraid = {
    enable = true;
    # Manual assembly - see https://std.rocks/gnulinux_mdadm_uefi.html
    mdadmConf = ''
      ARRAY /dev/md0 metadata=1.2 UUID=2a4d8a6f:3c6b02d7:3f22d8e0:3b2b18bd
      ARRAY <ignore> metadata=1.0 UUID=157660cb:85532742:c1a1af01:b6df6973
    '';
  };

  # Bootloader (manually installed on each drive)
  boot.loader = {
    efi = {
      canTouchEfiVariables = false;
      efiSysMountPoint = "/boot/efi";
    };
    grub = {
      efiSupport = true;
      device = "nodev";
    };
  };

  # Auto-mount /boot/efi after mdadm resync
  systemd.services.boot_efi_mount = {
    after = ["local-fs.target"];
    wantedBy = ["sysinit.target"];
    path = [pkgs.mdadm pkgs.mount];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = "yes";
    };
    script = ''
      echo "Resyncing /boot/efi"
      if ! grep -q md100 /proc/mdstat; then
        mdadm -A /dev/md100 --uuid=157660cb:85532742:c1a1af01:b6df6973 --update=resync
      else
        echo "md100 already assembled, requesting resync"
        mdadm --action=repair /dev/md100 || true
      fi
      if ! mountpoint -q /boot/efi; then
        mount /dev/md100 /boot/efi
      else
        echo "/boot/efi already mounted"
      fi
    '';
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
