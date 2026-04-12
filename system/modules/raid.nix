{
    pkgs,
  config,
  lib,
  ...
}: let
  cfg = config.modules.raid;
in {
  options.modules.raid = {
    enable = lib.mkEnableOption "Enable raid";

    rootMdName = lib.mkOption {
      type = lib.types.str;
      description = "Md name for root";
      default = "/dev/md0"
    };

    efiMdName = lib.mkOption {
      type = lib.types.str;
      description = "Md name for root";
      default = "/dev/md100"
    };

    efiMdUuid = lib.mkOption {
      type = lib.types.str;
      description = "/boot/efi MD UUID";
    };

    rootMdUuid = lib.mkOption {
      type = lib.types.str;
      description = "/ MD UUID";
    };
  };

  config = lib.mkIf cfg.enable {
    boot.kernelModules = ["md_mod"];
    boot.supportedFilesystems = ["xfs" "fat32"];
    boot.initrd = {
      availableKernelModules = ["md_mod" "raid1" "raid10"];
      kernelModules = ["dm-snapshot" "md_mod"];
    };

    # RAID Configuration
    boot.swraid = {
      enable = true;
      # Manual assembly - see https://std.rocks/gnulinux_mdadm_uefi.html
      mdadmConf = ''
        ARRAY ${cfg.rootMdName} metadata=1.2 UUID=${cfg.rootMdUuid}
        ARRAY <ignore> metadata=1.0 UUID=${cfg.efiMdUuid}
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
        efiInstallAsRemovable = true;
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
        if ! grep -q ${cfg.efiMdName} /proc/mdstat; then
            mdadm -A ${cfg.efiMdName} --uuid=${cfg.efiMdUuid} --update=resync
        else
            echo "${cfg.efiMdName} already assembled, requesting resync"
            mdadm --action=repair ${cfg.efiMdName} || true
        fi
        if ! mountpoint -q /boot/efi; then
            mount ${cfg.efiMdName} /boot/efi
        else
            echo "/boot/efi already mounted"
        fi
      '';
    };
  };
}
