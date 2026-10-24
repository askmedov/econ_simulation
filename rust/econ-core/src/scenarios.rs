//! What-if experiments: many runs, with and without forced events.
//!
//! Every run with the forced events is paired with a baseline run that has
//! the same seed. Because each rule draws from its own named random stream,
//! the pair shares the same luck up to the month the event starts, so the
//! gap between them is the event's effect rather than chance. Repeating this
//! over many seeds shows how large and how certain that effect is. Runs go
//! in parallel (rayon), each on one thread, so the results never depend on
//! the number of threads.

use std::io::Write;
use std::path::Path;

use rayon::prelude::*;

use crate::config::{Config, ScheduledEvent};
use crate::metrics::{python_float, round_to, MonthRecord};
use crate::simulation::Simulation;

/// Monthly measures worth comparing, in the order they are reported.
pub const METRICS: [&str; 16] = [
    "population",
    "births",
    "deaths",
    "food_stock",
    "food_price",
    "wage",
    "ration",
    "poorest_fifth_ration",
    "landless_ration",
    "underfed",
    "avg_health",
    "poor_health",
    "relief",
    "treated",
    "treasury",
    "food_reserve",
];

/// What a finished run leaves: its settings, monthly records and log (not
/// its world, which can be large), and a few measures for each place (see
/// `simulation::PLACE_SERIES`), as (months x places).
#[derive(Clone, Debug)]
pub struct Run {
    pub config: Config,
    pub records: Vec<MonthRecord>,
    pub log: Vec<String>,
    pub places: Vec<(&'static str, Vec<Vec<f64>>)>,
}

impl Run {
    pub fn place(&self, name: &str) -> &Vec<Vec<f64>> {
        &self.places.iter().find(|(n, _)| *n == name).unwrap_or_else(|| panic!("no place series {name:?}")).1
    }
}

pub fn run_one(config: Config) -> Result<Run, String> {
    let mut sim = Simulation::new(config.clone())?;
    sim.run();
    Ok(Run { config, records: sim.records.clone(), log: sim.log.clone(), places: sim.place_series() })
}

/// Run each config, several at a time on separate threads; results in order.
pub fn run_many(configs: Vec<Config>) -> Result<Vec<Run>, String> {
    configs.into_par_iter().map(run_one).collect()
}

/// `runs` runs with seeds config.seed, config.seed + 1, ...
pub fn run_batch(config: &Config, runs: usize) -> Result<Vec<Run>, String> {
    run_many((0..runs).map(|i| with_seed(config, config.seed + i as i64)).collect())
}

pub fn with_seed(config: &Config, seed: i64) -> Config {
    let mut c = config.clone();
    c.seed = seed;
    c
}

/// A metric as (runs x months).
pub fn series(runs: &[Run], metric: &str) -> Vec<Vec<f64>> {
    runs.iter().map(|run| run.records.iter().map(|r| r.value(metric)).collect()).collect()
}

/// numpy's `percentile` (linear between the closest ranks).
pub fn percentile(values: &[f64], q: f64) -> f64 {
    let mut sorted: Vec<f64> = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if sorted.is_empty() {
        return f64::NAN;
    }
    let at = q / 100.0 * (sorted.len() - 1) as f64;
    let (low, high) = (at.floor() as usize, at.ceil() as usize);
    sorted[low] + (sorted[high] - sorted[low]) * (at - low as f64)
}

/// Average across runs, with the range that 80% of runs fall into.
#[derive(Clone, Debug, PartialEq)]
pub struct Spread {
    pub mean: f64,
    pub low: f64,  // 10th percentile
    pub high: f64, // 90th percentile
}

pub fn spread(values: &[f64]) -> Spread {
    Spread {
        mean: values.iter().sum::<f64>() / values.len() as f64,
        low: percentile(values, 10.0),
        high: percentile(values, 90.0),
    }
}

/// The spread across runs of each month of a (runs x months) table.
pub fn spread_by_month(values: &[Vec<f64>]) -> Vec<Spread> {
    let months = values.first().map_or(0, |v| v.len());
    (0..months).map(|m| spread(&values.iter().map(|run| run[m]).collect::<Vec<_>>())).collect()
}

/// Paired runs: the same seeds without and with the forced events.
#[derive(Clone, Debug)]
pub struct Comparison {
    pub baseline: Vec<Run>,
    pub scenario: Vec<Run>,
}

impl Comparison {
    /// Scenario minus baseline, run by run (runs x months).
    pub fn difference(&self, metric: &str) -> Vec<Vec<f64>> {
        let (a, b) = (series(&self.baseline, metric), series(&self.scenario, metric));
        a.iter().zip(&b).map(|(x, y)| x.iter().zip(y).map(|(p, q)| q - p).collect()).collect()
    }
}

/// Paired runs of `config` without and with the `forced` events (all of
/// them at once, in parallel).
pub fn compare(config: &Config, forced: &[ScheduledEvent], runs: usize) -> Result<Comparison, String> {
    let mut with_events = config.clone();
    with_events.scheduled_events.extend_from_slice(forced);
    let mut configs: Vec<Config> = (0..runs).map(|i| with_seed(config, config.seed + i as i64)).collect();
    configs.extend((0..runs).map(|i| with_seed(&with_events, config.seed + i as i64)));
    let mut results = run_many(configs)?;
    let scenario = results.split_off(runs);
    Ok(Comparison { baseline: results, scenario })
}

/// What the forced events did over the whole run, across runs.
#[derive(Clone, Debug)]
pub struct Effect {
    pub extra_deaths: Spread,
    pub fewer_births: Spread,
    pub population_change: Spread,         // at the end
    pub lowest_ration: (f64, f64),         // (baseline, scenario), averaged over runs
    pub poorest_lowest_ration: (f64, f64), // the poorest fifth's worst month
    pub landless_lowest_ration: (f64, f64),
    pub hungry_months: (f64, f64), // months in which the place ate under 97% of its need
    pub highest_price: (f64, f64), // food price at its peak
    pub lowest_health: (f64, f64),
    pub left: (f64, f64),        // people who left their place over the whole run
    pub relief: (f64, f64),      // rations the council gave out over the whole run
    pub lord_relief: (f64, f64), // rations a charitable lord gave out over the whole run
    pub treated: (f64, f64),     // people healers saw over the whole run
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

fn per_run(runs: &[Run], metric: &str, stat: impl Fn(&[f64]) -> f64) -> f64 {
    mean(&series(runs, metric).iter().map(|v| stat(v)).collect::<Vec<_>>())
}

pub fn effect(c: &Comparison) -> Effect {
    let pair = |metric: &str, stat: &dyn Fn(&[f64]) -> f64| {
        (per_run(&c.baseline, metric, stat), per_run(&c.scenario, metric, stat))
    };
    let min = |v: &[f64]| v.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = |v: &[f64]| v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let total = |v: &[f64]| v.iter().sum::<f64>();
    let hungry = |v: &[f64]| v.iter().filter(|&&r| r < 0.97).count() as f64;
    let totals = |table: Vec<Vec<f64>>| table.iter().map(|v| v.iter().sum()).collect::<Vec<f64>>();
    Effect {
        extra_deaths: spread(&totals(c.difference("deaths"))),
        fewer_births: spread(&totals(c.difference("births")).iter().map(|x| -x).collect::<Vec<_>>()),
        population_change: spread(
            &c.difference("population").iter().map(|v| *v.last().unwrap_or(&0.0)).collect::<Vec<_>>(),
        ),
        lowest_ration: pair("ration", &min),
        poorest_lowest_ration: pair("poorest_fifth_ration", &min),
        landless_lowest_ration: pair("landless_ration", &min),
        hungry_months: pair("ration", &hungry),
        highest_price: pair("food_price", &max),
        lowest_health: pair("avg_health", &min),
        left: pair("emigrants", &total),
        relief: pair("relief", &total),
        lord_relief: pair("lord_relief", &total),
        treated: pair("treated", &total),
    }
}

/// A per-place measure as (runs x months x places).
pub fn place_series<'a>(runs: &'a [Run], name: &str) -> Vec<&'a Vec<Vec<f64>>> {
    runs.iter().map(|r| r.place(name)).collect()
}

