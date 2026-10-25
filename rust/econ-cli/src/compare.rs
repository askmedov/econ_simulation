//! What a switch changes (`econ-sim compare minds|harvest-hands|refugees`;
//! `econ-sim minds` for short): each seed is run four ways, without and
//! with the switch, each without and with the forced events, and the two
//! are compared seed by seed: in how the years go, and in what the event
//! does.

use std::time::Instant;

use econ_core::config::Config;
use econ_core::metrics::MonthRecord;
use econ_core::scenarios::{run_many, with_seed, Run};

use crate::options::RunArgs;

/// A setting that can be switched on, to see what it changes.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum Switch {
    /// Families with minds of their own (`MindsConfig`)
    Minds,
    /// Townsfolk out in the villages' fields at harvest (`HarvestHandsConfig`)
    HarvestHands,
    /// Families fleeing a famine to the towns, and the towns' fevers (`RefugeesConfig`)
    Refugees,
}

impl Switch {
    fn set(self, c: &mut Config, on: bool) {
        match self {
            Switch::Minds => c.minds.enabled = on,
            Switch::HarvestHands => c.harvest_hands.enabled = on,
            Switch::Refugees => c.refugees.enabled = on,
        }
    }

    /// (without, with, what the runs are)
    fn names(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Switch::Minds => ("rules", "minds", "families by the rules, and with minds of their own"),
            Switch::HarvestHands => ("without", "with", "without and with harvest hands from the towns"),
            Switch::Refugees => ("without", "with", "without and with famine refugees going to the towns"),
        }
    }
}

/// What is measured in each run, by name.
const MEASURES: [&str; 20] = [
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
    "food produced",
    "harvest help",
    "people in towns at the end",
    "deaths in towns",
    "towns' worst month",
];

fn measures(run: &Run, event_month: usize) -> Vec<f64> {
    let records = &run.records;
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
        sum(&|r| r.food_produced),
        sum(&|r| r.harvest_help),
        last.town_people as f64,
        town_deaths(run),
        least(&|r| r.town_ration),
    ]
}

fn town_deaths(run: &Run) -> f64 {
    let towns: Vec<usize> = (0..run.config.villages.len()).filter(|&v| run.config.villages[v].kind == "town").collect();
    run.place("deaths").iter().map(|month| towns.iter().map(|&t| month[t]).sum::<f64>()).sum()
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

pub fn run(args: &RunArgs, switch: Switch) -> Result<(), String> {
    let base = args.config()?;
    let forced = args.forced()?;
    let runs = args.runs.max(2);
    let event_month = forced.first().map_or(4, |e| e.month as usize);
    let (without, with, what) = switch.names();
    let kind = |on: bool, with_event: bool| -> Vec<Config> {
        (0..runs)
            .map(|i| {
                let mut c = with_seed(&base, base.seed + i as i64);
                switch.set(&mut c, on);
                if with_event {
                    c.scheduled_events.extend_from_slice(&forced);
                }
                c
            })
            .collect()
    };
    println!(
        "{} paired runs of {} months, seeds {} to {}: {what}",
        runs,
        base.months,
        base.seed,
        base.seed + runs as i64 - 1
    );
    let (rules, rules_time) = timed(kind(false, false))?;
    let (minds, minds_time) = timed(kind(true, false))?;
    let of = |runs: &[Run]| -> Vec<Vec<f64>> { runs.iter().map(|r| measures(r, event_month)).collect() };
    let (rules_m, minds_m) = (of(&rules), of(&minds));
    print_table("Without the event (seed by seed):", without, with, &rules_m, &minds_m);
    if !forced.is_empty() {
        let names: Vec<String> = forced.iter().map(|e| format!("{} in month {}", e.event, e.month)).collect();
        let (rules_e, _) = timed(kind(false, true))?;
        let (minds_e, _) = timed(kind(true, true))?;
        let effect = |with: &[Vec<f64>], without: &[Vec<f64>]| -> Vec<Vec<f64>> {
            with.iter().zip(without).map(|(w, o)| w.iter().zip(o).map(|(a, b)| a - b).collect()).collect()
        };
        print_table(
            &format!("What {} does (with it, less without it, seed by seed):", names.join(" and ")),
            without,
            with,
            &effect(&of(&rules_e), &rules_m),
            &effect(&of(&minds_e), &minds_m),
        );
    }
    println!(
        "\nSpeed: {:.1} ms a run {without}, {:.1} ms {with} ({:+.0}%)",
        1000.0 * rules_time,
        1000.0 * minds_time,
        100.0 * (minds_time / rules_time - 1.0)
    );
    Ok(())
}
