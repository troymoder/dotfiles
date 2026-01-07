{
  pkgs,
  buildName,
  timeZone,
  config,
  ...
}: let
  variables = import ../variables.nix;
in {
  nix = {
    channel.enable = false;
    settings.experimental-features = ["nix-command" "flakes"];
  };

  home-manager.useGlobalPkgs = true;
  home-manager.useUserPackages = true;
  home-manager.backupFileExtension = "bak";

  services.tailscale.enable = true;

  networking.nameservers = ["1.1.1.1" "1.0.0.1"];

  services.resolved = {
    enable = true;
    dnssec = "true";
    domains = ["~."];
    fallbackDns = config.networking.nameservers;
    dnsovertls = "opportunistic";
  };

  users.users.${variables.username} = {
    isNormalUser = true;
    extraGroups = ["wheel" "networkmanager"];
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

  time.timeZone = timeZone;

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
  nix.settings.trusted-users = [ "@wheel" "root" ];
}
