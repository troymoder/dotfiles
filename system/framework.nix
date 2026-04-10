{
  pkgs,
  lib,
  ...
}: let
  variables = import ../variables.nix;
  timezone = "Africa/Johannesburg";
in {
  # Hardware
  hardware = {
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

  # For Printing
  services.avahi = {
    enable = true;
    nssmdns4 = true;
    openFirewall = true;
  };

  services.printing = {
    enable = true;
    drivers = with pkgs; [
      cups-filters
      cups-browsed
    ];
  };

  # Disable touchpad while typing
  services.libinput = {
    enable = true;
    touchpad = {
      tapping = false;
      disableWhileTyping = true;
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
  };

  # Filesystems
  fileSystems = {
    "/" = {
      device = "/dev/disk/by-label/nixos";
      fsType = "xfs";
    };
    "/boot" = {
      device = "/dev/disk/by-label/boot";
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
      XDG_DATA_DIRS = ["${pkgs.gdm}/share/gsettings-schemas/gdm-${pkgs.gdm.version}"];
      TZ = timezone;
    };

    # System packages (minimal - most should be in home-manager)
    systemPackages = with pkgs; [
      neovim
      alsa-utils
      alsa-ucm-conf
      ghostty
      xdg-terminal-exec
      libinput
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

  time.timeZone = timezone;

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
