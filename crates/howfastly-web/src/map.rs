use futures::future::join_all;
use gloo_timers::future::TimeoutFuture;
use howfastly::types::{Coordinates, MetaResponse};
use howfastly_map::cells::{self, Level, Paths};
use howfastly_map::map::{self, Place, Side, View};
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::engine;

// the frame is twice as wide as tall, taller on phones where the css breakpoint matches
const ASPECT: f64 = 2.0;
const NARROW_ASPECT: f64 = 4.0 / 3.0;
const NARROW_PX: f64 = 640.0;
// the main column caps at 72rem from 1024 px up and at 65ch below, the paddings and border come off
const WIDE_COLUMN_PX: f64 = 1152.0;
const COLUMN_PX: f64 = 585.0;
const INSET_PX: f64 = 66.0;
// the label text starts two spacing units off its dot, then advances per glyph
const OFFSET_PX: f64 = 8.0;
const CHAR_PX: f64 = 6.6;
// fraction of the route extent kept clear on each side
const PAD: f64 = 0.35;
// narrowest viewport in map units so a short route keeps its surroundings
const MIN_W: f64 = 24.0;
const FLY_MS: f64 = 1500.0;
const FRAME_MS: u32 = 16;
const ARC_STEPS: usize = 64;
const MAX_LABELS: usize = 24;
// a phone frame holds fewer labels and needs more clearance between rows
const NARROW_LABELS: usize = 12;
const NARROW_GAP: (f64, f64) = (0.05, 0.07);

// aspect, clearance, label budget and text metrics, settled once at mount
fn frame() -> (f64, (f64, f64), usize, (f64, f64)) {
    let inner = web_sys::window()
        .and_then(|w| w.inner_width().ok())
        .and_then(|v| v.as_f64())
        .unwrap_or(WIDE_COLUMN_PX);
    let column = if inner >= 1024.0 {
        WIDE_COLUMN_PX
    } else {
        COLUMN_PX
    };
    let px = inner.min(column) - INSET_PX;
    let text = (OFFSET_PX / px, CHAR_PX / px);
    if inner < NARROW_PX {
        (NARROW_ASPECT, NARROW_GAP, NARROW_LABELS, text)
    } else {
        (ASPECT, map::GAP, MAX_LABELS, text)
    }
}

// map points of the visitor and the pop joined by the great circle
// the arc is traced from the visitor so the pop lands on its nearest copy
#[derive(Clone, PartialEq)]
struct Route {
    client: Option<(f64, f64)>,
    pop: Option<(f64, f64)>,
    arc: Vec<(f64, f64)>,
}

impl Route {
    fn points(&self) -> Vec<(f64, f64)> {
        if self.arc.is_empty() {
            self.client.into_iter().chain(self.pop).collect()
        } else {
            self.arc.clone()
        }
    }

    // the viewport the flight ends in
    fn target(&self, aspect: f64) -> Option<View> {
        let (lo, hi) = map::bounds(&self.points())?;
        Some(map::fit(lo, hi, aspect, PAD, MIN_W))
    }
}

fn route(m: &MetaResponse) -> Route {
    let lonlat = |c: Coordinates| (c.longitude, c.latitude);
    match (m.coordinates.map(lonlat), m.pop.coordinates.map(lonlat)) {
        (Some(c), Some(p)) => {
            let arc = map::trace(&map::arc(c, p, ARC_STEPS));
            Route {
                client: arc.first().copied(),
                pop: arc.last().copied(),
                arc,
            }
        }
        (c, p) => Route {
            client: c.map(|(lon, lat)| map::project(lon, lat)),
            pop: p.map(|(lon, lat)| map::project(lon, lat)),
            arc: Vec::new(),
        },
    }
}

// a town label laid out for the target viewport
#[derive(Clone, PartialEq)]
struct Town {
    name: String,
    at: (f64, f64),
}

// the detail under a landed frame, fetched for one flight
#[derive(Clone, PartialEq)]
struct Detail {
    flight: u32,
    paths: Paths,
    places: Vec<Place>,
}