/// What the forced events did to one group of places (by terrain, or by
/// distance from market), across runs.
#[derive(Clone, Debug)]
pub struct PlaceEffect {
    pub label: String,
    pub villages: usize,
    pub people: f64, // at the start
    pub extra_deaths: Spread,
    pub population_change: Spread, // at the end
    pub lowest_ration: (f64, f64), // (baseline, scenario): the group's worst month, averaged over runs
    pub left: (f64, f64),          // people who left its places over the whole run
}

/// The forced events' effect on each group of places (`groups` gives each
/// place's index into `labels`, or -1 for none); empty groups are left out.
pub fn effect_by(c: &Comparison, groups: &[i64], labels: &[String]) -> Vec<PlaceEffect> {
    let start: Vec<f64> = c.baseline[0].config.villages.iter().map(|v| v.population as f64).collect();
    let mut out = Vec::new();
    for (g, label) in labels.iter().enumerate() {
        let mine: Vec<usize> = (0..groups.len()).filter(|&i| groups[i] == g as i64).collect();
        if mine.is_empty() {
            continue;
        }
        // runs x months, summed over the group's places
        let total = |runs: &[Run], name: &str| -> Vec<Vec<f64>> {
            runs.iter()
                .map(|r| r.place(name).iter().map(|month| mine.iter().map(|&i| month[i]).sum()).collect())
                .collect()
        };
        let worst = |runs: &[Run]| -> f64 {
            mean(
                &runs
                    .iter()
                    .map(|r| {
                        let fed = r.place("ration");
                        let people = r.place("population");
                        fed.iter()
                            .zip(people)
                            .map(|(f, p)| {
                                let weights: f64 = mine.iter().map(|&i| p[i].max(1e-9)).sum();
                                mine.iter().map(|&i| f[i] * p[i].max(1e-9)).sum::<f64>() / weights
                            })
                            .fold(f64::INFINITY, f64::min)
                    })
                    .collect::<Vec<_>>(),
            )
        };
        let (d0, d1) = (total(&c.baseline, "deaths"), total(&c.scenario, "deaths"));
        let (p0, p1) = (total(&c.baseline, "population"), total(&c.scenario, "population"));
        let (l0, l1) = (total(&c.baseline, "left"), total(&c.scenario, "left"));
        let runs = d0.len();
        let sum = |v: &Vec<f64>| v.iter().sum::<f64>();
        out.push(PlaceEffect {
            label: label.clone(),
            villages: mine.len(),
            people: mine.iter().map(|&i| start[i]).sum(),
            extra_deaths: spread(&(0..runs).map(|r| sum(&d1[r]) - sum(&d0[r])).collect::<Vec<_>>()),
            population_change: spread(
                &(0..runs).map(|r| p1[r].last().unwrap_or(&0.0) - p0[r].last().unwrap_or(&0.0)).collect::<Vec<_>>(),
            ),
            lowest_ration: (worst(&c.baseline), worst(&c.scenario)),
            left: (mean(&l0.iter().map(sum).collect::<Vec<_>>()), mean(&l1.iter().map(sum).collect::<Vec<_>>())),
        });
    }
    out
}

