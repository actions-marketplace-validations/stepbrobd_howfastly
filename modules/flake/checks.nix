{
  perSystem = { config, crane, lib, pkgs, ... }:
    let
      crate = name: ../../crates/${name};
      workspace = crane.fileSetForCrates (lib.map crate [
        "howfastly"
        "howfastly-compute"
        "howfastly-gen"
        "howfastly-map"
        "howfastly-web"
      ]);
      compute = config.legacyPackages.crates.howfastly-compute;
    in
    {
      checks.default = crane.lib.cargoNextest (crane.commonArgs // {
        inherit (crane) cargoArtifacts;
        src = workspace;
        cargoNextestExtraArgs = "--workspace";

        __darwinAllowLocalNetworking = true;

        nativeBuildInputs = with pkgs; [
          cacert
          curl
          nushell
          viceroy
        ];

        env = {
          HOWFASTLY_E2E = "${../../crates/howfastly-compute/tests}/e2e.nu";
          HOWFASTLY_WASM = "${compute}/bin/howfastly-compute.wasm";
          HOWFASTLY_CONFIG = "${../../fastly.toml}";
        };
      });
    };
}
