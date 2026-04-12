{
  config,
  lib,
  variables,
  ...
}: let
  cfg = config.modules.ssh;
in {
  options.modules.ssh = {
    enable = lib.mkEnableOption "SSH client config";

    publicKey = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = variables.sshKeyPub;
      description = "SSH public key to write to ~/.ssh/id_ed25519.pub";
    };

    forwardAgent = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Forward SSH agent to remote hosts";
    };
  };

  config = lib.mkIf cfg.enable {
    home.file.".ssh/id_ed25519.pub" = lib.mkIf (cfg.publicKey != null) {
      text = cfg.publicKey;
    };

    programs.ssh = {
      enable = true;
      enableDefaultConfig = false;
      matchBlocks."*" = {
        forwardAgent = cfg.forwardAgent;
        addKeysToAgent = "yes";
      };
    };

    services.ssh-agent.enable = true;
  };
}
