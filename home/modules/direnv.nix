{
  config,
  lib,
  ...
}: let
  cfg = config.modules.direnv;
in {
  options.modules.direnv = {
    enable = lib.mkEnableOption "direnv with nix-direnv";
  };

  config = lib.mkIf cfg.enable {
    programs.direnv = {
      enable = true;
      nix-direnv.enable = true;
    };
  };
}