// glide the viewport to its target, a newer flight takes over mid-air
// settled marks the landing, the labels laid out for the target wait for it
async fn fly(
    view: RwSignal<View>,
    flight: RwSignal<u32>,
    settled: RwSignal<bool>,
    id: u32,
    to: View,
) {
    let from = view.get_untracked();
    let start = engine::now_ms();
    loop {
        if flight.get_untracked() != id {
            return;
        }
        let t = ((engine::now_ms() - start) / FLY_MS).min(1.0);
        view.set(from.toward(&to, map::ease(t)));
        if t >= 1.0 {
            settled.set(true);
            return;
        }
        TimeoutFuture::new(FRAME_MS).await;
    }
}

// the cells under the target frame
// any failure leaves the base map and its towns in place and says so in the console
async fn load(
    detail: RwSignal<Option<Detail>>,
    flight: RwSignal<u32>,
    id: u32,
    level: &'static Level,
    to: View,
) {
    let urls: Vec<String> = level.keys(&to).into_iter().map(|k| level.url(k)).collect();
    let fetched = join_all(urls.iter().map(|url| engine::text(url))).await;
    let mut loaded = Vec::with_capacity(fetched.len());
    for (url, text) in urls.iter().zip(fetched) {
        let cell = match text {
            Ok(text) => cells::parse(&text),
            Err(e) => {
                web_sys::console::warn_1(&format!("{url} failed, {}", engine::describe(e)).into());
                return;
            }
        };
        let Some(cell) = cell else {
            web_sys::console::warn_1(&format!("{url} does not parse").into());
            return;
        };
        loaded.push(cell);
    }
    if flight.get_untracked() != id {
        return;
    }
    detail.set(Some(Detail {
        flight: id,
        paths: cells::paths(&loaded, to.x + to.w / 2.0),
        places: cells::places(&loaded),
    }));
}

