#!/usr/bin/env nu
# refreshes the base files next to this script from natural earth, public domain
# the sources are the commit pkgs/howfastly/cells.nix pins, the cut itself is crates/howfastly-gen
# the detail cells the compute embeds come from the same generator through nix, see pkgs/howfastly/cells.nix

const rev = "ca96624a56bd078437bca8184e78163e5039ad19"
const base = [
  ne_110m_land
  ne_110m_lakes
  ne_110m_admin_0_boundary_lines_land
  ne_10m_populated_places_simple
]
const detail = [
  ne_50m_land
  ne_50m_lakes
  ne_50m_admin_0_boundary_lines_land
  ne_50m_admin_1_states_provinces_lines
  ne_10m_land
  ne_10m_lakes
  ne_10m_rivers_lake_centerlines
  ne_10m_urban_areas
  ne_10m_admin_0_boundary_lines_land
  ne_10m_admin_1_states_provinces_lines
]

# --cells also cuts the detail cells into that directory, as the nix build does
def main [--cells: path] {
  let sources = mktemp -d -t natural-earth-XXXXXX
  let layers = if $cells == null { $base } else { $base ++ $detail }
  for layer in $layers {
    http get --raw $"https://raw.githubusercontent.com/nvkelso/natural-earth-vector/($rev)/geojson/($layer).geojson"
    | save --force ($sources | path join $"($layer).geojson")
  }
  let out = if $cells == null { [] } else { [--cells $cells] }
  # the download goes whether the cut succeeds or not
  let failure = try {
    cargo run --release --package howfastly-gen -- --sources $sources --base $env.FILE_PWD ...$out
    null
  } catch { |err| $err }
  rm -r $sources
  if $failure != null {
    error make { msg: $failure.msg }
  }
}
