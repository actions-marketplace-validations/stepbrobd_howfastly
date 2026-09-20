use crate::map::{self, Place, View, WORLD};

// a detail level of the route map, natural earth cut into square cells of deg degrees
// a frame narrower than max_w map units takes the level, the first that holds it wins
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Level {
    pub name: &'static str,
    pub deg: f64,
    pub max_w: f64,
}

// 10m in ten degree cells for frames under thirty map units, 50m in thirty degree cells under 120
// wider frames keep the embedded 110m base, which is drawn for the world
pub const LEVELS: [Level; 2] = [
    Level {
        name: "10m",
        deg: 10.0,
        max_w: 30.0,
    },
    Level {
        name: "50m",
        deg: 30.0,
        max_w: 120.0,
    },
];

pub fn level(w: f64) -> Option<&'static Level> {
    LEVELS.iter().find(|l| w <= l.max_w)
}

// a cell by column from the antimeridian and row from the south pole
pub type Key = (i64, i64);

impl Level {
    pub fn columns(&self) -> i64 {
        (360.0 / self.deg).round() as i64
    }

    pub fn rows(&self) -> i64 {
        (180.0 / self.deg).round() as i64
    }

    // the cell holding a point, longitude wraps and the poles fold into the last row
    pub fn cell(&self, lon: f64, lat: f64) -> Key {
        let ix = ((lon + 180.0) / self.deg).floor() as i64;
        let iy = ((lat + 90.0) / self.deg).floor() as i64;
        (ix.rem_euclid(self.columns()), iy.clamp(0, self.rows() - 1))
    }

    // the south west corner of a cell in degrees
    pub fn corner(&self, (ix, iy): Key) -> (f64, f64) {
        (ix as f64 * self.deg - 180.0, iy as f64 * self.deg - 90.0)
    }

    // the file of a cell under the level directory, named by its corner
    pub fn file(&self, key: Key) -> String {
        let (lon, lat) = self.corner(key);
        format!("{}/{lon}_{lat}.txt", self.name)
    }

    pub fn url(&self, key: Key) -> String {
        format!("/cells/{}", self.file(key))
    }

    // every cell under a viewport, west to east within south to north
    // the columns are taken before wrapping so a frame over the antimeridian keeps both sides
    pub fn keys(&self, view: &View) -> Vec<Key> {
        let column = |x: f64| (x / WORLD * 360.0 / self.deg).floor() as i64;
        let (c0, c1) = (column(view.x), column(view.x + view.w));
        let columns = self.columns();
        let span = (c1 - c0 + 1).min(columns);
        let row = |y: f64| self.cell(0.0, map::unproject(0.0, y.clamp(0.0, WORLD)).1).1;
        let (r0, r1) = (row(view.y + view.h), row(view.y));
        let mut out = Vec::new();
        for iy in r0..=r1 {
            for c in c0..c0 + span {
                out.push((c.rem_euclid(columns), iy));
            }
        }
        out
    }
}

pub type Lines = Vec<Vec<(f64, f64)>>;

// the layers of one cell in degrees, rings for the areas and open lines for the rest
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cell {
    pub land: Lines,
    pub urban: Lines,
    pub lakes: Lines,
    pub rivers: Lines,
    pub admin1: Lines,
    pub borders: Lines,
    pub places: Vec<Place>,
}

// the layers a cell file may carry, in drawing order, and the section of its towns
pub const LAYERS: [&str; 6] = ["land", "urban", "lakes", "rivers", "admin1", "borders"];
pub const PLACES: &str = "places";

impl Cell {
    pub fn layer_mut(&mut self, name: &str) -> Option<&mut Lines> {
        Some(match name {
            "land" => &mut self.land,
            "urban" => &mut self.urban,
            "lakes" => &mut self.lakes,
            "rivers" => &mut self.rivers,
            "admin1" => &mut self.admin1,
            "borders" => &mut self.borders,
            _ => return None,
        })
    }
}

// a cell file, a line of = and a name opens a section
// a polyline is lon,lat pairs in hundredths of a degree, a place is zoom, lon, lat and name tab separated
// an unknown section is skipped so an older client still reads what it knows, None on a line that does not parse
pub fn parse(text: &str) -> Option<Cell> {
    let mut cell = Cell::default();
    let mut section = None;
    for line in text.lines().filter(|l| !l.is_empty()) {
        if let Some(name) = line.strip_prefix('=') {
            section = Some(name);
            continue;
        }
        match section? {
            PLACES => cell.places.push(place(line)?),
            name => {
                if let Some(layer) = cell.layer_mut(name) {
                    layer.push(polyline(line)?);
                }
            }
        }
    }
    Some(cell)
}

