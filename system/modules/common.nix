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

    hardware.enableAllFirmware = lib.mkDefault true;
    # Below mkDefault so hardware modules (e.g. x1e-nixos-config) can set their own.
    boot.kernelPackages = lib.mkOverride 1001 pkgs.linuxPackages_latest;

    environment.systemPackages = with pkgs; [
      (writeShellScriptBin "nixos-rebuild-flake" ''
        ${nixos-rebuild}/bin/nixos-rebuild --flake .#${buildName} $@
      '')
      neovim
      git
      coreutils
    ];

    services.fwupd.enable = lib.mkDefault true;
    networking.networkmanager.enable = lib.mkDefault true;
    networking.hostName = lib.mkDefault "${variables.username}-${buildName}";
  };
}
