// cuts the natural earth layers into the route map data
// the base files keep the format of the nushell generator they replace, the web app embeds them
// the detail cells hold every layer of one cell in the format of howfastly_map::cells, the compute serves them

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Parser;
use howfastly_map::cells::{Key, LEVELS, Level, PLACES};
use howfastly_map::map::zoom;
use serde_json::Value;

#[derive(Debug, Parser)]
#[command(
    name = "howfastly-gen",
    about = concat!(
        "HowFastly ",
        env!("CARGO_PKG_VERSION"),
        "\nCut the Natural Earth layers into the route map's base files and detail cells"
    )
)]
struct Args {
    /// Directory holding the Natural Earth geojson files
    #[arg(long)]
    sources: PathBuf,

    /// Directory for the base files the web app embeds
    #[arg(long)]
    base: Option<PathBuf>,

    /// Directory for the detail cells the compute serves
    #[arg(long)]
    cells: Option<PathBuf>,
}

type Points = Vec<(f64, f64)>;

// a natural earth layer and the features of it that stay
struct Source {
    file: &'static str,
    ring: bool,
    keep: fn(&Value) -> bool,
}

fn all(_: &Value) -> bool {
    true
}

// lake centerlines cut through lakes that are drawn, rivers alone stay
fn river(p: &Value) -> bool {
    p["featurecla"] == "River"
}

// regional, statistical and indicator lines stay out, the last run through the sea
fn state(p: &Value) -> bool {
    p["FEATURECLA"] == "Admin-1 boundary"
}

// a populated place with its natural earth prominence
struct Town {
    zoom: f64,
    lon: f64,
    lat: f64,
    name: String,
}

fn read(dir: &Path, file: &str) -> Result<Value> {
    let path = dir.join(format!("{file}.geojson"));
    let reader =
        BufReader::new(File::open(&path).with_context(|| format!("open {}", path.display()))?);
    serde_json::from_reader(reader).with_context(|| format!("parse {}", path.display()))
}

// v rounded to a grid of steps per degree, as the nushell generator did with its precision
fn round(v: f64, steps: f64) -> f64 {
    (v * steps).round() / steps
}

// consecutive repeats collapse into one
fn dedup(points: Points) -> Points {
    let mut out: Points = Vec::with_capacity(points.len());
    for p in points {
        if out.last() != Some(&p) {
            out.push(p);
        }
    }
    out
}

// every ring or line of a layer, rounded and freed of repeats, in file order
// a geometry this does not know or a layer that keep empties is a source change and stops the run
// a feature without geometry carries nothing to draw and natural earth has a few
fn polylines(geojson: &Value, source: &Source, steps: f64) -> Result<Vec<Points>> {
    let mut out = Vec::new();
    for feature in geojson["features"].as_array().into_iter().flatten() {
        if !(source.keep)(&feature["properties"]) || feature["geometry"].is_null() {
            continue;
        }
        let geometry = &feature["geometry"];
        let coordinates = &geometry["coordinates"];
        let parts: Vec<&Value> = match geometry["type"].as_str() {
            Some("Polygon" | "MultiLineString") => {
                coordinates.as_array().into_iter().flatten().collect()
            }
            Some("MultiPolygon") => coordinates
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|polygon| polygon.as_array().into_iter().flatten())
                .collect(),
            Some("LineString") => vec![coordinates],
            other => bail!("{} holds a {other:?} geometry", source.file),
        };
        for part in parts {
            let points = part
                .as_array()
                .into_iter()
                .flatten()
                .map(|p| {
                    match (
                        p.get(0).and_then(Value::as_f64),
                        p.get(1).and_then(Value::as_f64),
                    ) {
                        (Some(lon), Some(lat)) => Ok((round(lon, steps), round(lat, steps))),
                        _ => bail!("{} holds a point that is not two numbers", source.file),
                    }
                })
                .collect::<Result<Points>>()?;
            out.push(dedup(points));
        }
    }
    if out.is_empty() {
        bail!(
            "{} yields nothing, its format or its properties changed",
            source.file
        );
    }
    Ok(out)
}

