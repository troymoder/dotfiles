{
  config,
  lib,
  pkgs,
  variables,
  buildName,
  ...
}: let
  cfg = config.modules.home-manager;
in {
  options.modules.home-manager = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Enable home-manager";
    };

    username = lib.mkOption {
      type = lib.types.str;
      default = variables.username;
      description = "Username of the user account";
    };

    sshKeyPub = lib.mkOption {
      type = lib.types.str;
      default = variables.sshKeyPub;
      description = "Public SSH Key";
    };

    extraGroups = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = ["wheel" "networkmanager"];
      description = "Extra groups for the user";
    };
  };

  config = lib.mkIf cfg.enable {
    home-manager.users.${cfg.username} = import ../../home/${buildName}.nix;

    users.users.${cfg.username} = {
      isNormalUser = true;
      extraGroups = cfg.extraGroups;
      openssh.authorizedKeys.keys = lib.mkIf (cfg.sshKeyPub != "") [
        cfg.sshKeyPub
      ];
      shell = pkgs.bash;
    };
  };
}
