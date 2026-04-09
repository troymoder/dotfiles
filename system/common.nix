{
  pkgs,
  buildName,
  config,
  lib,
  ...
}: let
  variables = import ../variables.nix;
in {
  nix = {
    channel.enable = false;
    settings.experimental-features = ["nix-command" "flakes" "flake-self-attrs"];
  };

  home-manager.useGlobalPkgs = true;
  home-manager.useUserPackages = true;
  home-manager.backupFileExtension = "bak";

  services.tailscale.enable = true;

  networking.nameservers = ["1.1.1.1" "1.0.0.1"];

  networking.firewall = {
    trustedInterfaces = ["tailscale0"];
    allowedTCPPorts = [22];
    allowedUDPPorts = [41641 1900 5351];
  };

  services.resolved = {
    enable = true;
    dnssec = "true";
    domains = ["~."];
    fallbackDns = config.networking.nameservers;
    dnsovertls = "opportunistic";
  };

  # Docker (rootless)
  virtualisation.docker.rootless = {
    enable = true;
    setSocketVariable = true;
    daemon.settings = {
      dns = config.networking.nameservers;
      registry-mirrors = ["https://mirror.gcr.io"];
      features = {
        buildkit = true;
        containerd-snapshotter = true;
        cdi = true;
      };
    };
  };

  # Docker environment variables (set globally)
  environment.sessionVariables = {
    DOCKER_HOST = "unix://$XDG_RUNTIME_DIR/docker.sock";
    CONTAINERD_ADDRESS = "$XDG_RUNTIME_DIR/docker/containerd/containerd.sock";
  };

  users.users.${variables.username} = {
    isNormalUser = true;
    extraGroups = ["wheel" "networkmanager" "docker"];
    openssh.authorizedKeys.keys = [
      variables.sshKeyPub
    ];
    shell = pkgs.bash;
  };

  security.sudo.wheelNeedsPassword = false;

  services.openssh = {
    enable = true;
    settings.PasswordAuthentication = false;
    settings.PermitRootLogin = "no";
  };

  environment.sessionVariables = {
    ENVFS_RESOLVE_ALWAYS = "1";
  };

  environment.systemPackages = with pkgs; [
    (writeShellScriptBin "nixos-rebuild-flake" ''
      ${nixos-rebuild}/bin/nixos-rebuild --flake .#${buildName} $@
    '')
    neovim
    git
    coreutils
    nixos-rebuild
  ];

  networking.hostName = "${variables.username}-${buildName}";
  home-manager.users.${variables.username} = import ../home/${buildName}.nix;
  nix.settings.trusted-users = ["@wheel" "root"];
}
