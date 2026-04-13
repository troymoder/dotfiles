{
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.modules.fish;
in {
  options.modules.fish = {
    enable = lib.mkEnableOption "Fish shell";
  };

  config = lib.mkIf cfg.enable {
    programs.fish = {
      enable = true;

      interactiveShellInit = ''
        set fish_greeting
        export SHELL="${pkgs.fish}/bin/fish"
        ${pkgs.nix-your-shell}/bin/nix-your-shell fish --info-right | source
      '';

      shellAliases = {
        gp = "git push";
        gs = "git status";
        gd = "git diff";
        gl = "git lg";
        vim = "nvim";
        vi = "nvim";
        ls = "eza";
        cat = "bat";
        ".." = "cd ..";
        "..." = "cd ../..";
        "...." = "cd ../../..";
      };

      functions = {
        gcl = "git clone $argv[1] && cd (basename $argv[1] .git)";
        mkcd = "mkdir -p $argv[1] && cd $argv[1]";
      };
    };

    # Auto-launch Fish from Bash
    programs.bash = {
      enable = true;
      initExtra = ''
        if [[ -z $IN_NIX_SHELL && $(${pkgs.procps}/bin/ps --no-header --pid=$PPID --format=comm) != "fish" && -z ''${BASH_EXECUTION_STRING} ]]; then
          shopt -q login_shell && LOGIN_OPTION='--login' || LOGIN_OPTION=""
          exec ${pkgs.fish}/bin/fish $LOGIN_OPTION
        fi
      '';
    };
  };
}
