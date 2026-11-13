# cabin-ec: the USB-C charger as the Framework EC sees it, on the system bus,
# for the cabin Battery menu. Built from the crate next to this file.
{
  lib,
  rustPlatform,
}:
rustPlatform.buildRustPackage {
  pname = "cabin-ec";
  version = "0.1.0";

  src = lib.cleanSourceWith {
    src = ./.;
    filter = path: _type: let
      name = baseNameOf path;
    in
      name != "target" && name != "package.nix";
  };

  cargoLock.lockFile = ./Cargo.lock;

  meta = {
    description = "The Framework laptop's USB-C charger as its EC sees it, for the cabin panels";
    license = lib.licenses.gpl3Only;
    mainProgram = "cabin-ec";
  };
}
