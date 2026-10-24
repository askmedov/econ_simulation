//! Run the village through many scenarios for decades and look for drift
//! (the port of `scripts/stress_test.py`, with its scenarios exported from
//! Python to `rust/configs/stress.json`).
//!
//! Each scenario is run for several seeds; every year the village is
//! measured (population, prices, wages, how families eat, the mix of
//! trades, how money is spread, food stores, family sizes). The report
//! compares the first years with the last, and flags drift: a measure that
//! wanders out of a plausible range or keeps trending. Yearly averages per
//! scenario are written to <out>/<scenario>.csv.

use std::io::Write;
use std::path::Path;

use econ_core::config::Config;
use econ_core::households::sizes;
use econ_core::Simulation;
use rayon::prelude::*;
use serde_json::Value;

pub const SUITE: &str = include_str!("../../configs/stress.json");

struct Scenario {
    name: String,
    config: Config,
    note: String,
    /// Measures this scenario is meant to push out of their usual range:
    /// reported, but not counted as drift.
    expected: Vec<String>,
}

fn scenarios() -> Result<Vec<Scenario>, String> {
    let items: Vec<Value> = serde_json::from_str(SUITE).map_err(|e| e.to_string())?;
    items
        .into_iter()
        .map(|item| {
            Ok(Scenario {
                name: item["name"].as_str().unwrap_or("").to_string(),
                note: item["note"].as_str().unwrap_or("").to_string(),
                expected: item["expected"]
                    .as_array()
                    .map_or(Vec::new(), |a| a.iter().map(|x| x.as_str().unwrap_or("").to_string()).collect()),
                config: serde_json::from_value(item["config"].clone()).map_err(|e| e.to_string())?,
            })
        })
        .collect()
}

/// numpy's `interp` (x within or beyond the points `xp`, increasing).
fn interp(x: f64, xp: &[f64], fp: &[f64]) -> f64 {
    if x <= xp[0] {
        return fp[0];
    }
    if x >= xp[xp.len() - 1] {
        return fp[fp.len() - 1];
    }
    let i = xp.partition_point(|&p| p <= x);
    let (x0, x1, y0, y1) = (xp[i - 1], xp[i], fp[i - 1], fp[i]);
    if x1 == x0 {
        y1
    } else {
        y0 + (y1 - y0) * (x - x0) / (x1 - x0)
    }
}

/// Gini of money per person (people-weighted), top-10% share, bottom-half share.
fn gini_and_top(money: &[f64], people: &[f64]) -> (f64, f64, f64) {
    let mut pairs: Vec<(f64, f64)> =
        (0..money.len()).filter(|&i| people[i] > 0.0).map(|i| (money[i].max(0.0), people[i])).collect();
    let total: f64 = pairs.iter().map(|p| p.0).sum();
    if total <= 0.0 {
        return (0.0, 0.0, 0.0);
    }
    pairs.sort_by(|a, b| (a.0 / a.1).partial_cmp(&(b.0 / b.1)).unwrap_or(std::cmp::Ordering::Equal));
    let all_people: f64 = pairs.iter().map(|p| p.1).sum();
    let (mut cum_people, mut cum_money) = (Vec::new(), Vec::new());
    let (mut cp, mut cm) = (0.0, 0.0);
    for &(m, p) in &pairs {
        cp += p;
        cm += m;
        cum_people.push(cp / all_people);
        cum_money.push(cm / total);
    }
    let mut gini = 1.0;
    let (mut previous_people, mut previous_money) = (0.0, 0.0);
    for i in 0..pairs.len() {
        gini -= (cum_people[i] - previous_people) * (cum_money[i] + previous_money);
        previous_people = cum_people[i];
        previous_money = cum_money[i];
    }
    let top = 1.0 - interp(0.9, &cum_people, &cum_money);
    let bottom = interp(0.5, &cum_people, &cum_money);
    (gini, top, bottom)
}

type Yearly = Vec<(String, Vec<f64>)>;

fn add(rows: &mut Yearly, key: &str, value: f64) {
    match rows.iter_mut().find(|(k, _)| k == key) {
        Some((_, values)) => values.push(value),
        None => rows.push((key.to_string(), vec![value])),
    }
}

fn last(rows: &Yearly, key: &str) -> f64 {
    *rows.iter().find(|(k, _)| k == key).and_then(|(_, v)| v.last()).unwrap_or(&f64::NAN)
}

fn mean(v: impl Iterator<Item = f64>) -> f64 {
    let (mut total, mut n) = (0.0, 0.0);
    for x in v {
        total += x;
        n += 1.0;
    }
    total / n
}

