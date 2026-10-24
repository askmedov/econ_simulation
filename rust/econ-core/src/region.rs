//! Laying out a region from a few settings, as Python's `geography.region`:
//! villages on a rough grid, hills on one side, a river across it with a
//! market town on its bank, woods, and roads to each village's nearest
//! neighbours and towns. The steps are Python's; the draws are the Rust
//! engine's own, so the same settings give the same kind of region, not
//! the same map (for Python's very map, export it with
//! `scripts/export_config.py --region`). The default settings are
//! Python's, exported to `rust/configs/region_settings.json`.

use std::collections::BTreeSet;

use serde::Deserialize;

use crate::config::{Config, VillageConfig};
use crate::rng::Rng;

pub const REGION_SETTINGS_JSON: &str = include_str!("../../configs/region_settings.json");

/// A few settings for `region` (Python's `RegionSettings`).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegionSettings {
    pub villages: usize,
    pub mean_size: f64,       // people in a village of the plain, on average
    pub size_spread: f64,     // spread of village sizes (sd of their log)
    pub smallest: i64,        //
    pub spacing_km: f64,      // between neighbouring villages
    pub towns: Option<usize>, // market towns; None: one for every 25 villages
    pub town_share: f64,      // share of the region's people who live in its towns
    pub smallest_town: i64,   //
    pub city: f64,            // with several towns, the river town is this many times the size
    pub upland: f64,          // share of villages in the hills (to the west)
    pub forest: f64,          // share of the rest in the woods
    pub river: bool,          // a river across the region, west to east
    pub coast: bool,          // a coast along the east side
    pub seed: i64,
}

#[derive(Deserialize)]
struct Exported {
    settings: RegionSettings,
    size_by_terrain: std::collections::HashMap<String, f64>,
}

impl Default for RegionSettings {
    fn default() -> RegionSettings {
        let exported: Exported = serde_json::from_str(REGION_SETTINGS_JSON).expect("region_settings.json");
        exported.settings
    }
}

/// Villages are bigger where the land is richest, smaller in hills and woods.
fn size_by_terrain(terrain: &str) -> f64 {
    let exported: Exported = serde_json::from_str(REGION_SETTINGS_JSON).expect("region_settings.json");
    *exported.size_by_terrain.get(terrain).unwrap_or(&1.0)
}

/// numpy's `quantile` (linear between the closest ranks).
fn quantile(values: &[f64], q: f64) -> f64 {
    crate::scenarios::percentile(values, 100.0 * q)
}

/// A smooth random surface over the places (standard normal at each).
fn smooth(x: &[f64], y: &[f64], scale: f64, rng: &mut Rng) -> Vec<f64> {
    let waves: Vec<(f64, f64)> =
        (0..64).map(|_| (rng.normal(0.0, 1.0 / scale), rng.normal(0.0, 1.0 / scale))).collect();
    let phase: Vec<f64> = (0..64).map(|_| rng.uniform(0.0, 2.0 * std::f64::consts::PI)).collect();
    (0..x.len())
        .map(|i| {
            (2.0f64 / 64.0).sqrt()
                * waves.iter().zip(&phase).map(|(&(wx, wy), &p)| (x[i] * wx + y[i] * wy + p).cos()).sum::<f64>()
        })
        .collect()
}

