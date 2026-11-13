{pkgs, ...}: {
  imports = [
    ./profiles/base.nix
  ];

  modules = {
    dev-tools = {
      enable = true;
      rust = true;
      python = true;
      go = true;
      bazel = true;
      protobuf = true;
      extraPackages = with pkgs; [pkg-config cmake ninja google-cloud-sdk];
    };
  };

  programs.home-manager.enable = true;
  home.stateVersion = "25.05";
}