/// One seed of one scenario: yearly measures.
fn run_one(config: &Config, seed: i64, years: usize) -> Result<Yearly, String> {
    let mut c = config.clone();
    c.seed = seed;
    c.months = 12 * years as i64;
    let mut sim = Simulation::new(c)?;
    let mut rows: Yearly = Vec::new();
    let names: Vec<String> = sim.config.businesses.iter().map(|b| b.name.clone()).collect();
    for _ in 0..years {
        let year: Vec<_> = (0..12).map(|_| sim.step()).collect();
        let w = &sim.world;
        let jobs = year.iter().map(|r| r.jobs.iter().map(|(_, k)| *k as f64).sum::<f64>()).sum::<f64>() / 12.0;
        let workers = if jobs == 0.0 { 1.0 } else { jobs };
        let first_pop = year[0].population.max(1) as f64;
        let products: Vec<String> = year[0].prices.iter().map(|(k, _)| k.clone()).collect();
        add(&mut rows, "population", year[11].population as f64);
        add(&mut rows, "births_per_1000", 1000.0 * year.iter().map(|r| r.births as f64).sum::<f64>() / first_pop);
        add(&mut rows, "deaths_per_1000", 1000.0 * year.iter().map(|r| r.deaths as f64).sum::<f64>() / first_pop);
        add(&mut rows, "weddings_per_1000", 1000.0 * year.iter().map(|r| r.weddings as f64).sum::<f64>() / first_pop);
        add(&mut rows, "sown", year.iter().map(|r| r.sown).fold(f64::INFINITY, f64::min));
        add(&mut rows, "animals_per_plot", year[11].animals / w.land.iter().sum::<f64>().max(1e-9));
        add(&mut rows, "wage_cover", mean(year.iter().map(|r| r.wage_cover)));
        add(&mut rows, "food_price", mean(year.iter().map(|r| r.food_price)));
        for (p, product) in products.iter().enumerate() {
            add(&mut rows, &format!("price_{product}"), mean(year.iter().map(|r| r.prices[p].1)));
        }
        add(&mut rows, "wage", mean(year.iter().map(|r| r.wage)));
        // Prices in days of work: money's own level moves with how much there
        // is. A hamlet of a few survivors has no market to speak of.
        let wage = if year[11].population >= 20 { last(&rows, "wage").max(1e-9) } else { f64::NAN };
        let real_food = last(&rows, "food_price") / wage;
        add(&mut rows, "real_food_price", real_food);
        for product in &products {
            let real = last(&rows, &format!("price_{product}")) / wage;
            add(&mut rows, &format!("real_price_{product}"), real);
        }
        add(&mut rows, "ration", mean(year.iter().map(|r| r.ration)));
        add(&mut rows, "poorest_fifth", mean(year.iter().map(|r| r.poorest_fifth_ration)));
        add(
            &mut rows,
            "poorest_fifth_worst",
            year.iter().map(|r| r.poorest_fifth_ration).fold(f64::INFINITY, f64::min),
        );
        add(&mut rows, "warmth", mean(year.iter().map(|r| r.warmth)));
        add(&mut rows, "woods", mean(year.iter().map(|r| r.woods)));
        add(&mut rows, "soil", mean(year.iter().map(|r| r.soil)));
        add(&mut rows, "health", mean(year.iter().map(|r| r.avg_health)));
        add(&mut rows, "clothing_per_person_year", year.iter().map(|r| r.clothing).sum());
        for (b, name) in names.iter().enumerate() {
            add(
                &mut rows,
                &format!("share_{name}"),
                year.iter().map(|r| r.jobs[b].1 as f64).sum::<f64>() / 12.0 / workers,
            );
        }
        add(&mut rows, "food_store_months", mean(year.iter().map(|r| r.food_stock / r.food_needed.max(1e-9))));
        add(&mut rows, "food_cover", mean(year.iter().map(|r| r.food_cover)));
        let total_money = w.money();
        add(&mut rows, "treasury_share", w.council.treasury.iter().sum::<f64>() / total_money);
        add(&mut rows, "business_cash_share", w.cash.sum() / total_money);
        let size = sizes(&w.population, w.households.len());
        let (gini, top, bottom) = gini_and_top(&w.households.money, &size);
        add(&mut rows, "land_gini", gini_and_top(&w.households.land, &size).0);
        let price = w.food_price();
        let wealth: Vec<f64> = (0..w.households.len())
            .map(|h| w.households.money[h] + w.households.grain[h] * price[w.households.location[h] as usize])
            .collect();
        add(&mut rows, "wealth_gini", gini_and_top(&wealth, &size).0);
        add(&mut rows, "landless_share", mean(year.iter().map(|r| r.landless as f64 / r.population.max(1) as f64)));
        add(&mut rows, "own_food_share", mean(year.iter().map(|r| r.own_food / r.food_eaten.max(1e-9))));
        add(&mut rows, "money_gini", gini);
        add(&mut rows, "money_top10_share", top);
        add(&mut rows, "money_bottom_half_share", bottom);
        let living: Vec<f64> = size.iter().cloned().filter(|&s| s > 0.0).collect();
        add(&mut rows, "household_size", if living.is_empty() { 0.0 } else { mean(living.iter().cloned()) });
        add(&mut rows, "household_size_max", size.iter().cloned().fold(0.0, f64::max));
        add(&mut rows, "households", living.len() as f64);
        add(&mut rows, "reserve_months", w.council.reserve.iter().sum::<f64>() / year[11].food_needed.max(1e-9));
    }
    Ok(rows)
}

