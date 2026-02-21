{
  pkgs,
  lib,
  ...
}: let
  variables = import ../variables.nix;
in {
  # Hardware
  hardware = {
    lenovo-thinkpad-t14s.enable = true;
    enableRedistributableFirmware = true;
    bluetooth.enable = true;
  };

  # For Audio
  security.rtkit.enable = true;
  services.pipewire = {
    enable = true;
    alsa.enable = true;
    alsa.support32Bit = true;
    pulse.enable = true;
  };

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
    '';
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
  };

  # Boot
  boot = {
    loader.systemd-boot.enable = true;
    loader.systemd-boot.configurationLimit = 20;

    initrd.systemd = {
      enable = true;
      emergencyAccess = true; # Not secure, but easier for debugging
    };

    # Enable some SysRq keys (80 = sync + process kill)
    # See: https://docs.kernel.org/admin-guide/sysrq.html
    kernel.sysctl."kernel.sysrq" = 80;
    kernelParams = lib.mkAfter [ "pcie_aspm=off" ];
    binfmt.emulatedSystems = [ "x86_64-linux" ];
  };

  systemd.services.thermal-throttle = {
    description = "Userspace CPU thermal throttle for Snapdragon X Elite";
    wantedBy = [ "multi-user.target" ];
    
    script = ''
      ${pkgs.python3}/bin/python3 ${./thinkpad/thermal-throttle.py}
    '';
    
    serviceConfig = {
      Type = "simple";
      Restart = "always";
      RestartSec = 5;
    };
  };

  # Filesystems
  fileSystems = {
    "/" = {
      device = "/dev/disk/by-label/PRIMARY";
      fsType = "xfs";
    };
    "/boot" = {
      device = "/dev/disk/by-label/BOOT";
      fsType = "vfat";
      options = ["umask=0077"];
    };
  };

  # Networking
  networking.networkmanager = {
    enable = true;
    plugins = lib.mkForce [];
  };

  # Fingerprint reader
  services.fprintd.enable = true;

  # GNOME Desktop Environment
  services = {
    xserver.enable = true;
    xserver.excludePackages = [pkgs.xterm];

    displayManager.gdm = {
      enable = true;
      wayland = true;
    };

    desktopManager.gnome = {
      enable = true;

      # Enable fractional scaling
      extraGSettingsOverrides = ''
        [org.gnome.mutter]
        experimental-features=['scale-monitor-framebuffer', 'xwayland-native-scaling']
      '';
    };

    gnome.gnome-keyring.enable = true;
  };

  # Wayland environment
  environment = {
    sessionVariables = {
      NIXOS_OZONE_WL = "1"; # Enable Wayland for Electron apps
      XDG_RUNTIME_DIR = "/run/user/$UID";
      # Required for GNOME to detect fingerprint reader
      XDG_DATA_DIRS = ["${pkgs.gdm}/share/gsettings-schemas/gdm-${pkgs.gdm.version}"];
    };

    # System packages (minimal - most should be in home-manager)
    systemPackages = with pkgs; [
      neovim
      alsa-utils
      alsa-ucm-conf
      ghostty
      xdg-terminal-exec
    ];

    etc."xdg/xdg-terminals.list".text = ''
      ghostty
    '';

    # Remove GNOME bloatware
    gnome.excludePackages = with pkgs; [
      # Web & Communication
      epiphany
      geary

      # Media
      gnome-music
      totem
      cheese

      # Organization
      gnome-contacts
      gnome-maps
      gnome-calendar
      gnome-weather
      gnome-clocks

      # Utilities
      gnome-tour
      gnome-characters
      gnome-font-viewer
      gnome-connections
      gnome-terminal
      gnome-console
      snapshot

      # Games
      gnome-chess
      gnome-mahjongg
      gnome-mines
      gnome-sudoku
      gnome-tetravex
      iagno
      hitori
      atomix
      aisleriot
      tali
    ];
  };

  users.users.${variables.username}.extraGroups = lib.mkAfter ["docker" "video"];

  # PAM configuration for GNOME Keyring
  security.pam.services.gdm.enableGnomeKeyring = true;

  # Power management
  services = {
    power-profiles-daemon.enable = false;
    tlp.enable = true;
  };

  system.stateVersion = "25.11";
}
