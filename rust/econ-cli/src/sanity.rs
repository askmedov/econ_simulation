//! Run many seeds and settings and flag outcomes no real village would show
//! (the port of `scripts/sanity_check.py`, with its settings exported from
//! Python to `rust/configs/sanity.json`).
//!
//! Three kinds of finding: impossible values (negative food, broken
//! accounting), nonsense (hunger with a full granary, a year of hunger with
//! no cause, implausible birth rates), and disasters (starvation rations, a
//! year in which 30% die), allowed in a small share of runs, more in harsh
//! settings (a setting fails when its count would be under 1% likely at
//! that share).

use std::collections::BTreeMap;

use econ_core::checks::broken_invariants;
use econ_core::config::Config;
use econ_core::metrics::{Cell, MonthRecord};
use econ_core::Simulation;
use rayon::prelude::*;
use serde_json::Value;

pub const SUITE: &str = include_str!("../../configs/sanity.json");

const IMPOSSIBLE: [&str; 6] = [
    "not-a-number value",
    "negative value",
    "ration or health out of range",
    "age groups don't add up",
    "money created or destroyed",
    "price not positive",
];
const DISASTERS: [&str; 3] = [
    "starvation rations (below 50%)",
    "deaths above 30% in a year",
    // A small village can starve out, or empty as its people move away after
    // disasters or a bad living, as thousands of medieval villages did; a
    // village of many hundreds dying out within a few years is nonsense.
    "small village died out or was abandoned",
];

struct Setting {
    label: String,
    config: Config,
    seeds: usize,
    /// Share of runs allowed a disaster; None: an extreme setting where only
    /// impossible values count.
    disasters_allowed: Option<f64>,
}

fn settings(full: bool) -> Result<Vec<Setting>, String> {
    let items: Vec<Value> = serde_json::from_str(SUITE).map_err(|e| e.to_string())?;
    items
        .into_iter()
        .map(|item| {
            Ok(Setting {
                label: item["label"].as_str().unwrap_or("").to_string(),
                seeds: item[if full { "seeds_full" } else { "seeds" }].as_u64().unwrap_or(1) as usize,
                disasters_allowed: item["disasters_allowed"].as_f64(),
                config: serde_json::from_value(item["config"].clone()).map_err(|e| e.to_string())?,
            })
        })
        .collect()
}

fn coins(r: &MonthRecord) -> f64 {
    r.savings
        + r.business_cash
        + r.treasury
        + r.dealers_cash
        + r.lenders_coins
        + r.lord_purse
        + r.state_purse
        + r.town_purse
        + r.coins_lost
}

fn is_close(a: f64, b: f64) -> bool {
    (a - b).abs() <= (1e-6 * a.abs().max(b.abs())).max(1e-6)
}

