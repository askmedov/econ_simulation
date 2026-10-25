//! What minds change (`econ-sim minds`): each seed is run four ways, by the
//! rules and with minds, each without and with the forced events, and the
//! two kinds are compared seed by seed: in how the years go, and in what
//! the event does.

use std::time::Instant;

use econ_core::config::Config;
use econ_core::metrics::MonthRecord;
use econ_core::scenarios::{run_many, with_seed, Run};

use crate::options::RunArgs;

/// What is measured in each run, by name.
const MEASURES: [&str; 15] = [
    "deaths",
    "births",
    "people at the end",
    "average grain price",
    "highest grain price",
    "lowest ration",
    "poorest fifth, worst month",
    "grain sold in the 6 months from the event",
    "food in store when grain is dearest",
    "grain sent to town",
    "coins lent",
    "debts written off",
    "people who left",
    "families who left",
    "savings at the end",
];

fn measures(records: &[MonthRecord], event_month: usize) -> Vec<f64> {
    let sum = |f: &dyn Fn(&MonthRecord) -> f64| records.iter().map(f).sum::<f64>();
    let least = |f: &dyn Fn(&MonthRecord) -> f64| records.iter().map(f).fold(f64::INFINITY, f64::min);
    let dearest = records.iter().max_by(|a, b| a.food_price.total_cmp(&b.food_price)).expect("a month");
    let from = event_month.saturating_sub(1).min(records.len());
    let last = records.last().expect("a month");
    vec![
        sum(&|r| r.deaths as f64),
        sum(&|r| r.births as f64),
        last.population as f64,
        sum(&|r| r.food_price) / records.len() as f64,
        dearest.food_price,
        least(&|r| r.ration),
        least(&|r| r.poorest_fifth_ration),
        records[from..(from + 6).min(records.len())].iter().map(|r| r.grain_sold).sum(),
        dearest.food_stock,
        sum(&|r| r.grain_exported),
        sum(&|r| r.borrowed),
        sum(&|r| r.debts_written_off),
        sum(&|r| r.emigrants as f64),
        sum(&|r| r.families_left as f64),
        last.savings,
    ]
}

/// Mean of `values` and the t of that mean against nought (paired runs:
/// the differences seed by seed).
fn mean_and_t(values: &[f64]) -> (f64, f64) {
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    let var = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1.0).max(1.0);
    let se = (var / n).sqrt();
    let t = if se > 0.0 {
        mean / se
    } else if mean == 0.0 {
        0.0
    } else {
        f64::INFINITY
    };
    (mean, t)
}

fn print_table(title: &str, left: &str, right: &str, a: &[Vec<f64>], b: &[Vec<f64>]) {
    println!("\n{title}");
    println!("  {:<44} {:>12} {:>12} {:>12} {:>7}", "", left, right, "difference", "t");
    for (k, name) in MEASURES.iter().enumerate() {
        let left: Vec<f64> = a.iter().map(|m| m[k]).collect();
        let right: Vec<f64> = b.iter().map(|m| m[k]).collect();
        let diff: Vec<f64> = left.iter().zip(&right).map(|(x, y)| y - x).collect();
        let (d, t) = mean_and_t(&diff);
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        println!(
            "  {:<44} {:>12} {:>12} {:>12} {:>7.1}",
            name,
            number(mean(&left)),
            number(mean(&right)),
            number(d),
            t
        );
    }
}

fn number(x: f64) -> String {
    if x.abs() >= 100.0 {
        format!("{x:.0}")
    } else if x.abs() >= 1.0 {
        format!("{x:.2}")
    } else {
        format!("{x:.3}")
    }
}

/// The runs of `configs`, timed: (runs, seconds a run on one thread).
fn timed(configs: Vec<Config>) -> Result<(Vec<Run>, f64), String> {
    let n = configs.len() as f64;
    let threads = rayon::current_num_threads() as f64;
    let start = Instant::now();
    let runs = run_many(configs)?;
    Ok((runs, start.elapsed().as_secs_f64() * threads.min(n) / n))
}

pub fn run(args: &RunArgs) -> Result<(), String> {
    let base = args.config()?;
    let forced = args.forced()?;
    let runs = args.runs.max(2);
    let event_month = forced.first().map_or(4, |e| e.month as usize);
    let kind = |minds: bool, with_event: bool| -> Vec<Config> {
        (0..runs)
            .map(|i| {
                let mut c = with_seed(&base, base.seed + i as i64);
                c.minds.enabled = minds;
                if with_event {
                    c.scheduled_events.extend_from_slice(&forced);
                }
                c
            })
            .collect()
    };
    println!(
        "{} paired runs of {} months, seeds {} to {}: families by the rules, and with minds of their own",
        runs,
        base.months,
        base.seed,
        base.seed + runs as i64 - 1
    );
    let (rules, rules_time) = timed(kind(false, false))?;
    let (minds, minds_time) = timed(kind(true, false))?;
    let of = |runs: &[Run]| -> Vec<Vec<f64>> { runs.iter().map(|r| measures(&r.records, event_month)).collect() };
    let (rules_m, minds_m) = (of(&rules), of(&minds));
    print_table("Without the event (seed by seed):", "rules", "minds", &rules_m, &minds_m);
    if !forced.is_empty() {
        let names: Vec<String> = forced.iter().map(|e| format!("{} in month {}", e.event, e.month)).collect();
        let (rules_e, _) = timed(kind(false, true))?;
        let (minds_e, _) = timed(kind(true, true))?;
        let effect = |with: &[Vec<f64>], without: &[Vec<f64>]| -> Vec<Vec<f64>> {
            with.iter().zip(without).map(|(w, o)| w.iter().zip(o).map(|(a, b)| a - b).collect()).collect()
        };
        print_table(
            &format!("What {} does (with it, less without it, seed by seed):", names.join(" and ")),
            "rules",
            "minds",
            &effect(&of(&rules_e), &rules_m),
            &effect(&of(&minds_e), &minds_m),
        );
    }
    println!(
        "\nSpeed: {:.1} ms a run by the rules, {:.1} ms with minds ({:+.0}%)",
        1000.0 * rules_time,
        1000.0 * minds_time,
        100.0 * (minds_time / rules_time - 1.0)
    );
    Ok(())
}