fn towns(geojson: &Value) -> Result<Vec<Town>> {
    let mut out = Vec::new();
    for feature in geojson["features"].as_array().into_iter().flatten() {
        let p = &feature["properties"];
        let at = feature["geometry"]["coordinates"].as_array();
        let (Some(zoom), Some(name), Some([lon, lat, ..])) = (
            p["min_zoom"].as_f64(),
            p["name"].as_str(),
            at.map(Vec::as_slice),
        ) else {
            bail!("a place without zoom, name or coordinates");
        };
        let (Some(lon), Some(lat)) = (lon.as_f64(), lat.as_f64()) else {
            bail!("a place with coordinates that are not numbers");
        };
        out.push(Town {
            zoom: round(zoom, 10.0),
            lon: round(lon, 100.0),
            lat: round(lat, 100.0),
            name: name.to_string(),
        });
    }
    // stable, file order breaks ties like the nushell sort did
    out.sort_by(|a, b| a.zoom.total_cmp(&b.zoom));
    Ok(out)
}

fn write(path: PathBuf, lines: &[String]) -> Result<()> {
    let text = if lines.is_empty() {
        String::new()
    } else {
        lines.join("\n") + "\n"
    };
    fs::write(&path, text).with_context(|| format!("write {}", path.display()))
}