/// A number as Python's summary files show it: whole numbers plain, others
/// to 3 decimals without trailing zeros.
pub fn fmt_number(value: f64) -> String {
    if value.is_nan() {
        return "nan".into();
    }
    let text = format!("{:.3}", round_to(value, 3));
    let text = text.trim_end_matches('0').trim_end_matches('.').to_string();
    if text == "-0" {
        "0".into()
    } else {
        text
    }
}

/// One row per month: each metric's average and 10-90% range across runs.
/// With a scenario, adds the same for the scenario (`_event`) and for the
/// difference between paired runs (`_diff`).
pub fn write_summary_csv(path: &Path, baseline: &[Run], scenario: Option<&[Run]>) -> std::io::Result<()> {
    let first = &baseline[0].records;
    let mut columns: Vec<(String, Vec<String>)> = vec![
        ("month_number".into(), first.iter().map(|r| r.month_number.to_string()).collect()),
        ("year".into(), first.iter().map(|r| r.year.to_string()).collect()),
        ("month".into(), first.iter().map(|r| r.month.to_string()).collect()),
    ];
    for metric in METRICS {
        let base = series(baseline, metric);
        let mut groups: Vec<(&str, Vec<Vec<f64>>)> = vec![("", base.clone())];
        if let Some(scenario) = scenario {
            let with = series(scenario, metric);
            let diff: Vec<Vec<f64>> =
                base.iter().zip(&with).map(|(a, b)| a.iter().zip(b).map(|(x, y)| y - x).collect()).collect();
            groups.push(("_event", with));
            groups.push(("_diff", diff));
        }
        for (suffix, values) in groups {
            let s = spread_by_month(&values);
            columns.push((format!("{metric}{suffix}"), s.iter().map(|x| fmt_number(x.mean)).collect()));
            columns.push((format!("{metric}{suffix}_low"), s.iter().map(|x| fmt_number(x.low)).collect()));
            columns.push((format!("{metric}{suffix}_high"), s.iter().map(|x| fmt_number(x.high)).collect()));
        }
    }
    write_columns(path, &columns, first.len())
}

