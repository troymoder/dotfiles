{
  config,
  lib,
  pkgs,
  variables,
  buildName,
  ...
}: let
  cfg = config.modules.home-manager;
  inherit (variables) username;
in {
  options.modules.home-manager = {
    enable = lib.mkEnableOption "Enable home-manager";

    extraGroups = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [];
      description = "Extra groups for the user";
    };
  };

  config = lib.mkIf cfg.enable {
    home-manager.users.${username} = import ../../home/${buildName}.nix;

    users.users.${username} = {
      isNormalUser = true;
      extraGroups = ["wheel" "networkmanager"] ++ cfg.extraGroups;
      openssh.authorizedKeys.keys = [variables.sshKeyPub];
      shell = pkgs.bash;
    };

    system.activationScripts.userIcon = ''
      mkdir -p /var/lib/AccountsService/{icons,users}
      cp ${variables.profilePicture} /var/lib/AccountsService/icons/${username}
      chmod 0644 /var/lib/AccountsService/icons/${username}

      cat > /var/lib/AccountsService/users/${username} <<EOT
      [User]
      Icon=/var/lib/AccountsService/icons/${username}
      SystemAccount=false
      EOT
    '';
  };
}