/// (measure, lowest plausible, highest plausible) for the average of the last five years.
const RANGES: [(&str, f64, f64); 25] = [
    ("ration", 0.9, 1.01),
    ("poorest_fifth", 0.85, 1.01),
    ("warmth", 0.85, 1.01),
    ("health", 75.0, 100.0),
    ("share_farming", 0.55, 0.92),
    ("share_woodcutting", 0.02, 0.25),
    ("share_smithing", 0.005, 0.15),
    ("treasury_share", 0.0, 0.25),
    ("business_cash_share", 0.0, 0.3),
    ("money_top10_share", 0.0, 0.9),
    ("money_gini", 0.0, 0.95),
    ("wealth_gini", 0.0, 0.85),
    ("land_gini", 0.0, 0.9),
    ("landless_share", 0.0, 0.75),
    ("food_store_months", 0.5, 12.0),
    ("household_size", 3.0, 9.0),
    ("household_size_max", 0.0, 30.0),
    ("births_per_1000", 30.0, 60.0),
    ("deaths_per_1000", 25.0, 60.0),
    ("weddings_per_1000", 5.0, 25.0),
    ("sown", 0.85, 1.01),
    ("animals_per_plot", 0.1, 0.5),
    ("wage_cover", 0.6, 2.5),
    ("woods", 0.5, 1.5),
    ("soil", 0.85, 1.11),
];
/// Measures that should stay within these multiples of their first-year value.
const RELATIVE: [(&str, f64, f64); 5] = [
    ("population", 0.6, 1.6),
    ("real_food_price", 0.4, 2.5),
    ("real_price_firewood", 0.33, 3.0),
    ("real_price_clothing", 0.33, 3.0),
    ("real_price_tools", 0.33, 3.0),
];
/// Measures whose trend over the second half shouldn't exceed this share per decade.
const TRENDS: [(&str, f64); 5] = [
    ("population", 0.15),
    ("real_food_price", 0.3),
    ("wage_cover", 0.3),
    ("household_size", 0.15),
    ("land_gini", 0.15),
];
/// Shares that may be small, whose trend is checked in points per decade instead.
const TRENDS_ABSOLUTE: [(&str, f64); 1] = [("landless_share", 0.15)];

/// The least-squares slope of `values` against 0, 1, 2, ...
fn slope(values: &[f64]) -> f64 {
    let n = values.len() as f64;
    let mx = (n - 1.0) / 2.0;
    let my = values.iter().sum::<f64>() / n;
    let (mut cov, mut var) = (0.0, 0.0);
    for (i, &v) in values.iter().enumerate() {
        cov += (i as f64 - mx) * (v - my);
        var += (i as f64 - mx).powi(2);
    }
    cov / var
}

fn series<'a>(mean: &'a Yearly, key: &str) -> &'a [f64] {
    mean.iter().find(|(k, _)| k == key).map_or(&[], |(_, v)| v.as_slice())
}

fn nan_mean(values: &[f64]) -> f64 {
    let finite: Vec<f64> = values.iter().cloned().filter(|v| !v.is_nan()).collect();
    if finite.is_empty() {
        f64::NAN
    } else {
        finite.iter().sum::<f64>() / finite.len() as f64
    }
}