/// One row per place: what it is (`about`: terrain, place on the map, cost
/// of carriage...) and how it fared, averaged over runs (and with the forced
/// events, if any).
pub fn write_places_csv(
    path: &Path,
    names: &[String],
    about: &[(String, Vec<String>)],
    baseline: &[Run],
    scenario: Option<&[Run]>,
) -> std::io::Result<()> {
    let mut columns: Vec<(String, Vec<String>)> = vec![("village".into(), names.to_vec())];
    columns.extend_from_slice(about);
    let mut arms: Vec<(&str, &[Run])> = vec![("", baseline)];
    if let Some(s) = scenario {
        arms.push(("_event", s));
    }
    let n = names.len();
    for (suffix, runs) in arms {
        let average = |f: &dyn Fn(&Run, usize) -> f64| -> Vec<String> {
            (0..n).map(|i| fmt_number(mean(&runs.iter().map(|r| f(r, i)).collect::<Vec<_>>()))).collect()
        };
        let total = |name: &'static str| move |r: &Run, i: usize| r.place(name).iter().map(|m| m[i]).sum::<f64>();
        columns.push((
            format!("population_end{suffix}"),
            average(&|r, i| r.place("population").last().map_or(0.0, |m| m[i])),
        ));
        columns.push((format!("deaths{suffix}"), average(&total("deaths"))));
        columns.push((format!("left{suffix}"), average(&total("left"))));
        columns.push((
            format!("lowest_ration{suffix}"),
            average(&|r, i| r.place("ration").iter().map(|m| m[i]).fold(f64::INFINITY, f64::min)),
        ));
        columns.push((
            format!("highest_price{suffix}"),
            average(&|r, i| r.place("food_price").iter().map(|m| m[i]).fold(f64::NEG_INFINITY, f64::max)),
        ));
    }
    write_columns(path, &columns, n)
}

fn write_columns(path: &Path, columns: &[(String, Vec<String>)], rows: usize) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
    let header: Vec<String> = columns.iter().map(|(n, _)| crate::metrics::csv_text(n)).collect();
    writeln!(out, "{}", header.join(","))?;
    for i in 0..rows {
        let row: Vec<String> = columns.iter().map(|(_, v)| crate::metrics::csv_text(&v[i])).collect();
        writeln!(out, "{}", row.join(","))?;
    }
    out.flush()
}

/// A float written as Python writes it (used for `about` columns).
pub fn about_number(value: f64) -> String {
    if value.is_infinite() {
        return python_float(value);
    }
    fmt_number(value)
}
