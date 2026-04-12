{pkgs, ...}: let
  loginPlymouthTheme = pkgs.runCommand "plymouth-login-theme" {
    nativeBuildInputs = [ pkgs.imagemagick ];
  } ''
    themeDir=$out/share/plymouth/themes/login
    mkdir -p $themeDir

    cp ${../static/login.plymouth} $themeDir/login.plymouth
    cp ${../static/login.script}   $themeDir/login.script

    # Clear wallpaper (idle mode with snow)
    magick ${../static/wallpaper.jpg} \
      -resize 2560x1600^ -gravity center -extent 2560x1600 \
      $themeDir/wallpaper.png

    # Blurred + darkened wallpaper (password mode)
    magick ${../static/wallpaper.jpg} \
      -resize 2560x1600^ -gravity center -extent 2560x1600 \
      -blur 0x10 \
      $themeDir/wallpaper-blur.png

    # Circular avatar
    magick ${../static/pfp.jpg} \
      \( +clone -alpha extract \
        -draw "fill black polygon 0,0 0,%[fx:h/2] %[fx:w/2],0 fill white circle %[fx:w/2],%[fx:h/2] %[fx:w/2],0" \
        \( +clone -flip \) -compose Multiply -composite \
        \( +clone -flop \) -compose Multiply -composite \
      \) -alpha off -compose CopyOpacity -composite \
      $themeDir/avatar.png

    # Password bullet
    magick -size 24x24 xc:none -fill white -draw "circle 12,12 12,2" \
      $themeDir/bullet.png

    # Snowflake (soft white dot)
    magick -size 16x16 xc:none -fill white -draw "circle 8,8 8,3" \
      -blur 0x1.5 \
      $themeDir/snow.png

    substituteInPlace $themeDir/login.plymouth \
      --replace /etc/plymouth-login $themeDir
  '';
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
    theme = "login";
    themePackages = [ loginPlymouthTheme ];
  };

  boot.kernelParams = [ "quiet" "splash" "loglevel=3" "rd.udev.log_level=3" ];
  boot.consoleLogLevel = 0;
  boot.initrd.verbose = false;
  boot.initrd.systemd.enable = true;

  system.stateVersion = "25.11";
}
