{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-25.11";
    nixpkgs-unstable.url = "github:nixos/nixpkgs/nixos-unstable";

    x1e-nixos-config = {
      url = "github:kuruczgy/x1e-nixos-config/v6.18";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    home-manager = {
      url = "github:nix-community/home-manager/release-25.11";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    alejandra = {
      url = "github:kamadorueda/alejandra/4.0.0";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    vscode-server = {
      url = "github:nix-community/nixos-vscode-server/6d5f074e4811d143d44169ba4af09b20ddb6937d";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    nix-ld = {
      url = "github:Mic92/nix-ld/2.0.6";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    envfs = {
      url = "github:Mic92/envfs/1.1.0";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    nix-your-shell = {
      url = "github:TroyKomodo/nix-your-shell/0c45887935c3507b2ab00b64dac61311fac01d4f";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    nix-index-database = {
      url = "github:nix-community/nix-index-database";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    technorino = {
      url = "git+https://github.com/2547techno/technorino?submodules=1";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    nixpkgs,
    x1e-nixos-config,
    home-manager,
    alejandra,
    vscode-server,
    nix-ld,
    envfs,
    nix-your-shell,
    nixpkgs-unstable,
    nix-index-database,
    technorino,
    ...
  }: let
    mkSystem = {
      buildName,
      system,
      timeZone,
      extraModules ? [],
    }: let
      pkgs-unstable = import nixpkgs-unstable {
        inherit system;
        config.allowUnfree = true;
      };
      pkgs = import nixpkgs {
        inherit system;
        config.allowUnfree = true;
        overlays = [
          nix-your-shell.overlays.default
          (final: prev: {
            tlp = prev.tlp.overrideAttrs (oldAttrs: rec {
              version = "1.9.0";
              src = oldAttrs.src.override {
                rev = "1.9.0";
                hash = "sha256-aM/4+cgtUe6qv3MNT4moXvNzqG5gKvwMbg14L8ifWlc=";
              };
            });
          })
        ];
      };
    in
      nixpkgs.lib.nixosSystem {
        inherit system pkgs;
        specialArgs = {inherit buildName timeZone;};
        modules =
          [
            home-manager.nixosModules.home-manager
            nix-ld.nixosModules.nix-ld
            envfs.nixosModules.envfs
            {
              home-manager.sharedModules = [
                nix-index-database.homeModules.default
              ];
            }
            ./system/common.nix
            ./system/${buildName}.nix
          ]
          ++ extraModules;
      };
  in {
    nixosConfigurations = {
      thinkpad = mkSystem {
        buildName = "thinkpad";
        system = "aarch64-linux";
        timeZone = "America/Denver";
        extraModules = [
          x1e-nixos-config.nixosModules.x1e
          {
            home-manager.extraSpecialArgs = {inherit technorino;};
            home-manager.sharedModules = [];
          }
        ];
      };
      server = mkSystem {
        buildName = "server";
        system = "x86_64-linux";
        timeZone = "America/Toronto";
        extraModules = [vscode-server.nixosModules.default];
      };
    };

    formatter = alejandra.defaultPackage;
  };
}
