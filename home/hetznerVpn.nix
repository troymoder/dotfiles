# Example: what hetznerVpn.nix looks like now
{...}: {
  imports = [
    ./profiles/base.nix
  ];

  modules = {
  };

  programs.home-manager.enable = true;
  home.stateVersion = "25.11";
}
