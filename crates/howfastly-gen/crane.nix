{ crane, ... }:

{
  # the generator shares the cell grid with the map crate
  src = crane.fileSetForCrates [ ../howfastly-map ../howfastly-gen ];
}
