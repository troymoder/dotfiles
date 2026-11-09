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
      # Our own pinentry on desktops, in the quick settings' style; curses on
      # headless hosts.
      pinentry.package =
        if config.modules.desktop-apps.enable
        then config.modules.cabin.package
        else pkgs.pinentry-curses;
      pinentry.program = "cabin-pinentry";
      defaultCacheTtl = 34560000;
      maxCacheTtl = 34560000;
      defaultCacheTtlSsh = 34560000;
      maxCacheTtlSsh = 34560000;
    };

    # gpg-agent reads its config only at startup, so a switch that changes the
    # pinentry would otherwise keep the old one until the next login.
    home.file."${config.programs.gpg.homedir}/gpg-agent.conf".onChange = ''
      ${config.programs.gpg.package}/bin/gpgconf --reload gpg-agent || true
    '';
  };
}
