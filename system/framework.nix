{
  pkgs,
  variables,
  lib,
  ...
}: {
  imports = [
    ./presets/desktop.nix
  ];

  modules = {
    desktop.sessions = ["cosmic"];
    nvidia = {
      enable = true;
      switchable = true;
    };
    raid = {
      enable = true;
      rootMdUuid = "cb37143d:d9cb8ef8:f0081246:a0716c23";
    };
    fw16-kbd-uleds.enable = true;
    home-manager.extraGroups = ["plugdev"];
    dns.enable = false;
  };

  # fwupd's synaptics_cape plugin probes the Audio Expansion Card (32ac:0010) over HID
  # to read its firmware version; the card's firmware does not answer and hangs until
  # power-cycled (kernel logs "cannot get freq ... err -110"). No LVFS firmware exists
  # for the card, so disabling the plugin loses nothing.
  services.fwupd.daemonSettings.DisabledPlugins = ["synaptics_cape"];

  services.colord.enable = true;
  environment.etc."color/icc/BOE_CQ_______NE160QDM_NZ6.icc".source = ../static/BOE_CQ_______NE160QDM_NZ6.icc;

  programs.steam = {
    enable = true;
    # Steam passes its environment to every game it launches, so re-expose the
    # NVIDIA Vulkan ICD (hidden session-wide by modules/nvidia.nix) and pin
    # Vulkan to the dGPU here. GL is left alone so the Steam client itself keeps
    # rendering on the iGPU; native OpenGL games can use `nvidia-offload %command%`.
    package = pkgs.steam.override {
      extraProfile = ''
        unset VK_LOADER_DRIVERS_DISABLE
        export __NV_PRIME_RENDER_OFFLOAD=1
        export __VK_LAYER_NV_optimus=NVIDIA_only
      '';
    };
  };

  boot.initrd.services.lvm.enable = true;
  boot.initrd.luks.devices.cryptroot = {
    device = "/dev/md0";
    allowDiscards = true;
    # Unlock order: TPM2 (pcrlock policy from lanzaboote measured boot, hands-free
    # on an untampered boot chain), then the YubiKey (FIDO2), then after 10s the
    # passphrase prompt. Enroll once, after a rebuild so the policy is current:
    #   systemd-cryptenroll --tpm2-device=auto --tpm2-pcrlock=/var/lib/systemd/pcrlock.json /dev/md0
    #   systemd-cryptenroll --fido2-device=auto /dev/md0
    crypttabExtraOpts = ["tpm2-device=auto" "fido2-device=auto" "token-timeout=10"];
  };

  fileSystems."/" = {
    device = "/dev/disk/by-label/nixos";
    fsType = "xfs";
  };

  # The EC charge limit as standard battery attributes, so UPower (and the cabin
  # Battery menu) can stop charging at 80 %. The driver stands aside on Framework
  # laptops unless asked, since the EC also has its own charge limit command.
  boot.extraModprobeConfig = ''
    options cros_charge_control probe_with_fwk_charge_control=1
  '';

  # Night light (the cabin Display menu switches it): no password for wheel.
  programs.cosmic-nightlight.enable = true;

  services.automatic-timezoned.enable = true;
  services.geoclue2 = {
    enable = true;
    # The cabin clock asks for the location for its weather. COSMIC runs no
    # GeoClue agent to ask the user, so the clock is allowed as a system app.
    appConfig."dev.troy.Cabin.Clock" = {
      isAllowed = true;
      isSystem = true;
    };
  };

  boot.loader.systemd-boot.enable = lib.mkForce false;
  boot.loader.efi.canTouchEfiVariables = true;
  boot.loader.efi.efiSysMountPoint = "/boot/efi1";
  boot.lanzaboote = {
    enable = true;
    pkiBundle = "/var/lib/sbctl";
    extraEfiSysMountPoints = [
      "/boot/efi2"
    ];
    configurationLimit = 4;
    measuredBoot = {
      enable = true;
      pcrs = [
        0
        4
        7
      ];
    };
  };

  environment.systemPackages = [
    pkgs.sbctl
  ];

  systemd.services.systemd-boot-random-seed.serviceConfig.ExecStart = [
    ""
    "${pkgs.systemd}/bin/bootctl --graceful --esp-path=/boot/efi1 random-seed"
    "${pkgs.systemd}/bin/bootctl --graceful --esp-path=/boot/efi2 random-seed"
  ];

  fileSystems."/boot/efi1" = {
    device = "/dev/disk/by-label/efi1";
    fsType = "vfat";
    options = ["fmask=0077" "dmask=0077" "uid=0" "gid=0"];
  };

  fileSystems."/boot/efi2" = {
    device = "/dev/disk/by-label/efi2";
    fsType = "vfat";
    options = ["fmask=0077" "dmask=0077" "uid=0" "gid=0"];
  };

  systemd.services.bind-boot-efi = {
    description = "Mount /boot/efi to the ESP that was actually booted";
    wantedBy = ["local-fs.target"];
    before = ["local-fs.target"];

    unitConfig = {
      DefaultDependencies = false;
    };

    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
      ExecStart = pkgs.writeShellScript "mount-boot-efi" ''
        set -eu

        LOADER_VAR=/sys/firmware/efi/efivars/LoaderDevicePartUUID-4a67b082-0a4c-41cf-b6c7-440b29bb8c4f

        if [ ! -r "$LOADER_VAR" ]; then
          echo "LoaderDevicePartUUID not available at $LOADER_VAR" >&2
          exit 1
        fi

        # Strip 4-byte EFI variable attribute header, decode UTF-16LE to ASCII, lowercase
        booted_partuuid=$(${pkgs.coreutils}/bin/tail -c +5 "$LOADER_VAR" \
          | ${pkgs.coreutils}/bin/tr -d '\0' \
          | ${pkgs.gawk}/bin/awk '{print tolower($0)}')

        if [ -z "$booted_partuuid" ]; then
          echo "Could not decode booted PARTUUID from $LOADER_VAR" >&2
          exit 1
        fi

        device=/dev/disk/by-partuuid/$booted_partuuid

        # Wait briefly for udev to populate the symlink
        for _ in 1 2 3 4 5; do
          [ -e "$device" ] && break
          sleep 1
        done

        if [ ! -e "$device" ]; then
          echo "Device $device not found for booted PARTUUID $booted_partuuid" >&2
          exit 1
        fi

        echo "Booted PARTUUID: $booted_partuuid"
        echo "Mounting $device at /boot/efi"

        ${pkgs.coreutils}/bin/mkdir -p /boot/efi
        if ${pkgs.util-linux}/bin/mountpoint -q /boot/efi; then
          ${pkgs.util-linux}/bin/umount /boot/efi
        fi
        ${pkgs.util-linux}/bin/mount -t vfat \
          -o fmask=0077,dmask=0077,uid=0,gid=0 \
          "$device" /boot/efi
      '';
      ExecStop = pkgs.writeShellScript "unmount-boot-efi" ''
        ${pkgs.util-linux}/bin/umount /boot/efi || true
      '';
    };
  };

  system.stateVersion = "25.11";
}
