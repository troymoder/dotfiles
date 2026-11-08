{
  pkgs,
  config,
  lib,
  ...
}: let
  cfg = config.modules.ssh;
in {
  options.modules.ssh = {
    enable = lib.mkEnableOption "SSH client config";
  };

  config = lib.mkIf cfg.enable {
    programs.ssh = {
      enable = true;
      enableDefaultConfig = false;
      settings."*" = {
        ForwardAgent = true;
        AddKeysToAgent = "yes";
      };
    };

    programs.gpg = {
      enable = true;
      settings = {
        keyserver = "hkps://keyserver.ubuntu.com";
      };
      scdaemonSettings = {
        disable-ccid = true;
        pcsc-shared = true;
        disable-application = "piv";
      };
    };

    services.ssh-agent.enable = false;

    services.gpg-agent = {
      enable = true;
      grabKeyboardAndMouse = false;
      enableSshSupport = true;
      enableScDaemon = true;
      # gnome3 pinentry on desktops, curses on headless hosts
      pinentry.package =
        if config.modules.gnome-desktop.enable
        then pkgs.pinentry-gnome3
        else pkgs.pinentry-curses;
      defaultCacheTtl = 34560000;
      maxCacheTtl = 34560000;
      defaultCacheTtlSsh = 34560000;
      maxCacheTtlSsh = 34560000;
    };
  };
}
