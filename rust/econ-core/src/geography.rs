//! The map: where places lie, the country around them, the roads and rivers
//! to their market towns, and the weather that falls on it.
//!
//! Each village trades grain with its market town (the one it can reach most
//! cheaply). Carting grain costs a share of its price for loading, tolls,
//! dues and the merchant's margin, plus so much a km: by road, more across
//! hills and through woods; by boat on a river, about half as much.
//!
//! The weather is a smooth random field over the map, drawn afresh each
//! season: neighbouring villages share their droughts, good years and hard
//! winters, and the weather changes over a few hundred km.
//!
//! Regions are laid out by Python (`geography.region`) and read from the
//! exported config: their layout comes from numpy's draws.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use crate::config::Config;
use crate::rng::Rng;

pub const WEATHER_FEATURES: usize = 256; // waves summed to make a weather field
pub const ROUTE_NEIGHBOURS: usize = 6; // each town's routes to its nearest towns (trade beyond them never pays)
pub const KINDS: [&str; 2] = ["village", "town"];

/// The cheapest routes between neighbouring market towns, each pair once.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Routes {
    pub a: Vec<usize>,   // place number of one town
    pub b: Vec<usize>,   // and of the other
    pub cost: Vec<f64>,  // share of the grain price it costs to cart grain between them, handling included
    pub water: Vec<f64>, // share of that cost on rivers
}

impl Routes {
    pub fn len(&self) -> usize {
        self.a.len()
    }

    pub fn is_empty(&self) -> bool {
        self.a.is_empty()
    }
}

/// Where each place (village or market town) lies and what its country is like.
#[derive(Clone, Debug, PartialEq)]
pub struct Geography {
    pub x: Vec<f64>,         // km east
    pub y: Vec<f64>,         // km north
    pub terrain: Vec<usize>, // index into MapConfig.terrains
    /// Share of the grain price it costs to cart grain to or from its market
    /// town (0 for a town; infinite where no road reaches one).
    pub transport: Vec<f64>,
    /// Each place's market town, by place number (a town is its own; -1: none it can reach).
    pub market: Vec<i64>,
    pub pasture: Vec<f64>, // animals it can winter, against the usual
    pub woods: Vec<f64>,   // woods it can hold, against the usual
    pub forage: Vec<f64>,  // famine foods to be found, against the usual
    /// False: no map (every village the same distance from a town outside, with its own weather).
    pub on_map: bool,
    pub town: Vec<bool>,   // a market town
    pub border: Vec<bool>, // a town that trades with the world beyond the region
    pub routes: Routes,    // between neighbouring towns
    pub arable: Vec<f64>,  // farmland per person at the usual density, against the plain's
    /// Per place: share of a good's price to cart it to a border town (infinite: none).
    pub to_border: Vec<f64>,
}

impl Geography {
    /// Farmland as it counts at the usual density (hill fields count for more).
    pub fn usual_land(&self, land: &[f64]) -> Vec<f64> {
        land.iter().zip(&self.arable).map(|(l, a)| l / a).collect()
    }

    /// Market towns are places of their own (on a map).
    pub fn has_towns(&self) -> bool {
        self.on_map && self.town.iter().any(|&t| t)
    }

    /// Km as the crow flies from place `origin` to place `other`.
    #[inline]
    pub fn distance(&self, origin: usize, other: usize) -> f64 {
        (self.x[other] - self.x[origin]).hypot(self.y[other] - self.y[origin])
    }

    /// Places within `point.2` km of (point.0, point.1).
    pub fn near(&self, point: (f64, f64, f64)) -> Vec<bool> {
        let (x, y, km) = point;
        (0..self.x.len()).map(|i| (self.x[i] - x).hypot(self.y[i] - y) <= km).collect()
    }

    /// Each place's market town (a town is its own), or the place itself if it has none.
    pub fn town_of(&self) -> Vec<usize> {
        self.market.iter().enumerate().map(|(i, &m)| if m >= 0 { m as usize } else { i }).collect()
    }
}

