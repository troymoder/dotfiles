{lib, ...}: {
  modules = {
    cli-tools.enable = lib.mkDefault true;
    direnv.enable = lib.mkDefault true;
    fish.enable = lib.mkDefault true;
    git.enable = lib.mkDefault true;
    ssh.enable = lib.mkDefault true;
  };
}
