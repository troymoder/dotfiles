{
  pkgs,
  variables,
  lib,
  ...
}: let
  fw16-kbd-uleds = pkgs.stdenv.mkDerivation {
    pname = "fw16-kbd-uleds";
    version = "master";

    src = pkgs.fetchFromGitHub {
      owner = "paco3346";
      repo = "fw16-kbd-uleds";
      rev = "master";
      hash = "sha256-1mPZl5q3/pOXEE/yO1EngrL+CYtmoZ1aTNOIpvRgmsU=";
    };

    nativeBuildInputs = [pkgs.pkg-config];
    buildInputs = [pkgs.systemd];

    installFlags = ["PREFIX=$(out)"];

    meta = with lib; {
      description = "FW16 keyboard backlight bridge exposing QMK modules as UPower kbd_backlight";
      homepage = "https://github.com/paco3346/fw16-kbd-uleds";
      license = licenses.mit;
      platforms = platforms.linux;
    };
  };
in {
  imports = [
    ./presets/desktop.nix
  ];

  modules = {
    gnome = {
      autoLoginUser = variables.username;
    };
    nvidia.enable = true;
    raid = {
      enable = true;
      rootMdUuid = "cb37143d:d9cb8ef8:f0081246:a0716c23";
    };
    fw16-kbd-uleds.enable = true;
    home-manager.extraGroups = ["plugdev"];
  };

  services.colord.enable = true;
  environment.etc."color/icc/BOE_CQ_______NE160QDM_NZ6.icc".source = ../static/BOE_CQ_______NE160QDM_NZ6.icc;

  programs.steam.enable = true;

  boot.initrd.services.lvm.enable = true;
  boot.initrd.luks.devices.cryptroot = {
    device = "/dev/md0";
    allowDiscards = true;
  };

  fileSystems."/" = {
    device = "/dev/disk/by-label/nixos";
    fsType = "xfs";
  };

  services.automatic-timezoned.enable = true;
  services.geoclue2.enable = true;

  boot.loader.systemd-boot.enable = lib.mkForce false;
  boot.loader.efi.canTouchEfiVariables = true;
  boot.loader.efi.efiSysMountPoint = "/boot/efi1";
  boot.initrd.systemd.enable = true;
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
