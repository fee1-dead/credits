{
  stdenv,
  fetchFromGitHub,
  dev86
}:

stdenv.mkDerivation {
  pname = "qemu-vgavios";
  version = "0.0.0";

  src = fetchFromGitHub {
    owner = "qemu";
    repo = "vgabios";
    rev = "19ea12c230ded95928ecaef0db47a82231c2e485";
    sha256 = "sha256-nB6QyDsH7mvbbmtIRGXrvYrEkpyRer2/sje1dBQgD6w=";
  };

  patches = [ ./addresolutions.patch ];
   
  buildPhase = ''
    make stdvga-bios
  '';
  installPhase = ''
    install -D VGABIOS-lgpl-latest.stdvga.bin -t $out/
  '';


  nativeBuildInputs = [ dev86 ];
}