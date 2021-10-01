with import <nixpkgs> { };
mkShell {
  buildInputs = [
    rust-bin.nightly."2021-06-08".default
    bashInteractive
  ];
}
