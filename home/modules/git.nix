{
  config,
  lib,
  pkgs,
  variables,
  ...
}: let
  cfg = config.modules.git;

  # Test suite aborts with heap corruption in the nix sandbox; binary builds fine.
  mergiraf = pkgs.mergiraf.overrideAttrs (_: {doCheck = false;});
in {
  options.modules.git = {
    enable = lib.mkEnableOption "Git configuration";
  };

  config = lib.mkIf cfg.enable {
    home.packages = [pkgs.gh mergiraf];

    home.file.".gitattributes".text = "";

    programs.git = {
      enable = true;
      settings = {
        user = {
          name = variables.git.fullName;
          email = variables.git.email;
          signingkey = variables.sshKeyPub;
        };

        url."git@github.com:".pushInsteadOf = "https://github.com/";

        push.autoSetupRemote = true;
        pull.rebase = false;

        gpg.format = "ssh";
        commit.gpgsign = true;
        tag.gpgsign = true;

        merge.conflictStyle = "diff3";
        merge.mergiraf = {
          name = "mergiraf";
          driver = "${mergiraf}/bin/mergiraf merge --git %O %A %B -s %S -x %X -y %Y -p %P -l %L";
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