/// `m` centres of the weighted points (k-means, started far apart).
fn centres(px: &[f64], py: &[f64], weights: &[f64], m: usize) -> Vec<(f64, f64)> {
    let n = px.len();
    let (mx, my) = (px.iter().sum::<f64>() / n as f64, py.iter().sum::<f64>() / n as f64);
    let dist = |i: usize, (cx, cy): (f64, f64)| (px[i] - cx).hypot(py[i] - cy);
    let argmin = |f: &dyn Fn(usize) -> f64| (1..n).fold(0, |best, i| if f(i) < f(best) { i } else { best });
    let mut chosen = vec![argmin(&|i| dist(i, (mx, my)))];
    for _ in 1..m {
        let nearest = |i: usize| chosen.iter().map(|&c| dist(i, (px[c], py[c]))).fold(f64::INFINITY, f64::min);
        let far = (1..n).fold(0, |best, i| if nearest(i) > nearest(best) { i } else { best });
        chosen.push(far);
    }
    let mut centres: Vec<(f64, f64)> = chosen.iter().map(|&c| (px[c], py[c])).collect();
    for _ in 0..20 {
        let nearest: Vec<usize> = (0..n)
            .map(|i| (1..m).fold(0, |best, c| if dist(i, centres[c]) < dist(i, centres[best]) { c } else { best }))
            .collect();
        for (c, centre) in centres.iter_mut().enumerate() {
            let mine: Vec<usize> = (0..n).filter(|&i| nearest[i] == c).collect();
            if !mine.is_empty() {
                let w: f64 = mine.iter().map(|&i| weights[i]).sum();
                *centre = (
                    mine.iter().map(|&i| px[i] * weights[i]).sum::<f64>() / w,
                    mine.iter().map(|&i| py[i] * weights[i]).sum::<f64>() / w,
                );
            }
        }
    }
    centres
}

/// Add the shortest roads needed so that every place can be reached.
fn join_up(x: &[f64], y: &[f64], roads: &mut BTreeSet<(usize, usize)>) {
    let n = x.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for &(a, b) in roads.iter() {
        let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
        parent[ra] = rb;
    }
    loop {
        let groups: Vec<usize> = (0..n).map(|i| root(&mut parent, i)).collect();
        let inside: Vec<usize> = (0..n).filter(|&i| groups[i] == groups[0]).collect();
        let outside: Vec<usize> = (0..n).filter(|&i| groups[i] != groups[0]).collect();
        if outside.is_empty() {
            return;
        }
        let mut best = (f64::INFINITY, 0, 0);
        for &i in &inside {
            for &j in &outside {
                let d = (x[i] - x[j]).hypot(y[i] - y[j]);
                if d < best.0 {
                    best = (d, i, j);
                }
            }
        }
        let (a, b) = (best.1, best.2);
        roads.insert((a.min(b), a.max(b)));
        let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
        parent[ra] = rb;
    }
}

/// Indices of `values` from smallest to largest (ties in order).
fn argsort(values: &[f64]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..values.len()).collect();
    order.sort_by(|&a, &b| values[a].partial_cmp(&values[b]).unwrap_or(std::cmp::Ordering::Equal));
    order
}

fn round_to(x: f64, decimals: i32) -> f64 {
    let scale = 10f64.powi(decimals);
    (x * scale).round_ties_even() / scale
}

fn title(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map_or(String::new(), |c| c.to_uppercase().collect::<String>() + chars.as_str())
}

