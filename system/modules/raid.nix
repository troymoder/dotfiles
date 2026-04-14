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
      default = "/dev/md0";
    };

    rootMdUuid = lib.mkOption {
      type = lib.types.str;
      description = "/ MD UUID";
    };

    mdadmProgram = lib.mkOption {
      type = lib.types.str;
      default = "${pkgs.coreutils}/bin/true";
      description = "Program called by mdadm monitor for RAID events";
    };

    shellWarning = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Show a warning in interactive shells when /proc/mdstat indicates a degraded RAID array";
    };
  };

  config = lib.mkIf cfg.enable {
    boot.kernelModules = ["md_mod"];
    boot.supportedFilesystems = ["xfs" "vfat"];
    boot.initrd = {
      availableKernelModules = ["md_mod" "raid1" "raid10"];
      kernelModules = ["dm-snapshot" "md_mod"];
    };

    # RAID Configuration
    boot.swraid = {
      enable = true;
      mdadmConf = ''
        PROGRAM ${cfg.mdadmProgram}
        ARRAY ${cfg.rootMdName} metadata=1.2 UUID=${cfg.rootMdUuid}
      '';
    };

    environment.etc = lib.mkIf cfg.shellWarning {
      "profile.d/raid-health-warning.sh".text = ''
        if [ -t 1 ] && [ -r /proc/mdstat ] && grep -Eq '\[[U_]*_[U_]*\]' /proc/mdstat; then
          printf '\nWARNING: RAID array appears degraded (check /proc/mdstat and mdadm --detail).\n\n' >&2
        fi
      '';
    };
  };
}
