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
    enable = lib.mkEnableOption "Enable home-manager";

    username = lib.mkOption {
      type = lib.types.str;
      default = variables.username;
      description = "Username of the user account";
    };

    profilePicture = lib.mkOption {
      type = lib.types.path;
      default = variables.profilePicture;
      description = "Profile picture of the account";
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

    system.activationScripts.userIcon = ''
      mkdir -p /var/lib/AccountsService/{icons,users}
      cp ${cfg.profilePicture} /var/lib/AccountsService/icons/${cfg.username}
      chmod 0644 /var/lib/AccountsService/icons/${cfg.username}

      cat > /var/lib/AccountsService/users/${cfg.username} <<EOF
      [User]
      Icon=/var/lib/AccountsService/icons/${cfg.username}
      SystemAccount=false
      EOF
    '';
  };
}
