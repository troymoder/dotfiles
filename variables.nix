{
  username = "troy";
  git = {
    fullName = "Troy Benson";
    email = "me@troymoder.dev";
  };
  sshKeyPub = builtins.readFile ./id_ed25519.pub;
}
