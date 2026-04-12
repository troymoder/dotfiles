# Example: what hetznerVpn.nix looks like now
{...}: {
  modules = {
    cli-tools.enable = true;
    fish.enable = true;
    direnv.enable = true;
    git.enable = true;
    ssh.enable = true;
  };

  programs.home-manager.enable = true;
  home.stateVersion = "25.11";
}
