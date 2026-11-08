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

    rootMdUuid = lib.mkOption {
      type = lib.types.str;
      description = "/ MD UUID";
    };
  };

  config = lib.mkIf cfg.enable {
    boot.kernelModules = ["md_mod"];
    boot.supportedFilesystems = ["xfs" "vfat"];
    boot.initrd = {
      availableKernelModules = ["md_mod" "raid1" "raid10"];
      kernelModules = ["dm-snapshot" "md_mod"];
    };

    boot.swraid = {
      enable = true;
      mdadmConf = ''
        PROGRAM ${pkgs.coreutils}/bin/true
        ARRAY /dev/md0 metadata=1.2 UUID=${cfg.rootMdUuid}
      '';
    };

    # Warn in interactive shells when /proc/mdstat shows a degraded array.
    environment.etc."profile.d/raid-health-warning.sh".text = ''
      if [ -t 1 ] && [ -r /proc/mdstat ] && grep -Eq '\[[U_]*_[U_]*\]' /proc/mdstat; then
        printf '\nWARNING: RAID array appears degraded (check /proc/mdstat and mdadm --detail).\n\n' >&2
      fi
    '';
  };
}
