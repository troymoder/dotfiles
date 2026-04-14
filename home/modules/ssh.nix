{
  pkgs,
  config,
  lib,
  variables,
  ...
}: let
  cfg = config.modules.ssh;
  gnomeDesktopEnabled = (config.modules ? gnome-desktop) && (config.modules.gnome-desktop.enable or false);
  pinentryPackage =
    if cfg.pinentryPackage != null
    then cfg.pinentryPackage
    else if gnomeDesktopEnabled
    then pkgs.pinentry-gnome3
    else pkgs.pinentry-curses;
in {
  options.modules.ssh = {
    enable = lib.mkEnableOption "SSH client config";

    forwardAgent = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Forward SSH agent to remote hosts";
    };

    pinentryPackage = lib.mkOption {
      type = lib.types.nullOr lib.types.package;
      default = null;
      description = "Pinentry package for gpg-agent (auto: gnome3 on desktop, curses on headless hosts)";
    };
  };

  config = lib.mkIf cfg.enable {
    programs.ssh = {
      enable = true;
      enableDefaultConfig = false;
      matchBlocks."*" = {
        forwardAgent = cfg.forwardAgent;
        addKeysToAgent = "yes";
      };
    };

    programs.gpg = {
      enable = true;
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
      pinentry.package = pinentryPackage;
      defaultCacheTtl = 34560000;
      maxCacheTtl = 34560000;
      defaultCacheTtlSsh = 34560000;
      maxCacheTtlSsh = 34560000;
    };
  };
}
