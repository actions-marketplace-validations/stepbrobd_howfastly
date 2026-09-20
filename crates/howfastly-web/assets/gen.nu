#!/usr/bin/env nu

# renders the favicon from the svg source next to this script
# the png is committed, so a build needs no renderer
# run: nix shell nixpkgs#resvg -c nu gen.nu

def main [] {
  cd $env.FILE_PWD
  resvg --width 192 icon.svg favicon.png
}
