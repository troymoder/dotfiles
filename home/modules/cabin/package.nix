# Cabin: Troy's panels for COSMIC, one fixed layout, built from the crate
# next to this file. cabind draws the panels; cabin-applet is the buttons
# on the panel; cabinctl drives them from a shell.
{
  lib,
  makeRustPlatform,
  rust,
  libcosmicAppHook,
  pipewire,
  wireplumber,
  pulseaudio,
  curl,
  pciutils,
  ffmpeg,
  wl-clipboard,
  cantarell-fonts,
  fetchFromGitHub,
}: let
  # The official Rust binaries, same version as nixpkgs' rustc: that one is
  # built from source against a shared libLLVM and took 2m31s to build this
  # crate, against 1m41s with these.
  rustPlatform = makeRustPlatform {inherit (rust.packages.prebuilt) rustc cargo;};

  # Tabler Icons, the outline set, embedded by name at build time.
  tabler-icons = fetchFromGitHub {
    owner = "tabler";
    repo = "tabler-icons";
    rev = "v3.51.0";
    hash = "sha256-DouIBFdzy/ENlzSCiMkn1iYakO6i66IvKEuTwm4vWRQ=";
  };
in
  rustPlatform.buildRustPackage {
    pname = "cabin";
    version = "0.1.0";

    src = lib.cleanSourceWith {
      src = ./.;
      filter = path: _type: let
        name = baseNameOf path;
      in
        name != "target" && name != ".dev" && name != "package.nix" && !(lib.hasPrefix "run-dev" name) && name != "reload-dev.sh" && name != "build-dev.sh";
    };

    # All of Cargo.lock vendored in one fixed-output build (git crates too), so
    # evaluating the system doesn't fetch each libcosmic crate. Change the
    # Cargo.lock and the build fails on a hash mismatch: paste the new hash here.
    cargoHash = "sha256-Ao4/5jquXGuXlbv0Wv+Fu5PDOK3mxV7c0+G40CrJ/lQ=";

    nativeBuildInputs = [libcosmicAppHook];

    # The tests want the dev loop (a camera, the session bus): the package just builds.
    doCheck = false;

    TABLER_ICONS_DIR = "${tabler-icons}/icons/outline";

    # Incremental (Cargo.toml, for the dev loop) only costs time in a clean build.
    CARGO_PROFILE_RELEASE_INCREMENTAL = "false";
    # Link through Nix's cc wrapper, not the bundled rust-lld: the wrapper is
    # what puts the libraries' store paths in RUNPATH.
    NIX_RUSTFLAGS = "-C linker-features=-lld";

    # It shells out to these for sound and cover art, and cabin-capture to
    # ffmpeg (recordings, with PulseAudio for their sound; the headless one
    # has none), pw-cli/pw-link (an app's sound) and wl-copy (screenshots).
    preFixup = ''
      libcosmicAppWrapperArgs+=(--prefix PATH : ${lib.makeBinPath [pipewire wireplumber pulseaudio curl pciutils ffmpeg wl-clipboard]})
      libcosmicAppWrapperArgs+=(--set QS_FONT_DIR ${cantarell-fonts}/share/fonts)
    '';

    postInstall = ''
      mkdir -p $out/share/applications $out/share/dbus-1/services
      for id in dev.troy.Cabin dev.troy.Cabin.Clock dev.troy.Cabin.System dev.troy.Cabin.Workspaces; do
        sed "s|@bindir@|$out/bin|" data/$id.desktop.in > $out/share/applications/$id.desktop
      done
      # Started on the first call if the session hasn't started it yet.
      sed "s|@bindir@|$out/bin|" data/dev.troy.Cabin.service.in > $out/share/dbus-1/services/dev.troy.Cabin.service
      id=dev.troy.Cabin
      install -Dm644 data/$id.svg $out/share/icons/hicolor/scalable/apps/$id.svg
    '';

    meta = {
      description = "Troy's panels for COSMIC: quick settings, date menu, System monitor";
      license = lib.licenses.gpl3Only;
      mainProgram = "cabind";
    };
  }
