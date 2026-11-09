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
      settings.experimental-features = ["nix-command" "flakes"];
      settings.trusted-users = ["@wheel" "root"];
    };

    hardware.enableAllFirmware = lib.mkDefault true;
    # Below mkDefault so hardware modules (e.g. x1e-nixos-config) can set their own.
    boot.kernelPackages = lib.mkOverride 1001 pkgs.linuxPackages_latest;

    environment.systemPackages = with pkgs; [
      (writeShellScriptBin "nixos-rebuild-flake" ''
        rebuild=(${nixos-rebuild}/bin/nixos-rebuild --flake .#${buildName} "$@")
        # Builds go through nom; these need the terminal untouched.
        for arg in "$@"; do
          case $arg in
            edit | repl | list-generations | -h | --help) exec "''${rebuild[@]}" ;;
          esac
        done
        set -o pipefail
        "''${rebuild[@]}" --log-format internal-json |& ${nix-output-monitor}/bin/nom --json
      '')
      (noDesktopEntry neovim)
      git
      coreutils
    ];

    # No "NixOS Manual" launcher entry.
    documentation.nixos.enable = false;

    services.fwupd.enable = lib.mkDefault true;
    networking.networkmanager.enable = lib.mkDefault true;
    networking.hostName = lib.mkDefault "${variables.username}-${buildName}";
  };
}