/// A region laid out from `settings`, on top of `base` (its other settings).
pub fn region(settings: &RegionSettings, base: &Config) -> Result<Config, String> {
    let s = settings;
    if s.villages < 1 {
        return Err("a region needs at least one village".into());
    }
    let mut rng = Rng::named(s.seed, "region");
    let n = s.villages;
    let k = (n as f64).sqrt().ceil() as usize;
    let side = s.spacing_km * k as f64;
    let mut all_cells: Vec<usize> = (0..k * k).collect();
    rng.shuffle(&mut all_cells);
    let cells = &all_cells[..n];
    let x: Vec<f64> = cells.iter().map(|&c| ((c % k) as f64 + 0.2 + 0.6 * rng.random()) * s.spacing_km).collect();
    let y: Vec<f64> = cells.iter().map(|&c| ((c / k) as f64 + 0.2 + 0.6 * rng.random()) * s.spacing_km).collect();

    // Hills to the west, a river from west to east, woods here and there,
    // perhaps a coast to the east.
    let mut terrain = vec!["plain"; n];
    let height: Vec<f64> =
        smooth(&x, &y, side / 2.0, &mut rng).iter().zip(&x).map(|(h, xi)| h + 1.5 * (1.0 - xi / side)).collect();
    let woods = smooth(&x, &y, side / 3.0, &mut rng);
    let bend = rng.uniform(0.0, 2.0 * std::f64::consts::PI);
    let wiggle = rng.uniform(0.8, 1.6);
    let river_y = |at: f64| side / 2.0 + side / 6.0 * (2.0 * std::f64::consts::PI * wiggle * at / side + bend).sin();
    let on_river: Vec<bool> = (0..n).map(|i| s.river && (y[i] - river_y(x[i])).abs() < 0.6 * s.spacing_km).collect();
    let hill_line = quantile(&height, 1.0 - s.upland);
    let hills: Vec<bool> = (0..n).map(|i| s.upland > 0.0 && height[i] >= hill_line).collect();
    for i in 0..n {
        if hills[i] {
            terrain[i] = "upland";
        }
    }
    let rest: Vec<usize> = (0..n).filter(|&i| !hills[i] && !on_river[i]).collect();
    if s.forest > 0.0 && !rest.is_empty() {
        let line = quantile(&rest.iter().map(|&i| woods[i]).collect::<Vec<_>>(), 1.0 - s.forest);
        for &i in &rest {
            if woods[i] >= line {
                terrain[i] = "forest";
            }
        }
    }
    for i in 0..n {
        if on_river[i] {
            terrain[i] = "valley";
        }
        if s.coast && x[i] > side - s.spacing_km {
            terrain[i] = "coast";
        }
    }
    let spread = s.size_spread;
    let sizes: Vec<i64> = (0..n)
        .map(|i| {
            let size = s.mean_size
                * size_by_terrain(terrain[i])
                * (spread * rng.normal(0.0, 1.0) - spread * spread / 2.0).exp();
            (size.round_ties_even() as i64).max(s.smallest)
        })
        .collect();

    // Market towns where the people are, each the size of its share of the
    // countryside around it; the first on the river, which carries goods to
    // and from the world beyond (or, without a river, the one nearest the
    // east), and on a coast, the one nearest the sea is a port.
    let m = s.towns.unwrap_or_else(|| ((n as f64 / 25.0).round_ties_even() as usize).max(1)).min(n);
    let weights: Vec<f64> = sizes.iter().map(|&v| v as f64).collect();
    let mut towns = centres(&x, &y, &weights, m);
    let m = towns.len();
    let mut border = vec![false; m];
    let first = if s.river {
        let first = (1..m).fold(0, |best, t| {
            if (towns[t].1 - river_y(towns[t].0)).abs() < (towns[best].1 - river_y(towns[best].0)).abs() {
                t
            } else {
                best
            }
        });
        towns[first].1 = river_y(towns[first].0);
        first
    } else {
        (1..m).fold(0, |best, t| if towns[t].0 > towns[best].0 { t } else { best })
    };
    border[first] = true;
    if s.coast {
        let east = (1..m).fold(0, |best, t| if towns[t].0 > towns[best].0 { t } else { best });
        border[east] = true;
    }
    let (tx, ty): (Vec<f64>, Vec<f64>) = towns.iter().cloned().unzip();
    let mut around = vec![0.0; m];
    for i in 0..n {
        let nearest = (1..m).fold(0, |best, t| {
            if (x[i] - tx[t]).hypot(y[i] - ty[t]) < (x[i] - tx[best]).hypot(y[i] - ty[best]) {
                t
            } else {
                best
            }
        });
        around[nearest] += sizes[i] as f64;
    }
    let mut town_sizes: Vec<i64> = around
        .iter()
        .map(|a| ((s.town_share / (1.0 - s.town_share) * a).round_ties_even() as i64).max(s.smallest_town))
        .collect();
    let is_city = m > 1 && s.city > 1.0;
    if is_city {
        town_sizes[first] = (town_sizes[first] as f64 * s.city).round_ties_even() as i64;
    }

    // Roads to each village's nearest neighbours and from each town to its
    // nearest villages, joined up so every place can be reached.
    let px: Vec<f64> = x.iter().chain(&tx).cloned().collect();
    let py: Vec<f64> = y.iter().chain(&ty).cloned().collect();
    let mut roads = BTreeSet::new();
    for i in 0..n {
        let mut d: Vec<f64> = (0..n).map(|j| (x[j] - x[i]).hypot(y[j] - y[i])).collect();
        d[i] = f64::INFINITY;
        for &j in argsort(&d).iter().take(3.min(n - 1)) {
            roads.insert((i.min(j), i.max(j)));
        }
    }
    for t in 0..m {
        let d: Vec<f64> = (0..n).map(|j| (x[j] - tx[t]).hypot(y[j] - ty[t])).collect();
        for &j in argsort(&d).iter().take(4.min(n)) {
            roads.insert((j, n + t));
        }
    }
    join_up(&px, &py, &mut roads);
    // The river links the villages on its banks, and its town, in order downstream.
    let on_bank: Vec<bool> = (0..m).map(|t| s.river && (ty[t] - river_y(tx[t])).abs() < 0.6 * s.spacing_km).collect();
    let mut rivers = Vec::new();
    if s.river {
        let mut banks: Vec<usize> = (0..n).filter(|&i| on_river[i]).collect();
        banks.extend((0..m).filter(|&t| on_bank[t]).map(|t| n + t));
        let along: Vec<f64> = banks.iter().map(|&b| px[b]).collect();
        let order: Vec<usize> = argsort(&along).iter().map(|&i| banks[i]).collect();
        rivers = order.windows(2).map(|w| (w[0], w[1])).collect();
    }

    let village = &base.villages[0];
    let per_person = village.land / village.population as f64;
    let arable = |t: &str| base.map.terrains.iter().find(|s| s.name == t).map_or(1.0, |s| s.arable);
    let mut places: Vec<VillageConfig> = (0..n)
        .map(|i| VillageConfig {
            name: format!("{} {}", title(terrain[i]), i + 1),
            population: sizes[i],
            land: round_to(per_person * sizes[i] as f64 * arable(terrain[i]), 1),
            initial_food_months: village.initial_food_months,
            x: round_to(x[i], 2),
            y: round_to(y[i], 2),
            terrain: terrain[i].to_string(),
            kind: "village".into(),
            border: false,
        })
        .collect();
    places.extend((0..m).map(|t| VillageConfig {
        name: if t == first && is_city { "City".into() } else { format!("Town {}", t + 1) },
        population: town_sizes[t],
        land: 0.0,
        initial_food_months: 0.0,
        x: round_to(tx[t], 2),
        y: round_to(ty[t], 2),
        terrain: if on_bank[t] { "valley".into() } else { "plain".into() },
        kind: "town".into(),
        border: border[t],
    }));
    let mut config = base.clone();
    config.villages = places;
    config.map.enabled = true;
    config.map.roads = roads.into_iter().collect();
    config.map.rivers = rivers;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::default_config;

    #[test]
    fn a_region_is_laid_out_the_same_way_from_the_same_settings() {
        let s = RegionSettings { villages: 30, ..Default::default() };
        let a = region(&s, &default_config()).unwrap();
        let b = region(&s, &default_config()).unwrap();
        assert_eq!(a, b);
        let c = region(&RegionSettings { seed: 2, ..s }, &default_config()).unwrap();
        assert_ne!(a.villages, c.villages);
    }

    #[test]
    fn a_region_has_its_villages_a_town_on_the_river_and_every_place_reachable() {
        let s = RegionSettings { villages: 50, ..Default::default() };
        let config = region(&s, &default_config()).unwrap();
        let geo = crate::geography::build(&config).unwrap();
        let towns = config.villages.iter().filter(|v| v.kind == "town").count();
        assert_eq!(towns, 2);
        assert!(config.villages.iter().any(|v| v.border));
        assert!(config.villages.iter().any(|v| v.name == "City"));
        assert!(geo.transport.iter().all(|t| t.is_finite()), "every place reaches a town");
        assert!(!config.map.rivers.is_empty());
        for terrain in ["plain", "valley", "upland", "forest"] {
            assert!(config.villages.iter().any(|v| v.terrain == terrain), "{terrain}");
        }
        // Hill villages hold less farmland for their people.
        let per_person = |t: &str| {
            let v: Vec<&VillageConfig> =
                config.villages.iter().filter(|v| v.terrain == t && v.kind == "village").collect();
            v.iter().map(|v| v.land).sum::<f64>() / v.iter().map(|v| v.population as f64).sum::<f64>()
        };
        assert!(per_person("upland") < per_person("plain"));
    }
}
