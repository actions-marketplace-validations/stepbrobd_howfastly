{ callPackage, inputs, lib, pkgsFinal, stdenv }:

inputs.self.legacyPackages.${stdenv.hostPlatform.system}.crates.howfastly.overrideAttrs (old: {
  meta.mainProgram = "howfastly";

  # the dist is a trunk artifact and the cells a generator run rather than crate outputs
  # hang them off the cli so they stay out of the top level package set
  passthru = (old.passthru or { }) // {
    web = callPackage ./web.nix { inherit lib pkgsFinal; };
    cells = callPackage ./cells.nix { inherit inputs; };
  };
})