fn hundredths(s: &str) -> Option<f64> {
    s.parse::<i64>().ok().map(|v| v as f64 / 100.0)
}

fn polyline(line: &str) -> Option<Vec<(f64, f64)>> {
    line.split_whitespace()
        .map(|pair| {
            let (lon, lat) = pair.split_once(',')?;
            Some((hundredths(lon)?, hundredths(lat)?))
        })
        .collect()
}

fn place(line: &str) -> Option<Place> {
    let mut f = line.splitn(4, '\t');
    let zoom = f.next()?.parse().ok()?;
    let lon = hundredths(f.next()?)?;
    let lat = hundredths(f.next()?)?;
    Some(Place {
        name: f.next()?.to_string(),
        at: map::project(lon, lat),
        zoom,
    })
}

// the svg paths of a set of cells, drawn on the copy of the world nearest anchor
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Paths {
    pub land: String,
    pub urban: String,
    pub lakes: String,
    pub rivers: String,
    pub admin1: String,
    pub borders: String,
}

pub fn paths(cells: &[Cell], anchor: f64) -> Paths {
    let draw = |pick: fn(&Cell) -> &Lines, close: bool| {
        cells
            .iter()
            .flat_map(|c| pick(c).iter())
            .map(|pl| shape(pl, anchor, close))
            .collect::<String>()
    };
    Paths {
        land: draw(|c| &c.land, true),
        urban: draw(|c| &c.urban, true),
        lakes: draw(|c| &c.lakes, true),
        rivers: draw(|c| &c.rivers, false),
        admin1: draw(|c| &c.admin1, false),
        borders: draw(|c| &c.borders, false),
    }
}

// one polyline projected whole onto the copy nearest anchor, a ring closes
fn shape(points: &[(f64, f64)], anchor: f64, close: bool) -> String {
    let projected: Vec<(f64, f64)> = points
        .iter()
        .map(|&(lon, lat)| map::project(lon, lat))
        .collect();
    let Some(&(x0, _)) = projected.first() else {
        return String::new();
    };
    let shift = map::nearest(anchor, x0) - x0;
    let shifted: Vec<(f64, f64)> = projected.into_iter().map(|(x, y)| (x + shift, y)).collect();
    let mut d = map::path(&shifted);
    if close && !d.is_empty() {
        d.push('Z');
    }
    d
}

