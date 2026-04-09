{
  config,
  pkgs,
  ...
}: let
  variables = import ../variables.nix;
in {
  home.username = variables.username;
  home.homeDirectory = "/home/${variables.username}";

  # Core CLI utilities
  home.packages = with pkgs; [
    # System monitoring
    htop
    btop

    # Modern replacements
    eza # modern ls
    bat # modern cat
    ripgrep # modern grep
    fd # modern find
    ldns # modern dig

    # CLI tools
    fzf
    jq
    tree
    wget
    curl
    file
    which
    gnupg

    # Archive tools
    unzip
    zip
    xz
    zstd
    gzip

    # Development
    git
    gh # GitHub CLI
    mergiraf
    neovim
    rustup
    gcc15
    uv

    # System tools
    iperf3
    btop
    iftop
    iotop
    strace
    ltrace
    lsof
    ethtool
    pciutils # lspci
    usbutils # lsusb

    # Shell helpers
    direnv
    nix-your-shell

    # System info
    fastfetch
    hyfetch
  ];

  home.file.".ssh/id_ed25519.pub".text = variables.sshKeyPub;
  home.file.".gitattributes".text = "";

  # Git configuration
  programs.git = {
    enable = true;
    settings = {
      user = {
        name = variables.name;
        email = variables.email;
      };

      # Credentials & authentication
      credential.helper = "${pkgs.gh}/bin/gh auth git-credential";

      # Push/pull defaults
      push.autoSetupRemote = true;
      pull.rebase = false;

      # GPG signing with SSH
      gpg.format = "ssh";
      user.signingkey = "${config.home.homeDirectory}/.ssh/id_ed25519.pub";
      commit.gpgsign = true;
      tag.gpgsign = true;

      # Merge configuration
      merge.conflictStyle = "diff3";
      merge.mergiraf = {
        name = "mergiraf";
        driver = "${pkgs.mergiraf}/bin/mergiraf merge --git %O %A %B -s %S -x %X -y %Y -p %P -l %L";
      };

      # Core settings
      core.attributesFile = "${config.home.homeDirectory}/.gitattributes";
      init.defaultBranch = "main";

      # Git aliases
      aliases = {
        st = "status";
        co = "checkout";
        br = "branch";
        ci = "commit";
        lg = "log --graph --pretty=format:'%Cred%h%Creset -%C(yellow)%d%Creset %s %Cgreen(%cr) %C(bold blue)<%an>%Creset' --abbrev-commit";
      };
    };
  };

  # Direnv integration
  programs.direnv = {
    enable = true;
    nix-direnv.enable = true;
  };

  # SSH configuration
  programs.ssh = {
    enable = true;
    enableDefaultConfig = false;
    matchBlocks."*" = {
      forwardAgent = true;
      addKeysToAgent = "yes";
    };
  };

  # SSH agent service
  services.ssh-agent.enable = true;

  programs.nix-index.enable = true;
  programs.command-not-found.enable = false;

  # Fish shell
  programs.fish = {
    enable = true;

    interactiveShellInit = ''
      # Disable greeting
      set fish_greeting

      ${pkgs.nix-your-shell}/bin/nix-your-shell fish --info-right | source
    '';

    shellAliases = {
      # Git shortcuts
      gp = "git push";
      gs = "git status";
      gd = "git diff";
      gl = "git lg";

      # Editor
      vim = "nvim";
      vi = "nvim";

      # Modern tool replacements
      ls = "eza";
      cat = "bat";

      dig = "ldns";

      # Navigation
      ".." = "cd ..";
      "..." = "cd ../..";
      "...." = "cd ../../..";
    };

    functions = {
      # Git clone and cd
      gcl = "git clone $argv[1] && cd (basename $argv[1] .git)";

      # Make directory and cd into it
      mkcd = "mkdir -p $argv[1] && cd $argv[1]";
    };
  };

  # Bash - auto-launch Fish
  programs.bash = {
    enable = true;

    initExtra = ''
      # Auto-launch Fish shell if not already in Fish
      if [[ $(${pkgs.procps}/bin/ps --no-header --pid=$PPID --format=comm) != "fish" && -z ''${BASH_EXECUTION_STRING} ]]; then
        shopt -q login_shell && LOGIN_OPTION='--login' || LOGIN_OPTION=""
        exec ${pkgs.fish}/bin/fish $LOGIN_OPTION
      fi
    '';
  };

  # Let Home Manager manage itself
  programs.home-manager.enable = true;

  # Global environment variables
  home.sessionVariables = {
    EDITOR = "nvim";
    VISUAL = "nvim";
  };
}
