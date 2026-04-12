{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-25.11";
    nixpkgs-unstable.url = "github:nixos/nixpkgs/nixos-unstable";

    x1e-nixos-config = {
      url = "github:kuruczgy/x1e-nixos-config";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    home-manager = {
      url = "github:nix-community/home-manager/release-25.11";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    alejandra = {
      url = "github:kamadorueda/alejandra";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    vscode-server = {
      url = "github:nix-community/nixos-vscode-server";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    nix-ld = {
      url = "github:Mic92/nix-ld";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    envfs = {
      url = "github:Mic92/envfs";
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

    lix = {
      url = "github:lix-project/lix";
      flake = false;
    };

    lix-module = {
      url = "git+https://git.lix.systems/lix-project/nixos-module";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.lix.follows = "lix";
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
    lix-module,
    ...
  }: let
    variables = import ./variables.nix;

    mkSystem = {
      buildName,
      system,
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
            vscode = pkgs-unstable.vscode;
          })
        ];
      };
    in
      nixpkgs.lib.nixosSystem {
        inherit system pkgs;
        specialArgs = {
          inherit buildName nixpkgs variables;
        };
        modules =
          [
            lix-module.nixosModules.default
            ({
              pkgs,
              lib,
              ...
            }: {
              nix.package = lib.mkForce (pkgs.lix.overrideAttrs (old: {
                doCheck = false;
                doInstallCheck = false;
              }));
            })
            nix-ld.nixosModules.nix-ld
            envfs.nixosModules.envfs
            home-manager.nixosModules.home-manager
            nix-index-database.nixosModules.default
            vscode-server.nixosModules.default
            x1e-nixos-config.nixosModules.x1e
            {
              home-manager.useGlobalPkgs = true;
              home-manager.extraSpecialArgs = {
                inherit technorino variables;
              };
              home-manager.sharedModules = [
                nix-index-database.homeModules.default
                ./home/modules
              ];
            }
            ./system/modules
            ./system/${buildName}.nix
          ]
          ++ extraModules;
      };
  in {
    nixosConfigurations = {
      thinkpad = mkSystem {
        buildName = "thinkpad";
        system = "aarch64-linux";
      };
      framework = mkSystem {
        buildName = "framework";
        system = "x86_64-linux";
      };
      server = mkSystem {
        buildName = "server";
        system = "x86_64-linux";
      };
      hetznerVpn = mkSystem {
        buildName = "hetznerVpn";
        system = "x86_64-linux";
      };
    };

    formatter = alejandra.defaultPackage;
  };
}