// the towns of a set of cells in order of prominence, as labels wants them
pub fn places(cells: &[Cell]) -> Vec<Place> {
    let mut out: Vec<Place> = cells
        .iter()
        .flat_map(|c| c.places.iter().cloned())
        .collect();
    out.sort_by(|a, b| a.zoom.total_cmp(&b.zoom));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{fit, project};
    use proptest::prelude::*;

    fn ten() -> &'static Level {
        &LEVELS[0]
    }

    #[test]
    fn levels_by_width() {
        assert_eq!(level(12.0).map(|l| l.name), Some("10m"));
        assert_eq!(level(30.0).map(|l| l.name), Some("10m"));
        assert_eq!(level(30.1).map(|l| l.name), Some("50m"));
        assert_eq!(level(120.0).map(|l| l.name), Some("50m"));
        assert_eq!(level(121.0), None);
        assert!(LEVELS.windows(2).all(|w| w[0].max_w < w[1].max_w));
        assert_eq!(ten().columns(), 36);
        assert_eq!(ten().rows(), 18);
        assert_eq!(LEVELS[1].columns(), 12);
        assert_eq!(LEVELS[1].rows(), 6);
    }

    #[test]
    fn cells_and_files() {
        assert_eq!(ten().cell(-87.7, 41.9), (9, 13));
        assert_eq!(ten().file((9, 13)), "10m/-90_40.txt");
        assert_eq!(ten().url((0, 0)), "/cells/10m/-180_-90.txt");
        assert_eq!(ten().cell(180.0, 90.0), (0, 17));
        assert_eq!(ten().cell(-180.0, -90.0), (0, 0));
        assert_eq!(ten().cell(185.0, 0.0), (0, 9));
        assert_eq!(
            LEVELS[1].file(LEVELS[1].cell(145.3, -37.8)),
            "50m/120_-60.txt"
        );
        assert_eq!(ten().corner((35, 17)), (170.0, 80.0));
    }

    #[test]
    fn keys_under_frames() {
        // chicago at the floor sits in one cell
        let v = fit(
            project(-87.8, 42.0),
            project(-87.641, 41.863),
            2.0,
            0.35,
            12.0,
        );
        assert_eq!(ten().keys(&v), vec![(9, 13)]);
        // mc keesport to ashburn straddles the 80th meridian and the 40th parallel
        let v = fit(
            project(-79.8, 40.3),
            project(-77.455811, 38.944533),
            2.0,
            0.35,
            12.0,
        );
        assert_eq!(ten().keys(&v), vec![(9, 12), (10, 12), (9, 13), (10, 13)]);
        // a frame over the antimeridian keeps both sides in one order
        let v = View {
            x: 990.0,
            y: 400.0,
            w: 20.0,
            h: 10.0,
        };
        assert_eq!(ten().keys(&v), vec![(35, 12), (0, 12)]);
        // the whole world names every column once
        let v = View {
            x: -500.0,
            y: 0.0,
            w: 2000.0,
            h: 1000.0,
        };
        let keys = LEVELS[1].keys(&v);
        assert_eq!(keys.len(), 12 * 6);
    }

    proptest! {
        #[test]
        fn keys_cover_the_frame(
            x in -1000.0f64..2000.0,
            y in 0.0f64..900.0,
            w in 1.0f64..400.0,
            h in 1.0f64..100.0,
            fx in 0.0f64..=1.0,
            fy in 0.0f64..=1.0,
        ) {
            let v = View { x, y, w, h: h.min(WORLD - y) };
            for level in &LEVELS {
                let keys = level.keys(&v);
                let (lon, lat) = map::unproject(v.x + v.w * fx, v.y + v.h * fy);
                prop_assert!(keys.contains(&level.cell(lon, lat)));
                let mut sorted = keys.clone();
                sorted.sort();
                sorted.dedup();
                prop_assert_eq!(sorted.len(), keys.len());
            }
        }
    }

    const CELL: &str = "=land\n-9000,4000 -8000,4000 -8000,5000\n=lakes\n-8764,4186 -8700,4200 -8700,4300\n=rivers\n-8500,4100 -8600,4200\n=towns\n1 2 3\n=places\n3\t-8764\t4186\tChicago\n6.1\t-8600\t4300\tWaukegan\n";

    #[test]
    fn parse_exact() {
        let cell = parse(CELL).unwrap();
        assert_eq!(
            cell.land,
            vec![vec![(-90.0, 40.0), (-80.0, 40.0), (-80.0, 50.0)]]
        );
        assert_eq!(cell.lakes[0][0], (-87.64, 41.86));
        assert_eq!(cell.rivers.len(), 1);
        assert!(cell.urban.is_empty() && cell.admin1.is_empty() && cell.borders.is_empty());
        assert_eq!(cell.places.len(), 2);
        assert_eq!(cell.places[0].name, "Chicago");
        assert_eq!(cell.places[1].zoom, 6.1);
        assert_eq!(cell.places[1].at, project(-86.0, 43.0));
        assert_eq!(parse(""), Some(Cell::default()));
        assert_eq!(parse("\n\n"), Some(Cell::default()));
        // a line before any section, a pair without a comma and a decimal all refuse
        assert_eq!(parse("-9000,4000\n"), None);
        assert_eq!(parse("=land\n-9000 4000\n"), None);
        assert_eq!(parse("=land\n-90.0,40.0\n"), None);
        assert_eq!(parse("=places\n3\t-8764\t4186\n"), None);
        assert_eq!(parse("=places\nx\t-8764\t4186\tChicago\n"), None);
    }

    #[test]
    fn paths_and_places() {
        let cell = parse(CELL).unwrap();
        let p = paths(&[cell.clone(), cell.clone()], 250.0);
        assert!(p.land.starts_with("M250.000,"));
        assert!(p.land.ends_with('Z'));
        assert_eq!(p.land.matches('Z').count(), 2);
        assert_eq!(p.land.matches('M').count(), 2);
        assert!(!p.rivers.contains('Z'));
        assert!(p.urban.is_empty());
        // the copy east of the antimeridian is drawn when the frame sits there
        let east = paths(std::slice::from_ref(&cell), 1250.0);
        assert!(east.land.starts_with("M1250.000,"));
        let towns = places(&[cell.clone(), cell]);
        assert_eq!(towns.len(), 4);
        assert!(towns.windows(2).all(|w| w[0].zoom <= w[1].zoom));
        assert_eq!(paths(&[], 0.0), Paths::default());
    }

    #[test]
    fn layers_named() {
        let mut cell = Cell::default();
        for name in LAYERS {
            assert!(cell.layer_mut(name).is_some());
        }
        assert!(cell.layer_mut(PLACES).is_none());
        assert!(cell.layer_mut("roads").is_none());
    }
}