/// (problem, month number) for everything implausible in a finished run. In
/// an `extreme` setting (e.g. no food at all) famine is the right answer, so
/// only impossible values are flagged.
pub fn problems(sim: &Simulation, extreme: bool) -> Vec<(String, i64)> {
    let mut found = Vec::new();
    let config = &sim.config;
    let records = &sim.records;
    let start_pop: i64 = config.villages.iter().map(|v| v.population).sum();
    let one_village = config.villages.len() == 1;
    let mut flag = |name: &str, month: i64| found.push((name.to_string(), month));
    let money = records.first().map_or(0.0, coins);
    let start_prices: Vec<f64> = records.first().map_or(Vec::new(), |r| r.prices.iter().map(|(_, v)| *v).collect());
    let mut hungry_amid_plenty = 0;
    for r in records {
        let m = r.month_number;
        if r.flat().iter().any(|(_, v)| matches!(v, Cell::Float(x) if !x.is_finite())) {
            flag("not-a-number value", m);
        }
        let lowest = [
            r.population as f64,
            r.food_stock,
            r.food_eaten,
            r.food_produced,
            r.avg_health,
            r.savings,
            r.business_cash,
            r.treasury,
            r.food_reserve,
        ]
        .iter()
        .cloned()
        .fold(f64::INFINITY, f64::min);
        if lowest < 0.0 {
            flag("negative value", m);
        }
        if !is_close(coins(r), money) {
            flag("money created or destroyed", m);
        }
        if !r.prices.iter().all(|(_, p)| *p > 0.0) && r.population > 0 {
            flag("price not positive", m);
        }
        if !(0.0..=1.0 + 1e-9).contains(&r.ration) || r.avg_health > 100.0 + 1e-9 {
            flag("ration or health out of range", m);
        }
        if r.children + r.workers + r.elderly != r.population {
            flag("age groups don't add up", m);
        }
        if extreme {
            continue;
        }
        // Families own their stores, so a poor family can go short while a
        // rich one holds plenty; but the whole village going hungry amid full
        // stores means something is broken.
        if one_village && r.food_needed > 0.0 && r.ration < 0.85 && r.food_stock > 6.0 * r.food_needed {
            flag("rationing with 6+ months of food in store", m);
        }
        if r.food_needed > 0.0 && r.food_stock > 36.0 * r.food_needed {
            flag("more than 3 years of food in store", m);
        }
        if r.population >= 20 && r.ration < 0.5 {
            flag("starvation rations (below 50%)", m);
        }
        if r.prices.iter().zip(&start_prices).any(|((_, v), s)| !(s / 20.0 < *v && *v < 20.0 * s)) {
            flag("a price off by 20x", m);
        }
        let farmers = r.jobs.iter().find(|(name, _)| name == "farming").map_or(1, |(_, k)| *k);
        if r.population >= 50 && farmers == 0 {
            flag("nobody farms", m);
        }
        // Families too poor to buy food while the granary is full, for months.
        let plenty = one_village && r.food_stock > 6.0 * r.food_needed;
        hungry_amid_plenty =
            if plenty && r.underfed as f64 > r.population as f64 / 6.0 { hungry_amid_plenty + 1 } else { 0 };
        if hungry_amid_plenty == 6 {
            flag("many go hungry for months while the granary is full", m);
        }
    }
    // The state at the end: land, debts, households and couples consistent.
    for name in broken_invariants(&sim.world) {
        flag(&format!("broken invariant: {name}"), records.len() as i64);
    }
    if extreme {
        return found;
    }
    if records.last().is_some_and(|r| r.population == 0) {
        let died = records.iter().find(|r| r.population == 0).map_or(0, |r| r.month_number);
        flag(if start_pop < 300 { "small village died out or was abandoned" } else { "village died out" }, died);
    }
    // Yearly rates, over full years only.
    let mut start = 0;
    while start + 12 <= records.len() {
        let year = &records[start..start + 12];
        start += 12;
        let pop = year.iter().map(|r| r.population as f64).sum::<f64>() / 12.0;
        if pop < 20.0 {
            continue;
        }
        let births: i64 = year.iter().map(|r| r.births).sum();
        let deaths: i64 = year.iter().map(|r| r.deaths).sum();
        // Allow for luck in small villages: four standard deviations above a
        // high but real birth rate of 6% a year.
        if births as f64 > 0.06 * pop + 4.0 * (0.06 * pop).sqrt() {
            flag("implausibly many births in a year", year[0].month_number);
        }
        if deaths as f64 / pop > 0.30 {
            flag("deaths above 30% in a year", year[0].month_number);
        }
        // Hunger is explained by an event or by too many people on the land.
        if one_village && year.iter().all(|r| r.ration < 0.8 && r.events.is_empty() && r.food_margin >= 1.05) {
            flag("a year of hunger with no event or crowding to explain it", year[0].month_number);
        }
    }
    if records.len() >= 36 && start_pop >= 50 {
        let pops: Vec<i64> = records.iter().map(|r| r.population).collect();
        let mut i = 36;
        while i < pops.len() {
            if pops[i] as f64 > 1.5 * pops[i - 36].max(1) as f64 {
                flag("population up 50%+ in 3 years", i as i64 + 1);
                break;
            }
            i += 12;
        }
    }
    let pop = &sim.world.population;
    let (mut adults, mut women) = (0u64, 0u64);
    for i in 0..pop.len() {
        if pop.age_years(i) >= 15 {
            adults += pop.count[i] as u64;
            if pop.female[i] {
                women += pop.count[i] as u64;
            }
        }
    }
    if adults >= 40 {
        let share = women as f64 / adults as f64;
        if !(0.3..=0.7).contains(&share) {
            flag("lopsided sex ratio among adults", records.len() as i64);
        }
    }
    found
}

