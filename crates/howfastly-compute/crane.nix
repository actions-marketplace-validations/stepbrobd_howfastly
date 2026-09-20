{ crane, pkgs, ... }:

{
  cargoArtifacts = crane.lib.buildDepsOnly (crane.commonArgs // {
    pname = "howfastly-compute";
    cargoExtraArgs = "--package howfastly-compute";
    doCheck = false;
    env.CARGO_BUILD_TARGET = "wasm32-wasip1";
  });

  src = crane.fileSetForCrates [ ../howfastly ../howfastly-compute ];

  env.CARGO_BUILD_TARGET = "wasm32-wasip1";
  # web and cells come from the flake self overlay
  env.WEB_DIST = "${pkgs.howfastly.web}";
  env.CELLS = "${pkgs.howfastly.cells}";
  # the wasm reports the path it was built into, see meta in handlers.rs
  env.HOWFASTLY_OUTPATH = builtins.placeholder "out";
}