/// The map of `config`'s places (with no map: every village the same
/// `TownConfig.transport` from a town outside).
pub fn build(config: &Config) -> Result<Geography, String> {
    let villages = &config.villages;
    let cfg = &config.map;
    let names: Vec<&str> = cfg.terrains.iter().map(|t| t.name.as_str()).collect();
    for v in villages {
        if !names.contains(&v.terrain.as_str()) {
            return Err(format!("{}: unknown terrain {:?}; choose from: {}", v.name, v.terrain, names.join(", ")));
        }
        if !KINDS.contains(&v.kind.as_str()) {
            return Err(format!("{}: unknown kind of place {:?}; choose from: {}", v.name, v.kind, KINDS.join(", ")));
        }
        if (v.kind == "town" || v.border) && !cfg.enabled {
            return Err(format!("{}: a market town (or a border) needs a map (MapConfig.enabled)", v.name));
        }
        if v.border && v.kind != "town" {
            return Err(format!("{}: only a town can trade across the border", v.name));
        }
    }
    let n = villages.len();
    let x: Vec<f64> = villages.iter().map(|v| v.x).collect();
    let y: Vec<f64> = villages.iter().map(|v| v.y).collect();
    let terrain: Vec<usize> = villages.iter().map(|v| cfg.terrain_index(&v.terrain).expect("checked")).collect();
    let town: Vec<bool> = villages.iter().map(|v| v.kind == "town").collect();
    let border: Vec<bool> = villages.iter().map(|v| v.border).collect();
    let mut routes = Routes::default();
    let mut to_border = vec![f64::INFINITY; n];
    let (transport, market) = if cfg.enabled {
        let (transport, market) = carriage(config)?;
        let (a, b, cost, water) = legs(config)?;
        let towns: Vec<usize> = (0..n).filter(|&i| town[i]).collect();
        routes = town_routes(n, &a, &b, &cost, &water, &towns, cfg.wholesale_handling, ROUTE_NEIGHBOURS);
        if border.iter().any(|&b| b) {
            let borders: Vec<usize> = (0..n).filter(|&i| border[i]).collect();
            let (best, _) = cheapest(n, &a, &b, &cost, &borders);
            for i in 0..n {
                to_border[i] = if border[i] {
                    cfg.handling
                } else if best[i].is_finite() {
                    cfg.handling + best[i]
                } else {
                    f64::INFINITY
                };
            }
        }
        (transport, market)
    } else {
        (vec![config.town.transport; n], vec![0; n])
    };
    let spec = |f: fn(&crate::config::TerrainSpec) -> f64| terrain.iter().map(|&t| f(&cfg.terrains[t])).collect();
    Ok(Geography {
        pasture: spec(|t| t.pasture),
        woods: spec(|t| t.woods),
        forage: spec(|t| t.forage),
        arable: spec(|t| t.arable),
        x,
        y,
        terrain,
        transport,
        market,
        on_map: cfg.enabled,
        town,
        border,
        routes,
        to_border,
    })
}

/// Every road and river leg as (from, to, cost as a share of the grain
/// price, true for a river leg); places are numbered as in `Config.villages`.
#[allow(clippy::type_complexity)]
pub fn legs(config: &Config) -> Result<(Vec<usize>, Vec<usize>, Vec<f64>, Vec<bool>), String> {
    let cfg = &config.map;
    let villages = &config.villages;
    let n = villages.len();
    let carriage: Vec<f64> =
        villages.iter().map(|v| cfg.terrains[cfg.terrain_index(&v.terrain).unwrap_or(0)].carriage).collect();
    let (mut a, mut b, mut cost, mut water) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (links, per_km, by_road) in [(&cfg.roads, cfg.road_per_km, true), (&cfg.rivers, cfg.river_per_km, false)] {
        for &(i, j) in links.iter() {
            if i >= n || j >= n {
                return Err(format!("a road or river joins place {}, but there are only {n}", i.max(j)));
            }
            let km = (villages[i].x - villages[j].x).hypot(villages[i].y - villages[j].y);
            let factor = if by_road { 0.5 * (carriage[i] + carriage[j]) } else { 1.0 };
            a.push(i);
            b.push(j);
            cost.push(per_km * km * factor);
            water.push(!by_road);
        }
    }
    Ok((a, b, cost, water))
}

/// A queue entry: the cheapest first, ties by place (as Python's heapq of tuples).
#[derive(PartialEq)]
struct Entry(f64, usize, usize);

impl Eq for Entry {}

impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Entry {
    fn cmp(&self, other: &Self) -> Ordering {
        // (reversed: BinaryHeap pops the largest)
        other
            .0
            .partial_cmp(&self.0)
            .unwrap_or(Ordering::Equal)
            .then_with(|| other.1.cmp(&self.1))
            .then_with(|| other.2.cmp(&self.2))
    }
}

/// The cheapest route from each town to its `nearest` towns along the links
/// (a search from each town that stops once it has reached them), with the
/// share of its cost on rivers; each pair once, the cheaper way.
#[allow(clippy::too_many_arguments)]
pub fn town_routes(
    n_places: usize,
    a: &[usize],
    b: &[usize],
    cost: &[f64],
    water: &[bool],
    towns: &[usize],
    handling: f64,
    nearest: usize,
) -> Routes {
    if towns.len() < 2 {
        return Routes::default();
    }
    let mut neighbours: Vec<Vec<(usize, f64, f64)>> = vec![Vec::new(); n_places];
    for k in 0..a.len() {
        let w = if water[k] { cost[k] } else { 0.0 };
        neighbours[a[k]].push((b[k], cost[k], w));
        neighbours[b[k]].push((a[k], cost[k], w));
    }
    let mut is_town = vec![false; n_places];
    for &t in towns {
        is_town[t] = true;
    }
    // (pair -> (cost, by water)), in the order pairs were first found
    let mut order: Vec<(usize, usize)> = Vec::new();
    let mut found: HashMap<(usize, usize), (f64, f64)> = HashMap::new();
    for &start in towns {
        let mut best: HashMap<usize, f64> = HashMap::from([(start, 0.0)]);
        let mut by_water: HashMap<usize, f64> = HashMap::from([(start, 0.0)]);
        let mut done = vec![false; n_places];
        let mut queue = BinaryHeap::from([Entry(0.0, start, 0)]);
        let mut reached = 0;
        while reached < nearest {
            let Some(Entry(so_far, place, _)) = queue.pop() else {
                break;
            };
            if done[place] {
                continue;
            }
            done[place] = true;
            if place != start && is_town[place] {
                reached += 1;
                let pair = (start.min(place), start.max(place));
                match found.get(&pair) {
                    Some(&(c, _)) if so_far >= c => {}
                    Some(_) => {
                        found.insert(pair, (so_far, by_water[&place]));
                    }
                    None => {
                        order.push(pair);
                        found.insert(pair, (so_far, by_water[&place]));
                    }
                }
            }
            for &(other, c, w) in &neighbours[place] {
                if so_far + c < *best.get(&other).unwrap_or(&f64::INFINITY) {
                    best.insert(other, so_far + c);
                    let wet = by_water[&place] + w;
                    by_water.insert(other, wet);
                    queue.push(Entry(so_far + c, other, 0));
                }
            }
        }
    }
    let mut routes = Routes::default();
    for pair in order {
        let (total, on_water) = found[&pair];
        routes.a.push(pair.0);
        routes.b.push(pair.1);
        routes.cost.push(handling + total);
        routes.water.push(if total > 0.0 { on_water / total } else { 0.0 });
    }
    routes
}

/// Cheapest cost from any of `sources` to every place along the links (both
/// ways), and which source it comes from (-1 if none reaches it).
pub fn cheapest(n_places: usize, a: &[usize], b: &[usize], cost: &[f64], sources: &[usize]) -> (Vec<f64>, Vec<i64>) {
    let mut neighbours: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n_places];
    for k in 0..a.len() {
        neighbours[a[k]].push((b[k], cost[k]));
        neighbours[b[k]].push((a[k], cost[k]));
    }
    let mut best = vec![f64::INFINITY; n_places];
    let mut origin = vec![-1i64; n_places];
    let mut queue = BinaryHeap::new();
    for (k, &s) in sources.iter().enumerate() {
        best[s] = 0.0;
        origin[s] = k as i64;
        queue.push(Entry(0.0, s, k));
    }
    while let Some(Entry(so_far, place, k)) = queue.pop() {
        if so_far > best[place] {
            continue;
        }
        for &(other, c) in &neighbours[place] {
            if so_far + c < best[other] {
                best[other] = so_far + c;
                origin[other] = k as i64;
                queue.push(Entry(so_far + c, other, k));
            }
        }
    }
    (best, origin)
}

