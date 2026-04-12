# modules/default.nix
let
  dir = ./.;
  files = builtins.readDir dir;
  nixFiles =
    builtins.filter
    (name: name != "default.nix" && builtins.match ".*\\.nix" name != null)
    (builtins.attrNames files);
in {
  imports = map (name: dir + "/${name}") nixFiles;
}