/// Whether `count` disasters in `runs` runs is implausible (under 1% likely)
/// if they happen in at most `allowed` of runs.
pub fn too_many(count: usize, runs: usize, allowed: f64) -> bool {
    if allowed <= 0.0 {
        return count > 0;
    }
    // The binomial tail, summed in logs.
    let ln_choose = |n: usize, k: usize| -> f64 {
        let ln_fact = |m: usize| (1..=m).map(|x| (x as f64).ln()).sum::<f64>();
        ln_fact(n) - ln_fact(k) - ln_fact(n - k)
    };
    let tail: f64 = (count..=runs)
        .map(|k| {
            (ln_choose(runs, k) + k as f64 * allowed.ln() + (runs - k) as f64 * (1.0 - allowed).max(1e-300).ln()).exp()
        })
        .sum();
    tail < 0.01
}

fn check(config: &Config, extreme: bool) -> Vec<(String, i64)> {
    match Simulation::new(config.clone()) {
        Ok(mut sim) => {
            sim.run();
            problems(&sim, extreme)
        }
        Err(e) => vec![(format!("broken invariant: the setting doesn't start ({e})"), 0)],
    }
}

/// Run every setting over its seeds; returns how many settings failed.
pub fn run(full: bool) -> Result<usize, String> {
    let chosen = settings(full)?;
    let jobs: Vec<(usize, i64)> = chosen
        .iter()
        .enumerate()
        .flat_map(|(s, setting)| (0..setting.seeds as i64).map(move |seed| (s, seed)))
        .collect();
    let found: Vec<Vec<(String, i64)>> = jobs
        .par_iter()
        .map(|&(s, seed)| {
            let mut config = chosen[s].config.clone();
            config.seed = seed;
            check(&config, chosen[s].disasters_allowed.is_none())
        })
        .collect();
    let mut failures: Vec<&str> = Vec::new();
    let mut runs = 0;
    let mut next = found.iter();
    for setting in &chosen {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        let mut examples: BTreeMap<String, String> = BTreeMap::new();
        for seed in 0..setting.seeds {
            runs += 1;
            let mut seen: Vec<&str> = Vec::new();
            for (name, month) in next.next().expect("a result per run") {
                if seen.contains(&name.as_str()) {
                    continue;
                }
                seen.push(name);
                *counts.entry(name.clone()).or_default() += 1;
                examples.entry(name.clone()).or_insert_with(|| format!("seed {seed}, month {month}"));
            }
        }
        let mut notes = Vec::new();
        let mut ordered: Vec<(&String, &usize)> = counts.iter().collect();
        ordered.sort_by(|a, b| b.1.cmp(a.1));
        let mut failed = false;
        for (name, &n) in ordered {
            let (bad, kind) = if DISASTERS.contains(&name.as_str()) {
                (too_many(n, setting.seeds, setting.disasters_allowed.unwrap_or(0.0)), "disaster")
            } else {
                let impossible = IMPOSSIBLE.contains(&name.as_str()) || name.starts_with("broken invariant");
                (true, if impossible { "impossible" } else { "nonsense" })
            };
            notes.push(format!(
                "{} {kind}: {name} in {n} of {} runs (e.g. {})",
                if bad { "FAIL" } else { "ok  " },
                setting.seeds,
                examples[name]
            ));
            failed |= bad;
        }
        if failed {
            failures.push(&setting.label);
        }
        println!("{:<4} {} ({} runs)", if failed { "FAIL" } else { "ok" }, setting.label, setting.seeds);
        for note in notes {
            println!("       {note}");
        }
    }
    let summary = if failures.is_empty() {
        "all settings passed".to_string()
    } else {
        format!("{} settings failed", failures.len())
    };
    println!("\n{runs} runs checked; {summary}");
    Ok(failures.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_few_disasters_in_many_runs_are_luck_and_many_are_not() {
        assert!(!too_many(2, 100, 0.05));
        assert!(too_many(15, 100, 0.05));
        assert!(too_many(1, 10, 0.0));
        assert!(!too_many(0, 10, 0.0));
    }

    #[test]
    fn every_setting_of_the_suite_reads() {
        let quick = settings(false).unwrap();
        let full = settings(true).unwrap();
        assert_eq!(quick.len(), full.len());
        assert!(quick.iter().zip(&full).all(|(q, f)| q.seeds <= f.seeds));
    }
}