// active gates the flight, nothing moves before the visitor confirms the first run
// quiet says the connection carries no latency probes, the cells wait for it
// children overlay the frame, the run controls sit in its corner
#[component]
pub fn Map(
    meta: Signal<Option<MetaResponse>>,
    active: Signal<bool>,
    quiet: Signal<bool>,
    children: Children,
) -> impl IntoView {
    let land = map::land(map::LAND).expect("land outline");
    let borders = map::borders(map::BORDERS).expect("borders");
    let places = StoredValue::new(map::places(map::PLACES).expect("places"));
    let (aspect, gap, limit, text) = frame();
    let view = RwSignal::new(map::world(aspect));
    let route = Memo::new(move |_| {
        active
            .get()
            .then(|| meta.with(|m| m.as_ref().map(route)))
            .flatten()
    });
    // the pop is labeled by city like the towns around it, the code stands in when unresolved
    let names = Memo::new(move |_| {
        meta.with(|m| {
            m.as_ref().map(|m| {
                let you = if m.city.is_empty() { "You" } else { &m.city };
                let pop = if m.pop.name.is_empty() {
                    &m.pop.code
                } else {
                    &m.pop.name
                };
                (you.to_string(), pop.to_string())
            })
        })
    });
    // the viewport the flight ends in
    let target = Memo::new(move |_| route.get().and_then(|r| r.target(aspect)));
    let flight = RwSignal::new(0u32);
    let settled = RwSignal::new(false);
    let detail: RwSignal<Option<Detail>> = RwSignal::new(None);
    // the detail is on screen once landed and fetched for this flight
    let shown = Memo::new(move |_| {
        settled.get() && detail.with(|d| d.as_ref().is_some_and(|d| d.flight == flight.get()))
    });

    // the strips the two route labels take in the target frame and the side of their dot
    let anchors = Memo::new(move |_| {
        let (r, target, (you, pop)) = (route.get()?, target.get()?, names.get()?);
        let strip = |p: (f64, f64), name: &str| {
            let (fx, fy) = target.frac(p);
            (fx, fy, map::width(name, text))
        };
        let (a, b) = (
            r.client.map(|p| strip(p, &you)),
            r.pop.map(|p| strip(p, &pop)),
        );
        let sides = match (a, b) {
            (Some(a), Some(b)) => map::sides(a, b, gap),
            _ => (Side::Right, Side::Right),
        };
        Some((a.map(|a| (a, sides.0)), b.map(|b| (b, sides.1))))
    });

    // towns are laid out once for the viewport the flight ends in
    // the route labels are placed first and towns fill the space left, from the cells once they are in
    let towns = Memo::new(move |_| {
        let Some(target) = target.get() else {
            return Vec::new();
        };
        let taken: Vec<(f64, f64, f64)> = anchors
            .get()
            .into_iter()
            .flat_map(|(a, b)| [a, b])
            .flatten()
            .map(|(s, side)| map::strip(s, side))
            .collect();
        let lay = |p: &[Place]| {
            map::labels(p, &target, &taken, gap, text, limit)
                .into_iter()
                .map(|l| Town {
                    name: l.place.name.clone(),
                    at: l.at,
                })
                .collect::<Vec<Town>>()
        };
        detail.with(|d| match d.as_ref().filter(|d| d.flight == flight.get()) {
            Some(d) => lay(&d.places),
            None => places.with_value(|p| lay(p)),
        })
    });

    Effect::new(move |_| {
        let Some(to) = target.get() else {
            return;
        };
        let id = flight.get_untracked() + 1;
        flight.set(id);
        settled.set(false);
        detail.set(None);
        spawn_local(fly(view, flight, settled, id, to));
    });

    // one fetch per flight, once the connection is quiet, the transfers do not mind a few kilobytes
    let requested = RwSignal::new(0u32);
    Effect::new(move |_| {
        let id = flight.get();
        if id == 0 || !quiet.get() || requested.get_untracked() == id {
            return;
        }
        let Some((to, level)) = target
            .get_untracked()
            .and_then(|to| cells::level(to.w).map(|level| (to, level)))
        else {
            return;
        };
        requested.set(id);
        spawn_local(load(detail, flight, id, level, to));
    });

    view! {
        <div class="relative aspect-[4/3] w-full overflow-hidden rounded border border-nord-3 bg-nord-0 sm:aspect-[2/1]">
            <svg
                class="h-full w-full"
                viewBox=move || view.get().view_box()
                preserveAspectRatio="xMidYMid slice"
            >
                <defs>
                    <g id="tile">
                        <path
                            d=land
                            class="fill-nord-2 stroke-nord-3"
                            fill-rule="evenodd"
                            stroke-width="1"
                            vector-effect="non-scaling-stroke"
                        />
                        <path
                            d=borders
                            fill="none"
                            class="stroke-nord-3"
                            stroke-width="1"
                            vector-effect="non-scaling-stroke"
                        />
                    </g>
                </defs>
                // copies on both sides so a route over the antimeridian keeps its land
                // the base gives way to the cells once they are drawn
                <g class="transition-opacity duration-300" class=("opacity-0", move || shown.get())>
                    <use href="#tile" x=format!("{}", -map::WORLD)/>
                    <use href="#tile"/>
                    <use href="#tile" x=format!("{}", map::WORLD)/>
                </g>
                // cells overlap their neighbors by a hair and the areas fill nonzero, so the overlap
                // stays filled, the areas stroke under their fill, so the edges where cells abut
                // vanish under the neighbor and a coast keeps the outer half of a doubled stroke
                // the urban fill is opaque so the overlap of two cells does not darken
                <g class="transition-opacity duration-300" class=("opacity-0", move || !shown.get())>
                    {move || detail.get().map(|d| view! {
                        <path
                            d=d.paths.land
                            class="fill-nord-2 stroke-nord-3"
                            paint-order="stroke"
                            stroke-width="2"
                            vector-effect="non-scaling-stroke"
                        />
                        <path
                            d=d.paths.urban
                            class="fill-[color-mix(in_srgb,var(--color-nord-2),var(--color-nord-3)_55%)]"
                        />
                        <path
                            d=d.paths.lakes
                            class="fill-nord-0 stroke-nord-3"
                            paint-order="stroke"
                            stroke-width="2"
                            vector-effect="non-scaling-stroke"
                        />
                        <path
                            d=d.paths.rivers
                            fill="none"
                            class="stroke-nord-0"
                            stroke-width="1"
                            vector-effect="non-scaling-stroke"
                        />
                        <path
                            d=d.paths.admin1
                            fill="none"
                            class="stroke-nord-3"
                            stroke-width="1"
                            stroke-dasharray="3 3"
                            vector-effect="non-scaling-stroke"
                        />
                        <path
                            d=d.paths.borders
                            fill="none"
                            class="stroke-nord-3"
                            stroke-width="1"
                            vector-effect="non-scaling-stroke"
                        />
                    })}
                </g>
                <g class="transition-opacity duration-300" class=("opacity-0", move || !settled.get())>
                    <For
                        each=move || towns.get()
                        key=|t| (t.at.0.to_bits(), t.at.1.to_bits())
                        children=move |t| view! {
                            <path
                                d=format!("{}h0", map::path(&[t.at]))
                                class="stroke-nord-4"
                                stroke-width="4"
                                stroke-linecap="round"
                                vector-effect="non-scaling-stroke"
                            />
                        }
                    />
                </g>
                {move || route.get().map(|r| view! {
                    <path
                        d=map::path(&r.arc)
                        fill="none"
                        class="stroke-nord-4"
                        stroke-opacity="0.7"
                        stroke-dasharray="4 4"
                        vector-effect="non-scaling-stroke"
                    />
                    {r.pop.map(|p| view! { <Dot at=p class="stroke-nord-8"/> })}
                    {r.client.map(|p| view! { <Dot at=p class="stroke-nord-13"/> })}
                })}
            </svg>
            <div
                class="pointer-events-none absolute inset-0 transition-opacity duration-300"
                class=("opacity-0", move || !settled.get())
            >
                <For
                    each=move || towns.get()
                    key=|t| (t.at.0.to_bits(), t.at.1.to_bits())
                    children=move |t| view! {
                        <Label at=t.at view=view text=t.name side=Side::Right class="text-xs text-nord-4"/>
                    }
                />
                {move || route.get().zip(names.get()).zip(anchors.get()).map(|((r, (you, pop)), (a, b))| view! {
                    {r.client.zip(a).map(|(p, (_, side))| view! { <Label at=p view=view text=you side=side class="text-nord-6"/> })}
                    {r.pop.zip(b).map(|(p, (_, side))| view! { <Label at=p view=view text=pop side=side class="text-nord-6"/> })}
                })}
            </div>
            {children()}
            <details class="absolute right-1 bottom-1">
                <summary class="flex h-5 w-5 cursor-pointer list-none items-center justify-center rounded-full bg-nord-1 font-serif text-xs text-nord-4 [&::-webkit-details-marker]:hidden">
                    "i"
                </summary>
                <small class="absolute right-6 bottom-0 rounded bg-nord-1 px-2 py-1 whitespace-nowrap text-nord-4">
                    "Made with "
                    <a href="https://www.naturalearthdata.com" target="_blank" rel="noopener">"Natural Earth"</a>
                </small>
            </details>
        </div>
    }
}

