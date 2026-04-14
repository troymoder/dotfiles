{
  config,
  lib,
  ...
}: let
  cfg = config.modules.ssh;
in {
  options.modules.ssh = {
    enable = lib.mkEnableOption "Enable openssh";

    passwordAuth = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Allow password authentication";
    };

    permitRootLogin = lib.mkOption {
      type = lib.types.enum ["yes" "no" "prohibit-password" "forced-commands-only"];
      default = "no";
      description = "Whether root can login via SSH";
    };
  };

  config = lib.mkIf cfg.enable {
    services.openssh = {
      enable = true;
      settings = {
        PasswordAuthentication = cfg.passwordAuth;
        PermitRootLogin = cfg.permitRootLogin;
      };
    };

    networking.firewall.allowedTCPPorts = [22];
  };
}
