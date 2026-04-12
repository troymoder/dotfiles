{pkgs, ...}: let
  gdmPlymouthTheme = pkgs.stdenv.mkDerivation {
    pname = "gdm-plymouth-theme";
    version = "1.0";
    dontUnpack = true;

    installPhase = ''
      mkdir -p $out/share/plymouth/themes/gdm
      cp -r ${pkgs.plymouth}/share/plymouth/themes/spinner/* \
            $out/share/plymouth/themes/gdm/

      # Replace background with GNOME default wallpaper
      cp ${pkgs.gnome-backgrounds}/share/backgrounds/gnome/adwaita-d.jxl \
         $out/share/plymouth/themes/gdm/background.png || \
      cp ${pkgs.gnome-backgrounds}/share/backgrounds/gnome/adwaita-d.png \
         $out/share/plymouth/themes/gdm/background.png

      # Rename descriptor
      mv $out/share/plymouth/themes/gdm/spinner.plymouth \
         $out/share/plymouth/themes/gdm/gdm.plymouth

      find $out/share/plymouth/themes/ -name \*.plymouth \
        -exec sed -i "s@/usr/@$out/@" {} \;
      find $out/share/plymouth/themes/ -name \*.plymouth \
        -exec sed -i "s@Name=Spinner@Name=GDM@" {} \;
    '';
  };
in {
  modules = {
    audio.enable = true;
    bluetooth.enable = true;
    dns.enable = true;
    docker.enable = true;
    fingerprint.enable = true;
    gnome.enable = true;
    home-manager.enable = true;
    networking.enable = true;
    printing.enable = true;
    swap.enable = true;
    tailscale.enable = true;
    nvidia.enable = true;
    raid = {
      enable = true;
      rootMdUuid = "cb37143d:d9cb8ef8:f0081246:a0716c23";
      efiDirectories = [ "/boot/efi1" ];
    };
  };

  services.colord.enable = true;
  environment.etc."color/icc/BOE_CQ_______NE160QDM_NZ6.icc".source = ../static/BOE_CQ_______NE160QDM_NZ6.icc;

  services.fwupd.enable = true;

  boot.initrd.services.lvm.enable = true;
  boot.initrd.luks.devices.cryptroot = {
    device = "/dev/md0";
    allowDiscards = true;
  };

  fileSystems."/" = {
    device = "/dev/disk/by-label/nixos";
    fsType = "xfs";
  };

  fileSystems."/boot/efi1" = {
    device = "/dev/disk/by-label/efi1";
    fsType = "vfat";
  };

  boot.plymouth = {
    enable = true;
    theme = "gdm";
    themePackages = [ gdmPlymouthTheme ]
  };

  boot.kernelParams = [ "quiet" "splash" "loglevel=3" "rd.udev.log_level=3" ];
  boot.consoleLogLevel = 0;
  boot.initrd.verbose = false;
  boot.initrd.systemd.enable = true;

  system.stateVersion = "25.11";
}
