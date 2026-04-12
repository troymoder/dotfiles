{
  config,
  lib,
  pkgs,
  variables,
  ...
}: let
  cfg = config.modules.git;
in {
  options.modules.git = {
    enable = lib.mkEnableOption "Git configuration";

    fullName = lib.mkOption {
      type = lib.types.str;
      description = "Git user name";
      default = variables.git.fullName;
    };

    userEmail = lib.mkOption {
      type = lib.types.str;
      description = "Git user email";
      default = variables.git.email;
    };

    signing = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable SSH-based commit signing";
      };

      key = lib.mkOption {
        type = lib.types.str;
        description = "SSH Signing Key path";
        default = builtins.toFile "signing-key" variables.sshKeyPub;
      };
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = with pkgs; [gh mergiraf];

    home.file.".gitattributes".text = "";

    programs.git = {
      enable = true;
      settings = {
        user = {
          name = cfg.fullName;
          email = cfg.userEmail;
        };

        credential.helper = "${pkgs.gh}/bin/gh auth git-credential";

        push.autoSetupRemote = true;
        pull.rebase = false;

        gpg.format = lib.mkIf cfg.signing.enable "ssh";
        user.signingkey = lib.mkIf cfg.signing.enable cfg.signing.key;
        commit.gpgsign = cfg.signing.enable;
        tag.gpgsign = cfg.signing.enable;

        merge.conflictStyle = "diff3";
        merge.mergiraf = {
          name = "mergiraf";
          driver = "${pkgs.mergiraf}/bin/mergiraf merge --git %O %A %B -s %S -x %X -y %Y -p %P -l %L";
        };

        core.attributesFile = "${config.home.homeDirectory}/.gitattributes";
        init.defaultBranch = "main";

        aliases = {
          st = "status";
          co = "checkout";
          br = "branch";
          ci = "commit";
          lg = "log --graph --pretty=format:'%Cred%h%Creset -%C(yellow)%d%Creset %s %Cgreen(%cr) %C(bold blue)<%an>%Creset' --abbrev-commit";
        };
      };
    };
  };
}
