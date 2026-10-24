//! The environment: woods that shrink when overcut, and soil that tires.
//!
//! A place's woods are a stock of standing wood that regrows logistically:
//! slowly when nearly bare or nearly untouched, fastest in between. Cutting
//! and gathering firewood draw it down. A place starts with its woods in
//! balance with its use; one that grows beyond what they can bear thins
//! them, and as they thin, wood takes longer to cut and gather.
//!
//! Soil fertility follows how crowded the land is: cropped without enough
//! fallow it tires, and land left to rest after a famine recovers.

use crate::config::Config;
use crate::grid::{clip, div_or};

/// (capacity, starting stock) of each place's woods, in units of firewood,
/// for its starting number of `people`, `richness` times the usual: in
/// balance with their yearly firewood need (or at half capacity, the most
/// productive, if they need more than the woods can bear).
pub fn size_woods(people: &[f64], config: &Config, richness: Option<&[f64]>) -> (Vec<f64>, Vec<f64>) {
    let cfg = &config.environment;
    let firewood = &config.needs.firewood;
    let mean_firewood = firewood.iter().sum::<f64>() / firewood.len() as f64;
    let mut capacity = Vec::with_capacity(people.len());
    let mut stock = Vec::with_capacity(people.len());
    for (i, &p) in people.iter().enumerate() {
        let yearly = p * 12.0 * mean_firewood;
        let years = cfg.woods_years * richness.map_or(1.0, |r| r[i]);
        let cap = years * yearly;
        // Balance: regrowth x stock x (1 - stock / capacity) = yearly use.
        let pressure = 1.0 / (cfg.regrowth * years);
        let share = 0.5 * (1.0 + (1.0 - 4.0 * pressure).max(0.0).sqrt());
        capacity.push(cap);
        stock.push(share * cap);
    }
    (capacity, stock)
}

/// How far a day's work cutting or gathering wood goes, against the start,
/// by how much wood is standing (a little further where the woods have
/// grown back thicker).
pub fn woods_reach(woods: &[f64], normal: &[f64], config: &Config) -> Vec<f64> {
    let cfg = &config.environment;
    if !cfg.enabled {
        return vec![1.0; woods.len()];
    }
    woods.iter().zip(normal).map(|(&w, &n)| clip(div_or(w, n, 1.0), 0.0, 1.5).powf(cfg.reach_exponent)).collect()
}

/// A month of the woods: fire burns `burned` (a share), they regrow, and
/// `cut` (firewood cut and gathered) is taken. Changes `woods`; returns the
/// wood lost to fire per place.
pub fn grow_woods(woods: &mut [f64], capacity: &[f64], cut: &[f64], burned: &[f64], config: &Config) -> Vec<f64> {
    let cfg = &config.environment;
    if !cfg.enabled {
        return vec![0.0; woods.len()];
    }
    (0..woods.len())
        .map(|i| {
            let lost = woods[i] * burned[i];
            woods[i] -= lost;
            let full = div_or(woods[i], capacity[i], 1.0);
            woods[i] += cfg.regrowth / 12.0 * woods[i] * (1.0 - full) - cut[i];
            // Some scrub and saplings always remain to grow back from.
            woods[i] = woods[i].max(0.01 * capacity[i]);
            lost
        })
        .collect()
}

/// The fertility each place's soil heads toward, given how crowded its land is.
pub fn soil_target(people: f64, land: f64, config: &Config) -> f64 {
    let cfg = &config.environment;
    let crowding = div_or(people, land * cfg.people_per_plot, 1.0);
    let crowding = if land > 0.0 { crowding } else { 1.0 };
    clip(1.0 - cfg.overcropping * (crowding - 1.0), cfg.fertility_range.0, cfg.fertility_range.1)
}

/// A month's change in soil fertility. Changes `soil`.
pub fn tire_soil(soil: &mut [f64], people: &[f64], land: &[f64], config: &Config) {
    let cfg = &config.environment;
    if !cfg.enabled {
        return;
    }
    let monthly = 1.0 - (1.0 - cfg.soil_recovery).powf(1.0 / 12.0);
    for i in 0..soil.len() {
        soil[i] += monthly * (soil_target(people[i], land[i], config) - soil[i]);
    }
}