/// (transport, market) per place: the share of the grain price it costs to
/// cart grain between it and its cheapest market town, and that town (-1,
/// and no trade, if no road or river reaches one). A town is its own
/// market, at no cost.
pub fn carriage(config: &Config) -> Result<(Vec<f64>, Vec<i64>), String> {
    let n = config.villages.len();
    let towns: Vec<usize> = (0..n).filter(|&i| config.villages[i].kind == "town").collect();
    let (a, b, cost, _) = legs(config)?;
    let (best, origin) = cheapest(n, &a, &b, &cost, &towns);
    let mut transport: Vec<f64> =
        best.iter().map(|&c| if c.is_finite() { config.map.handling + c } else { f64::INFINITY }).collect();
    for &t in &towns {
        transport[t] = 0.0;
    }
    let market = origin.iter().map(|&o| if o >= 0 { towns[o as usize] as i64 } else { -1 }).collect();
    Ok((transport, market))
}

/// A season's weather at each place, as a share between 0 (the worst season)
/// and 1 (the best): uniform at each place, and alike at places close
/// together (a smooth Gaussian random field whose correlation falls to a
/// half at about 1.2 x `scale_km`, from random Fourier features).
pub fn weather(geo: &Geography, scale_km: f64, rng: &mut Rng) -> Vec<f64> {
    let features = WEATHER_FEATURES;
    let waves: Vec<(f64, f64)> =
        (0..features).map(|_| (rng.normal(0.0, 1.0 / scale_km), rng.normal(0.0, 1.0 / scale_km))).collect();
    let phase: Vec<f64> = (0..features).map(|_| rng.uniform(0.0, 2.0 * std::f64::consts::PI)).collect();
    let scale = (2.0 / features as f64).sqrt();
    (0..geo.x.len())
        .map(|i| {
            let field: f64 =
                waves.iter().zip(&phase).map(|(&(wx, wy), &p)| (geo.x[i] * wx + geo.y[i] * wy + p).cos()).sum();
            normal_share(scale * field)
        })
        .collect()
}

/// The standard normal distribution function.
pub fn normal_share(z: f64) -> f64 {
    0.5 * libm::erfc(-z / std::f64::consts::SQRT_2)
}

/// One line about the map: villages, towns, people, terrain, carriage.
pub fn describe(config: &Config, geo: &Geography) -> String {
    let cfg = &config.map;
    let villages: Vec<_> = config.villages.iter().filter(|v| v.kind == "village").collect();
    let towns: Vec<_> = config.villages.iter().filter(|v| v.kind == "town").collect();
    let counts: Vec<String> = cfg
        .terrains
        .iter()
        .enumerate()
        .filter_map(|(t, spec)| {
            let k = (0..geo.terrain.len()).filter(|&i| !geo.town[i] && geo.terrain[i] == t).count();
            (k > 0).then(|| format!("{k} {}", spec.name))
        })
        .collect();
    let reachable: Vec<f64> = (0..geo.transport.len())
        .filter(|&i| !geo.town[i] && geo.transport[i].is_finite())
        .map(|i| geo.transport[i])
        .collect();
    let cost = if reachable.is_empty() {
        "none reach a town".to_string()
    } else {
        let low = reachable.iter().cloned().fold(f64::INFINITY, f64::min);
        let high = reachable.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        format!("{:.0}%-{:.0}%", 100.0 * low, 100.0 * high)
    };
    let village_people: i64 = villages.iter().map(|v| v.population).sum();
    let town_people: i64 = towns.iter().map(|v| v.population).sum();
    let border = towns.iter().filter(|v| v.border).count();
    let city = match towns.iter().max_by_key(|v| v.population) {
        Some(biggest) if towns.len() > 1 => {
            format!("; {} {}", biggest.name, thousands(biggest.population))
        }
        _ => String::new(),
    };
    format!(
        "Region of {} villages ({} people: {}) and {} market town{} ({} people{city}, {border} trading beyond the \
         region, {} routes between towns); carting grain to market costs {cost} of its price",
        villages.len(),
        thousands(village_people),
        counts.join(", "),
        towns.len(),
        if towns.len() != 1 { "s" } else { "" },
        thousands(town_people),
        geo.routes.len(),
    )
}

/// A whole number with commas between thousands.
pub fn thousands(value: i64) -> String {
    let digits = value.abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if value < 0 {
        format!("-{out}")
    } else {
        out
    }
}