// the 110m base at a tenth of a degree, drawn for the world
// its places reach the zoom of the widest frame the base still serves alone
fn base(dir: &Path, sources: &Path) -> Result<()> {
    fs::create_dir_all(dir)?;
    let layers = [
        ("land.txt", "ne_110m_land", true),
        ("lakes.txt", "ne_110m_lakes", true),
        ("borders.txt", "ne_110m_admin_0_boundary_lines_land", false),
    ];
    for (out, file, ring) in layers {
        let source = Source {
            file,
            ring,
            keep: all,
        };
        let lines: Vec<String> = polylines(&read(sources, file)?, &source, 10.0)?
            .into_iter()
            .filter(|p| p.len() >= 3)
            .map(|p| {
                p.iter()
                    .map(|(lon, lat)| format!("{lon:.1},{lat:.1}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();
        write(dir.join(out), &lines)?;
    }
    let widest = LEVELS.iter().map(|l| l.max_w).fold(0.0, f64::max);
    let cut = zoom(widest);
    let lines: Vec<String> = towns(&read(sources, "ne_10m_populated_places_simple")?)?
        .iter()
        .filter(|t| t.zoom <= cut)
        .map(|t| format!("{:.1}\t{:.2}\t{:.2}\t{}", t.zoom, t.lon, t.lat, t.name))
        .collect();
    write(dir.join("places.txt"), &lines)
}

// the layers of a level with the grid of its rounding, in drawing order
struct Plan {
    level: &'static Level,
    steps: f64,
    layers: Vec<(&'static str, Source)>,
}

fn plans() -> Result<Vec<Plan>> {
    let source = |file, ring, keep| Source { file, ring, keep };
    LEVELS
        .iter()
        .map(|level| {
            let (steps, layers) = match level.name {
                "10m" => (
                    100.0,
                    vec![
                        ("land", source("ne_10m_land", true, all)),
                        ("urban", source("ne_10m_urban_areas", true, all)),
                        ("lakes", source("ne_10m_lakes", true, all)),
                        (
                            "rivers",
                            source("ne_10m_rivers_lake_centerlines", false, river),
                        ),
                        (
                            "admin1",
                            source("ne_10m_admin_1_states_provinces_lines", false, state),
                        ),
                        (
                            "borders",
                            source("ne_10m_admin_0_boundary_lines_land", false, all),
                        ),
                    ],
                ),
                "50m" => (
                    20.0,
                    vec![
                        ("land", source("ne_50m_land", true, all)),
                        ("lakes", source("ne_50m_lakes", true, all)),
                        (
                            "admin1",
                            source("ne_50m_admin_1_states_provinces_lines", false, state),
                        ),
                        (
                            "borders",
                            source("ne_50m_admin_0_boundary_lines_land", false, all),
                        ),
                    ],
                ),
                other => bail!("no layers for level {other}"),
            };
            Ok(Plan {
                level,
                steps,
                layers,
            })
        })
        .collect()
}

// a degree in hundredths, the unit of a cell file
fn hundredths(v: f64) -> i64 {
    (v * 100.0).round() as i64
}

fn line(points: &[(f64, f64)]) -> String {
    points
        .iter()
        .map(|(lon, lat)| format!("{},{}", hundredths(*lon), hundredths(*lat)))
        .collect::<Vec<_>>()
        .join(" ")
}

// the cell of a point without wrapping, a ring never crosses the antimeridian in natural earth
// the east and north edges fold into the last cell
fn cell(level: &Level, (lon, lat): (f64, f64)) -> Key {
    let ix = (((lon + 180.0) / level.deg).floor() as i64).clamp(0, level.columns() - 1);
    let iy = (((lat + 90.0) / level.deg).floor() as i64).clamp(0, level.rows() - 1);
    (ix, iy)
}

// a ring against a rectangle, sutherland hodgman over its four edges
// a ring around the whole rectangle comes back as the rectangle
fn clip(ring: &[(f64, f64)], (x0, y0, x1, y1): (f64, f64, f64, f64)) -> Points {
    type Test = (
        fn(f64, (f64, f64)) -> bool,
        fn(f64, (f64, f64), (f64, f64)) -> (f64, f64),
    );
    let at_x =
        |x: f64, p: (f64, f64), q: (f64, f64)| (x, p.1 + (x - p.0) / (q.0 - p.0) * (q.1 - p.1));
    let at_y =
        |y: f64, p: (f64, f64), q: (f64, f64)| (p.0 + (y - p.1) / (q.1 - p.1) * (q.0 - p.0), y);
    let edges: [(f64, Test); 4] = [
        (x0, (|x, p| p.0 >= x, at_x)),
        (x1, (|x, p| p.0 <= x, at_x)),
        (y0, (|y, p| p.1 >= y, at_y)),
        (y1, (|y, p| p.1 <= y, at_y)),
    ];
    let mut poly = ring.to_vec();
    for (edge, (inside, cut)) in edges {
        if poly.is_empty() {
            break;
        }
        let mut out = Vec::with_capacity(poly.len() + 4);
        for i in 0..poly.len() {
            let (cur, prev) = (poly[i], poly[(i + poly.len() - 1) % poly.len()]);
            match (inside(edge, cur), inside(edge, prev)) {
                (true, true) => out.push(cur),
                (true, false) => {
                    out.push(cut(edge, prev, cur));
                    out.push(cur);
                }
                (false, true) => out.push(cut(edge, prev, cur)),
                (false, false) => {}
            }
        }
        poly = out;
    }
    let mut poly = dedup(poly);
    if poly.len() > 1 && poly.first() == poly.last() {
        poly.pop();
    }
    poly
}

// abutting cells overlap by this much in degrees, so no seam shows between their fills
const OVERLAP: f64 = 0.01;

// a ring lands in every cell its box touches, clipped to each
fn rings(level: &Level, ring: &[(f64, f64)], out: &mut BTreeMap<Key, Vec<String>>) {
    let lon = ring.iter().map(|p| p.0);
    let lat = ring.iter().map(|p| p.1);
    let (lon0, lon1) = (
        lon.clone().fold(f64::MAX, f64::min),
        lon.fold(f64::MIN, f64::max),
    );
    let (lat0, lat1) = (
        lat.clone().fold(f64::MAX, f64::min),
        lat.fold(f64::MIN, f64::max),
    );
    let (c0, r0) = cell(level, (lon0, lat0));
    let (c1, r1) = cell(level, (lon1, lat1));
    for ix in c0..=c1 {
        for iy in r0..=r1 {
            let (x, y) = level.corner((ix, iy));
            let rect = (
                x - OVERLAP,
                y - OVERLAP,
                x + level.deg + OVERLAP,
                y + level.deg + OVERLAP,
            );
            let clipped = clip(ring, rect);
            if clipped.len() >= 3 {
                out.entry((ix, iy)).or_default().push(line(&clipped));
            }
        }
    }
}

// a line splits where it leaves a cell, the crossing segment is drawn from both sides
fn lines(level: &Level, points: &[(f64, f64)], out: &mut BTreeMap<Key, Vec<String>>) {
    let mut run: Points = Vec::new();
    let mut current = None;
    let flush = |key: Key, run: &Points, out: &mut BTreeMap<Key, Vec<String>>| {
        if run.len() >= 2 {
            out.entry(key).or_default().push(line(run));
        }
    };
    for &p in points {
        let key = cell(level, p);
        match current {
            Some(c) if c != key => {
                run.push(p);
                flush(c, &run, out);
                run = vec![run[run.len() - 2], p];
            }
            _ => run.push(p),
        }
        current = Some(key);
    }
    if let Some(c) = current {
        flush(c, &run, out);
    }
}

// the cells of one level, a file for every cell so a miss is an error and open sea is empty
fn level(plan: &Plan, sources: &Path, dir: &Path) -> Result<()> {
    fs::create_dir_all(dir.join(plan.level.name))?;
    let mut files: BTreeMap<Key, Vec<String>> = BTreeMap::new();
    for ix in 0..plan.level.columns() {
        for iy in 0..plan.level.rows() {
            files.insert((ix, iy), Vec::new());
        }
    }
    let section =
        |name: &str, cut: BTreeMap<Key, Vec<String>>, files: &mut BTreeMap<Key, Vec<String>>| {
            for (key, mut lines) in cut {
                let file = files.entry(key).or_default();
                file.push(format!("={name}"));
                file.append(&mut lines);
            }
        };
    for (name, source) in &plan.layers {
        let mut cut = BTreeMap::new();
        for polyline in polylines(&read(sources, source.file)?, source, plan.steps)? {
            if source.ring {
                rings(plan.level, &polyline, &mut cut);
            } else {
                lines(plan.level, &polyline, &mut cut);
            }
        }
        section(name, cut, &mut files);
    }
    let mut cut: BTreeMap<Key, Vec<String>> = BTreeMap::new();
    for t in towns(&read(sources, "ne_10m_populated_places_simple")?)? {
        cut.entry(cell(plan.level, (t.lon, t.lat)))
            .or_default()
            .push(format!(
                "{:.1}\t{}\t{}\t{}",
                t.zoom,
                hundredths(t.lon),
                hundredths(t.lat),
                t.name
            ));
    }
    section(PLACES, cut, &mut files);
    let (mut filled, mut bytes) = (0usize, 0usize);
    for (key, lines) in &files {
        write(dir.join(plan.level.file(*key)), lines)?;
        if !lines.is_empty() {
            filled += 1;
            bytes += lines.iter().map(|l| l.len() + 1).sum::<usize>();
        }
    }
    eprintln!(
        "{}: {filled} of {} cells hold data, {bytes} bytes",
        plan.level.name,
        files.len()
    );
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.base.is_none() && args.cells.is_none() {
        bail!("nothing to write, pass --base or --cells");
    }
    if let Some(dir) = &args.base {
        base(dir, &args.sources)?;
    }
    if let Some(dir) = &args.cells {
        for plan in plans()? {
            level(&plan, &args.sources, dir)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ten() -> &'static Level {
        &LEVELS[0]
    }

    fn source(keep: fn(&Value) -> bool) -> Source {
        Source {
            file: "x",
            ring: false,
            keep,
        }
    }

    #[test]
    fn rounding_and_text() {
        assert_eq!(round(-80.04, 10.0), -80.0);
        assert_eq!(round(0.33, 20.0), 0.35);
        assert_eq!(hundredths(-87.64), -8764);
        assert_eq!(hundredths(0.35), 35);
        assert_eq!(
            line(&[(-87.64, 41.86), (-87.0, 42.0)]),
            "-8764,4186 -8700,4200"
        );
        assert_eq!(
            dedup(vec![(1.0, 1.0), (1.0, 1.0), (2.0, 2.0), (1.0, 1.0)]),
            vec![(1.0, 1.0), (2.0, 2.0), (1.0, 1.0)]
        );
    }

    // every layer name a plan writes is one the client reads
    #[test]
    fn plan_layers_are_known() {
        let plans = plans().unwrap();
        assert_eq!(plans.len(), LEVELS.len());
        for plan in &plans {
            for (name, _) in &plan.layers {
                assert!(howfastly_map::cells::LAYERS.contains(name), "{name}");
            }
        }
    }

    #[test]
    fn clip_cases() {
        let cell = (0.0, 0.0, 10.0, 10.0);
        // a square straddling the east edge keeps its western half
        let half = clip(
            &[(5.0, 2.0), (15.0, 2.0), (15.0, 8.0), (5.0, 8.0), (5.0, 2.0)],
            cell,
        );
        assert_eq!(half, vec![(5.0, 2.0), (10.0, 2.0), (10.0, 8.0), (5.0, 8.0)]);
        // a ring inside stays as it is without its closing repeat
        assert_eq!(
            clip(&[(1.0, 1.0), (2.0, 1.0), (2.0, 2.0), (1.0, 1.0)], cell),
            vec![(1.0, 1.0), (2.0, 1.0), (2.0, 2.0)]
        );
        // a ring outside vanishes, a ring around the cell becomes the cell
        assert!(clip(&[(20.0, 20.0), (30.0, 20.0), (30.0, 30.0)], cell).is_empty());
        let around = clip(
            &[(-5.0, -5.0), (15.0, -5.0), (15.0, 15.0), (-5.0, 15.0)],
            cell,
        );
        assert_eq!(around.len(), 4);
        assert!(
            around
                .iter()
                .all(|&(x, y)| (x == 0.0 || x == 10.0) && (y == 0.0 || y == 10.0))
        );
    }

    #[test]
    fn rings_and_lines_land_in_cells() {
        let mut out = BTreeMap::new();
        // a ring over the 80th meridian west and the 40th parallel touches four cells
        rings(
            ten(),
            &[(-85.0, 35.0), (-75.0, 35.0), (-75.0, 45.0), (-85.0, 45.0)],
            &mut out,
        );
        assert_eq!(
            out.keys().copied().collect::<Vec<_>>(),
            vec![(9, 12), (9, 13), (10, 12), (10, 13)]
        );
        // the quarter in the south west cell is a square whichever corner the clip starts from
        // it reaches a hundredth past the cell edges so the fills of neighbors overlap
        let mut corners: Vec<&str> = out[&(9, 12)][0].split(' ').collect();
        corners.sort_unstable();
        assert_eq!(
            corners,
            vec!["-7999,3500", "-7999,4001", "-8500,3500", "-8500,4001"]
        );
        let mut out = BTreeMap::new();
        lines(
            ten(),
            &[(-85.0, 41.0), (-81.0, 41.0), (-79.0, 41.0), (-78.0, 42.0)],
            &mut out,
        );
        assert_eq!(
            out[&(9, 13)],
            vec!["-8500,4100 -8100,4100 -7900,4100".to_string()]
        );
        assert_eq!(
            out[&(10, 13)],
            vec!["-8100,4100 -7900,4100 -7800,4200".to_string()]
        );
        let mut out = BTreeMap::new();
        lines(ten(), &[(-85.0, 41.0)], &mut out);
        assert!(out.is_empty());
        assert_eq!(cell(ten(), (180.0, 90.0)), (35, 17));
        assert_eq!(cell(ten(), (-180.0, -90.0)), (0, 0));
    }

    #[test]
    fn geojson_shapes() {
        let geojson = json!({"features": [
            {"properties": {"featurecla": "River"}, "geometry": {"type": "LineString", "coordinates": [[1.04, 2.0], [1.04, 2.01], [3.0, 4.0]]}},
            {"properties": {"featurecla": "Lake Centerline"}, "geometry": {"type": "LineString", "coordinates": [[0.0, 0.0], [1.0, 1.0]]}},
            {"properties": {}, "geometry": {"type": "MultiLineString", "coordinates": [[[0.0, 0.0], [1.0, 1.0]], [[2.0, 2.0], [3.0, 3.0]]]}},
            {"properties": {}, "geometry": {"type": "MultiPolygon", "coordinates": [[[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]]}},
        ]});
        let rivers = polylines(&geojson, &source(river), 10.0).unwrap();
        assert_eq!(rivers, vec![vec![(1.0, 2.0), (3.0, 4.0)]]);
        let everything = polylines(&geojson, &source(all), 10.0).unwrap();
        assert_eq!(everything.len(), 5);
        assert_eq!(everything[4].len(), 4);
        // a layer that keep empties, an unknown geometry and a bad point stop the run
        assert!(polylines(&geojson, &source(state), 10.0).is_err());
        let odd = json!({"features": [{"properties": {}, "geometry": {"type": "Point", "coordinates": [0.0, 0.0]}}]});
        assert!(polylines(&odd, &source(all), 10.0).is_err());
        let bad = json!({"features": [{"properties": {}, "geometry": {"type": "LineString", "coordinates": [[0.0], [1.0, 1.0]]}}]});
        assert!(polylines(&bad, &source(all), 10.0).is_err());
        // a feature without geometry is skipped, natural earth has a few
        let empty = json!({"features": [{"properties": {}, "geometry": null}, {"properties": {}, "geometry": {"type": "LineString", "coordinates": [[0.0, 0.0], [1.0, 1.0]]}}]});
        assert_eq!(polylines(&empty, &source(all), 10.0).unwrap().len(), 1);
        assert!(state(&json!({"FEATURECLA": "Admin-1 boundary"})));
        assert!(!state(
            &json!({"FEATURECLA": "Admin-1 statistical meta bounds"})
        ));
        let places = json!({"features": [
            {"properties": {"min_zoom": 6.1, "name": "Grenoble"}, "geometry": {"coordinates": [5.724, 45.183]}},
            {"properties": {"min_zoom": 1.7, "name": "Paris"}, "geometry": {"coordinates": [2.351, 48.857]}},
        ]});
        let t = towns(&places).unwrap();
        assert_eq!(t[0].name, "Paris");
        assert_eq!((t[1].lon, t[1].lat, t[1].zoom), (5.72, 45.18, 6.1));
        assert!(towns(&json!({"features": [{"properties": {"name": "x"}, "geometry": {"coordinates": [1, 2]}}]})).is_err());
    }
}
