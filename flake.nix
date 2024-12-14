{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils, ... }: flake-utils.lib.eachDefaultSystem (system:
    let
      pkgs = import nixpkgs { inherit system; };
      vgabios = pkgs.callPackage ./vgabios.nix { };
    in
    {
      devShells.default = with pkgs; pkgs.mkShell {
        buildInputs = [
          (symlinkJoin {
            name = "qemu";
            paths = [ pkgs.qemu vgabios ];
            postBuild = "
              rm $out/share/qemu/vgabios-stdvga.bin
              cp $out/VGABIOS-lgpl-latest.stdvga.bin $out/share/qemu/vgabios-stdvga.bin
            ";
          })
          (pkgs.writeShellScriptBin "qemu-system-x86_64-uefi" ''
            qemu-system-x86_64 \
              -bios ${pkgs.OVMF.fd}/FV/OVMF.fd \
              "$@"
          '')
        ];
      };
    });
}
