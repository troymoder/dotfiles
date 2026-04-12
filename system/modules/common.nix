{
  pkgs,
  config,
  lib,
  buildName,
  variables,
  ...
}: {
  config = {
    nix = {
      channel.enable = false;
      settings.experimental-features = ["nix-command" "flakes" "flake-self-attrs"];
      settings.trusted-users = ["@wheel" "root"];
    };

    hardware.enableAllFirmware = true;

    environment.systemPackages = with pkgs; [
      (writeShellScriptBin "nixos-rebuild-flake" ''
        ${nixos-rebuild}/bin/nixos-rebuild --flake .#${buildName} $@
      '')
      neovim
      git
      coreutils
    ];

    networking.hostName = lib.mkDefault "${variables.username}-${buildName}";
  };
}
