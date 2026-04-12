{
  config,
  lib,
  ...
}: let
  cfg = config.modules.vscode-server;
in {
  options.modules.vscode-server = {
    enable = lib.mkEnableOption "VS Code remote server";
  };

  config = lib.mkIf cfg.enable {
    services.vscode-server = {
      enable = true;
      enableFHS = true;
      installPath = ["$HOME/.vscode-server"];
    };
  };
}
