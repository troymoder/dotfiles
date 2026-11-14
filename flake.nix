{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";

    x1e-nixos-config = {
      url = "github:kuruczgy/x1e-nixos-config";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    home-manager = {
      url = "github:nix-community/home-manager";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    vscode-server = {
      url = "github:nix-community/nixos-vscode-server";
    };

    nix-index-database = {
      url = "github:nix-community/nix-index-database";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    technorino = {
      url = "git+https://github.com/2547techno/technorino?submodules=1";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    lanzaboote = {
      url = "github:nix-community/lanzaboote";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    # Night light for COSMIC until it has its own: a root helper writes the
    # colour curve to the display (briefly taking the screen, hence a flicker).
    cosmic-nightlight = {
      url = "github:cosmic-nightlight/cosmic-nightlight/v0.6.0";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    nixpkgs,
    x1e-nixos-config,
    home-manager,
    vscode-server,
    nix-index-database,
    technorino,
    lanzaboote,
    cosmic-nightlight,
    ...
  }: let
    variables = import ./variables.nix;

    mkSystem = {
      buildName,
      system,
      extraModules ? [],
    }: let
      pkgs = import nixpkgs {
        inherit system;
        config.allowUnfree = true;
        overlays = [
          (final: prev: {
            # Keep a package but drop its .desktop files (CLI tools that ship launcher entries).
            noDesktopEntry = pkg:
              final.symlinkJoin {
                name = "${pkg.name}-no-desktop";
                paths = [pkg];
                postBuild = "rm -rf $out/share/applications";
              };

            # Launcher: drop the system76-power scripts (no such daemon here) and ship
            # only the web search shortcuts in use.
            pop-launcher = prev.pop-launcher.overrideAttrs (old: {
              postInstall =
                (old.postInstall or "")
                + ''
                  rm -r $out/share/pop-launcher/scripts/system76-power
                  cp ${./system/launcher/web-shortcuts.ron} $out/share/pop-launcher/plugins/web/config.ron
                '';
            });

            sbctl = prev.sbctl.overrideAttrs (old: {
              version = "0.19-unstable-2026-xx-xx";
              src = final.fetchFromGitHub {
                owner = "troykomodo";
                repo = "sbctl";
                rev = "16c0ce6cebd087b30496771fe8f828db78dc05b9";
                hash = "sha256-FugQYc1m4aJ86MCfbjwQ8smk0PkKrBTYw/oRLrB3LdA=";
              };
              vendorHash = "sha256-7BqYRPCItEvjCQ1oRuoP1BLXyZ2htXjctkMMCiskFHE=";
            });
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
            ({pkgs, ...}: {
              nix.package = pkgs.lix;

              services.envfs.enable = true;
              programs.nix-ld.enable = true;
            })
            home-manager.nixosModules.home-manager
            nix-index-database.nixosModules.default
            vscode-server.nixosModules.default
            lanzaboote.nixosModules.lanzaboote
            cosmic-nightlight.nixosModules.default
            {
              home-manager.useGlobalPkgs = true;
              home-manager.extraSpecialArgs = {
                inherit technorino variables;
              };
              home-manager.sharedModules = [
                nix-index-database.homeModules.default
                ./home/modules
              ];

              systemd.services."systemd-timedated".environment.SYSTEMD_ETC_LOCALTIME = "/etc/localtime";
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
        extraModules = [x1e-nixos-config.nixosModules.x1e];
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
      liveIso = nixpkgs.lib.nixosSystem {
        system = "x86_64-linux";
        modules = [
          "${nixpkgs}/nixos/modules/installer/cd-dvd/installation-cd-graphical-gnome.nix"
          ({pkgs, ...}: {
            environment.systemPackages = with pkgs; [
              vim
              git
              htop
              cryptsetup
            ];
            services.openssh.enable = true;
            networking.hostName = "nixos-live";
          })
        ];
      };
    };

    packages.x86_64-linux.iso = self.nixosConfigurations.liveIso.config.system.build.isoImage;
    formatter = nixpkgs.lib.genAttrs ["x86_64-linux" "aarch64-linux"] (s: nixpkgs.legacyPackages.${s}.alejandra);
  };
}
