{ inputs
, stdenv
, fetchurl
, linkFarm
, runCommand
}:

let
  gen = inputs.self.legacyPackages.${stdenv.hostPlatform.system}.crates.howfastly-gen;

  # natural earth vector master as of 2022-06-02, public domain
  rev = "ca96624a56bd078437bca8184e78163e5039ad19";

  layer = name: hash: {
    name = "${name}.geojson";
    path = fetchurl {
      url = "https://raw.githubusercontent.com/nvkelso/natural-earth-vector/${rev}/geojson/${name}.geojson";
      inherit hash;
    };
  };

  sources = linkFarm "natural-earth" [
    (layer "ne_110m_land" "sha256-ngcp7iU8p9elxK6TlfsZAiZMU3fFLiJNE92FAQ4oNdk=")
    (layer "ne_110m_lakes" "sha256-6wLsyGyCAE/Mv5eQWL+ru9bC0Hlox4RNOOsckVLS/8k=")
    (layer "ne_110m_admin_0_boundary_lines_land" "sha256-1CR5/XlVLMpO7H+F/NynF6eQ0p/wa+dnbxrwVoxtP3w=")
    (layer "ne_50m_land" "sha256-6HSyelHRRkUr42DK+zzFDIYAEHSmfVNBE+ZTRoL5gms=")
    (layer "ne_50m_lakes" "sha256-01C3WXiyb+g5t5fCxSmy+49H+zmDwD9JZONtXfk3ilI=")
    (layer "ne_50m_admin_0_boundary_lines_land" "sha256-L6rE9rNDhvPSG24BjPFR8kHwDlyTbUTdF9fZv7FH+kg=")
    (layer "ne_50m_admin_1_states_provinces_lines" "sha256-csypPIUNQSYopdpLxev+IbpNN26zRhG95rYj7nPw/c8=")
    (layer "ne_10m_land" "sha256-GskHlkCLxq1pEdaUSEhdPE2/IZA3AIA2igmXbhyfdBY=")
    (layer "ne_10m_lakes" "sha256-LQNvU97exXgAHFwwwpWe59TuvBMGkA+kNnxJkp7I8tk=")
    (layer "ne_10m_rivers_lake_centerlines" "sha256-u4VKkA7L07QI30bV4W4+D5dLpVmT+di1wm6FUnPAkFo=")
    (layer "ne_10m_urban_areas" "sha256-UTb/2BapsowPKV5nl7TAPqpCG+gNeLkPVC20qTLdRJc=")
    (layer "ne_10m_admin_0_boundary_lines_land" "sha256-dNnBYinAlf3mWUOpkZ4zdoLwRLzrzLEgdk847fO3D0o=")
    (layer "ne_10m_admin_1_states_provinces_lines" "sha256-Gh8wzKr0zJxL3jQmbwuMu5VdOkzyVLdWkSJV8ux8dbY=")
    (layer "ne_10m_populated_places_simple" "sha256-/T+oZ6Mgy9XFtrtbxVCv7sKTn7LO9ojlCABygqVaxC8=")
  ];
in
# the detail cells the compute embeds, a build product like the web dist
  # the base files the web embeds are cut from the same sources and committed, see crates/howfastly-map/assets/gen.nu
runCommand "howfastly-cells" { nativeBuildInputs = [ gen ]; } ''
  howfastly-gen --sources ${sources} --cells "$out"
''