// a zero length stroke with round caps is a dot of fixed screen size
// the dark halo separates it from the land
#[component]
fn Dot(at: (f64, f64), class: &'static str) -> impl IntoView {
    let d = format!("{}h0", map::path(&[at]));
    view! {
        <path
            d=d.clone()
            class="stroke-nord-0"
            stroke-width="14"
            stroke-linecap="round"
            vector-effect="non-scaling-stroke"
        />
        <path
            d=d
            class=class
            stroke-width="10"
            stroke-linecap="round"
            vector-effect="non-scaling-stroke"
        />
    }
}

// html text pinned to a map point, it follows the viewport without a rebuild
// two spacing units off its dot on the given side, on the left the text ends there
#[component]
fn Label(
    at: (f64, f64),
    view: RwSignal<View>,
    text: String,
    side: Side,
    class: &'static str,
) -> impl IntoView {
    let shift = match side {
        Side::Right => "translate-x-2",
        Side::Left => "translate-x-[calc(-100%_-_0.5rem)]",
    };
    view! {
        <small
            class=format!("absolute -translate-y-1/2 {shift} whitespace-nowrap [text-shadow:0_0_4px_var(--color-nord-0)] {class}")
            style=move || {
                let (fx, fy) = view.get().frac(at);
                format!("left:{:.2}%;top:{:.2}%", fx * 100.0, fy * 100.0)
            }
        >
            {text}
        </small>
    }
}