/// (measure, message) for each measure that drifted. Real prices are
/// compared with `reference`'s first year (the baseline village, as a
/// scenario's own first year may be a famine); other measures with their own.
fn drift_flags(mean: &Yearly, reference: Option<&Yearly>) -> Vec<(String, String)> {
    let mut flags = Vec::new();
    let years = series(mean, "population").len();
    let last = years.saturating_sub(5)..years;
    for (key, low, high) in RANGES.iter() {
        let values = series(mean, key);
        let value = nan_mean(&values[last.clone()]);
        if value.is_finite() && !(*low <= value && value <= *high) {
            flags.push((key.to_string(), format!("{key} {value:.2} outside [{}, {}]", g(*low), g(*high))));
        }
    }
    for (key, low, high) in RELATIVE {
        let first = match reference {
            Some(r) if key.starts_with("real_") => r,
            _ => mean,
        };
        let start = series(first, key)[0];
        let value = nan_mean(&series(mean, key)[last.clone()]);
        let ratio = if start != 0.0 { value / start } else { f64::INFINITY };
        if value.is_finite() && !(low <= ratio && ratio <= high) {
            flags.push((key.to_string(), format!("{key} x{ratio:.2} from year 1 ({start:.2} -> {value:.2})")));
        }
    }
    for (key, limit) in TRENDS {
        let half: Vec<f64> = series(mean, key)[years / 2..].iter().cloned().filter(|v| v.is_finite()).collect();
        if half.len() >= 6 && half.iter().sum::<f64>() / half.len() as f64 > 0.0 {
            let logs: Vec<f64> = half.iter().map(|v| v.max(1e-9).ln()).collect();
            let per_decade = slope(&logs) * 10.0;
            if per_decade.abs() > limit {
                flags.push((key.to_string(), format!("{key} still trending {:+.0}% a decade", 100.0 * per_decade)));
            }
        }
    }
    for (key, limit) in TRENDS_ABSOLUTE {
        let half = &series(mean, key)[years / 2..];
        if half.len() >= 6 {
            let per_decade = slope(half) * 10.0;
            if per_decade.abs() > limit {
                flags.push((key.to_string(), format!("{key} still trending {per_decade:+.2} a decade")));
            }
        }
    }
    flags
}

fn g(x: f64) -> String {
    if x == x.trunc() {
        format!("{}", x as i64)
    } else {
        format!("{x}")
    }
}

fn file_name(name: &str) -> String {
    name.replace(' ', "_").replace('%', "pct").replace(',', "")
}

/// Run the scenarios whose name contains `only`; returns how many drifted.
/// Each seed's yearly measures, one line of JSON per scenario and seed.
pub fn each_seed(years: usize, seeds: usize, only: &str) -> Result<(), String> {
    let chosen: Vec<Scenario> =
        scenarios()?.into_iter().filter(|s| s.name.to_lowercase().contains(&only.to_lowercase())).collect();
    let jobs: Vec<(usize, i64)> =
        (0..chosen.len()).flat_map(|s| (0..seeds as i64).map(move |seed| (s, seed))).collect();
    let results: Vec<Yearly> =
        jobs.par_iter().map(|&(s, seed)| run_one(&chosen[s].config, seed, years)).collect::<Result<_, _>>()?;
    for (&(s, seed), yearly) in jobs.iter().zip(&results) {
        let measures: serde_json::Map<String, serde_json::Value> =
            yearly.iter().map(|(key, values)| (key.clone(), serde_json::json!(values))).collect();
        println!("{}", serde_json::json!({"scenario": chosen[s].name, "seed": seed, "measures": measures}));
    }
    Ok(())
}

