{
  pkgs,
  lib,
  ...
}: {
  imports = [
    ./presets/desktop.nix
  ];

  # Enable the settings for this laptop
  hardware.lenovo-thinkpad-t14s.enable = true;

  modules = {
    grub = {
      enable = true;
      efiDirectories = ["/boot/efi"];
    };
  };

  services.fwupd.enable = true;

  # Special service for disabling audio compressors
  systemd.user.services.disable-audio-compressors = {
    description = "Disable audio compressors";
    wantedBy = ["graphical-session.target"];
    partOf = ["graphical-session.target"];
    after = ["wireplumber.service"];
    wants = ["wireplumber.service"];
    script = ''
      sleep 5
      ${pkgs.alsa-utils}/bin/amixer -c X1E80100LENOVOT sset 'RX_COMP1' off
      ${pkgs.alsa-utils}/bin/amixer -c X1E80100LENOVOT sset 'RX_COMP2' off
      ${pkgs.alsa-utils}/bin/amixer -c X1E80100LENOVOT sset 'WSA WSA_COMP1' off
      ${pkgs.alsa-utils}/bin/amixer -c X1E80100LENOVOT sset 'WSA WSA_COMP2' off
      ${pkgs.alsa-utils}/bin/amixer -c X1E80100LENOVOT sset 'SpkrLeft COMP' off
      ${pkgs.alsa-utils}/bin/amixer -c X1E80100LENOVOT sset 'SpkrRight COMP' off
    '';
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
  };

  boot = {
    # Enable some SysRq keys (80 = sync + process kill)
    # See: https://docs.kernel.org/admin-guide/sysrq.html
    kernel.sysctl."kernel.sysrq" = 80;
    # Improves system performance
    kernelParams = lib.mkAfter ["pcie_aspm=off"];
    # This is to get emulation for x86-64 on aarch64
    binfmt.emulatedSystems = ["x86_64-linux"];
  };

  # Correctly thermal throttle on this machine
  systemd.services.thermal-throttle = {
    description = "Userspace CPU thermal throttle for Snapdragon X Elite";
    wantedBy = ["multi-user.target"];

    script = ''
      ${pkgs.python3}/bin/python3 ${./thinkpad/thermal-throttle.py}
    '';

    serviceConfig = {
      Type = "simple";
      Restart = "always";
      RestartSec = 5;
    };
  };

  fileSystems."/" = {
    device = "/dev/disk/by-label/nixos";
    fsType = "xfs";
  };
  fileSystems."/boot/efi" = {
    device = "/dev/disk/by-label/efi";
    fsType = "vfat";
  };

  system.stateVersion = "25.11";
}