pub fn run(years: usize, seeds: usize, only: &str, out: &str) -> Result<usize, String> {
    let chosen: Vec<Scenario> =
        scenarios()?.into_iter().filter(|s| s.name.to_lowercase().contains(&only.to_lowercase())).collect();
    let jobs: Vec<(usize, i64)> =
        (0..chosen.len()).flat_map(|s| (0..seeds as i64).map(move |seed| (s, seed))).collect();
    let results: Vec<Yearly> =
        jobs.par_iter().map(|&(s, seed)| run_one(&chosen[s].config, seed, years)).collect::<Result<_, _>>()?;
    let out = Path::new(out);
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    println!("{} scenarios x {seeds} seeds x {years} years\n", chosen.len());
    println!(
        "{:<30} {:>11} {:>11} {:>11} {:>6} {:>6} {:>5} {:>4} {:>4} {:>5} {:>5} {:>5} {:>5} {:>5} {:>4}",
        "scenario",
        "pop",
        "food price",
        "wage",
        "ration",
        "poor5",
        "warm",
        "b/1k",
        "d/1k",
        "farm",
        "weave",
        "gini",
        "top10",
        "treas",
        "hh"
    );
    // Yearly averages over seeds (years when every run's village was empty are not-a-number).
    let means: Vec<Yearly> = (0..chosen.len())
        .map(|s| {
            let runs = &results[s * seeds..(s + 1) * seeds];
            runs[0]
                .iter()
                .map(|(key, _)| {
                    let by_year: Vec<f64> = (0..years)
                        .map(|y| nan_mean(&runs.iter().map(|r| series(r, key)[y]).collect::<Vec<_>>()))
                        .collect();
                    (key.clone(), by_year)
                })
                .collect()
        })
        .collect();
    let reference = chosen.iter().position(|s| s.name == "baseline").map(|i| &means[i]);
    let mut drifted = 0;
    for (scenario, mean) in chosen.iter().zip(&means) {
        let mut file = std::io::BufWriter::new(
            std::fs::File::create(out.join(format!("{}.csv", file_name(&scenario.name)))).map_err(|e| e.to_string())?,
        );
        let keys: Vec<&str> = mean.iter().map(|(k, _)| k.as_str()).collect();
        write!(file, "year,{}\r\n", keys.join(",")).map_err(|e| e.to_string())?;
        for y in 0..years {
            let row: Vec<String> = mean.iter().map(|(_, v)| format_g4(v[y])).collect();
            write!(file, "{},{}\r\n", y + 1, row.join(",")).map_err(|e| e.to_string())?;
        }
        let m = |k: &str| nan_mean(&series(mean, k)[years.saturating_sub(5)..]);
        let first = |k: &str| series(mean, k)[0];
        println!(
            "{:<30} {:>5.0}>{:<5.0} {:>5.2}>{:<5.2} {:>5.2}>{:<5.2} {:>6.2} {:>6.2} {:>5.2} {:>4.0} {:>4.0} {:>5.2} {:>5.2} {:>5.2} {:>5.2} {:>5.2} {:>4.1}",
            scenario.name,
            first("population"),
            m("population"),
            first("food_price"),
            m("food_price"),
            first("wage"),
            m("wage"),
            m("ration"),
            m("poorest_fifth"),
            m("warmth"),
            m("births_per_1000"),
            m("deaths_per_1000"),
            m("share_farming"),
            m("share_weaving"),
            m("money_gini"),
            m("money_top10_share"),
            m("treasury_share"),
            m("household_size")
        );
        let flags = drift_flags(mean, reference);
        let surprises: Vec<&String> =
            flags.iter().filter(|(key, _)| !scenario.expected.contains(key)).map(|(_, m)| m).collect();
        for (key, message) in &flags {
            if scenario.expected.contains(key) {
                println!("    expected ({}): {message}", scenario.note);
            }
        }
        if !surprises.is_empty() {
            drifted += 1;
            for message in surprises {
                println!("    DRIFT {message}");
            }
        }
    }
    println!("\n{drifted} of {} scenarios drifted; yearly averages in {}/", chosen.len(), out.display());
    Ok(drifted)
}

/// A number to 4 significant digits, as Python's `f"{x:.4g}"`.
fn format_g4(x: f64) -> String {
    if !x.is_finite() {
        return if x.is_nan() {
            "nan".into()
        } else if x > 0.0 {
            "inf".into()
        } else {
            "-inf".into()
        };
    }
    if x == 0.0 {
        return "0".into();
    }
    let exponent = x.abs().log10().floor() as i32;
    if !(-5..4).contains(&exponent) {
        let text = format!("{:.3e}", x);
        let (mantissa, exp) = text.split_once('e').unwrap();
        let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
        let exp: i32 = exp.parse().unwrap();
        return format!("{mantissa}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs());
    }
    let decimals = (3 - exponent).max(0) as usize;
    let text = format!("{x:.decimals$}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_spread_evenly_has_no_gini_and_all_in_one_hand_nearly_one() {
        let (gini, top, bottom) = gini_and_top(&[10.0; 10], &[1.0; 10]);
        assert!(gini.abs() < 1e-9 && (top - 0.1).abs() < 1e-9 && (bottom - 0.5).abs() < 1e-9);
        let mut money = vec![0.0; 10];
        money[9] = 100.0;
        assert!(gini_and_top(&money, &[1.0; 10]).0 > 0.85);
    }

    #[test]
    fn numbers_are_written_to_four_figures_as_python_writes_them() {
        assert_eq!(format_g4(1234.5678), "1235");
        assert_eq!(format_g4(0.012345), "0.01235");
        assert_eq!(format_g4(12345.0), "1.234e+04");
        assert_eq!(format_g4(2.0), "2");
    }

    #[test]
    fn every_scenario_of_the_suite_reads() {
        assert!(scenarios().unwrap().iter().any(|s| s.name == "baseline"));
    }
}
