//! `econ-sim starts`: the same region run from several starting points,
//! each from the same seeds, side by side, for up to ten years; with
//! `--event`, what the event does from each of them.

use std::path::Path;

use econ_core::config::Config;
use econ_core::geography::thousands;
use econ_core::metrics::MonthRecord;
use econ_core::scenarios::{run_many, with_seed, Run};
use econ_core::starts::{self, Start};

use crate::options::RunArgs;

/// Starting points are compared over at most this many months: what a
/// start leads to shows within ten years, and beyond that the luck of the
/// years since matters more than where the region began.
pub const MOST_MONTHS: i64 = 120;

pub fn list() {
    let all = starts::starts();
    let width = all.iter().map(|s| s.name.len()).max().unwrap_or(0);
    for start in all {
        println!("  {:width$}  {}", start.name, start.about);
    }
}

/// What a group of runs (one start, one arm) came to.
struct Outcome {
    people_start: f64,
    people_end: f64,
    deaths: f64,      // a year per 1,000
    births: f64,      // a year per 1,000
    left: f64,        // a year per 1,000
    grain_price: f64, // in the last year
    pay_covers: f64,  // in the last year: food a month's pay buys, over a worker's share of need
    worst_month: f64,
    poorest_worst: f64,
    hungry_months: f64,
    debt: f64,     // a family, at the end
    landless: f64, // share of people, at the end
    in_towns: f64, // share of people, at the end
    yearly: Vec<f64>,
    total_deaths: f64,
    highest_price: f64,
    total_left: f64,
}

fn mean(values: impl Iterator<Item = f64>) -> f64 {
    let (sum, n) = values.fold((0.0, 0usize), |(s, n), v| (s + v, n + 1));
    if n == 0 {
        0.0
    } else {
        sum / n as f64
    }
}

fn outcome(runs: &[Run], people_start: f64) -> Outcome {
    let per_run = |f: &dyn Fn(&[MonthRecord]) -> f64| mean(runs.iter().map(|r| f(&r.records)));
    let years = |r: &[MonthRecord]| r.len() as f64 / 12.0;
    let average_people = |r: &[MonthRecord]| mean(r.iter().map(|m| m.population as f64)).max(1.0);
    let rate = |r: &[MonthRecord], v: &dyn Fn(&MonthRecord) -> f64| {
        1000.0 * r.iter().map(v).sum::<f64>() / average_people(r) / years(r)
    };
    let last_year = |r: &[MonthRecord], v: &dyn Fn(&MonthRecord) -> f64| mean(r.iter().rev().take(12).map(v));
    let end = |r: &[MonthRecord]| r.last().expect("a month").clone();
    let months = runs[0].records.len();
    Outcome {
        people_start,
        people_end: per_run(&|r| end(r).population as f64),
        deaths: per_run(&|r| rate(r, &|m| m.deaths as f64)),
        births: per_run(&|r| rate(r, &|m| m.births as f64)),
        left: per_run(&|r| rate(r, &|m| m.emigrants as f64)),
        grain_price: per_run(&|r| last_year(r, &|m| m.food_price)),
        pay_covers: per_run(&|r| last_year(r, &|m| m.wage_cover)),
        worst_month: per_run(&|r| r.iter().map(|m| m.ration).fold(f64::INFINITY, f64::min)),
        poorest_worst: per_run(&|r| r.iter().map(|m| m.poorest_fifth_ration).fold(f64::INFINITY, f64::min)),
        hungry_months: per_run(&|r| r.iter().filter(|m| m.ration < 0.97).count() as f64),
        debt: per_run(&|r| end(r).debt / (end(r).households.max(1) as f64)),
        landless: per_run(&|r| end(r).landless as f64 / (end(r).population.max(1) as f64)),
        in_towns: per_run(&|r| end(r).town_people as f64 / (end(r).population.max(1) as f64)),
        yearly: (12..=months)
            .step_by(12)
            .map(|m| mean(runs.iter().map(|r| r.records[m - 1].population as f64)))
            .collect(),
        total_deaths: per_run(&|r| r.iter().map(|m| m.deaths as f64).sum()),
        highest_price: per_run(&|r| r.iter().map(|m| m.food_price).fold(f64::NEG_INFINITY, f64::max)),
        total_left: per_run(&|r| r.iter().map(|m| m.emigrants as f64).sum()),
    }
}

fn percent(x: f64) -> String {
    format!("{:.0}%", 100.0 * x)
}

pub fn run(args: &RunArgs, only: &[String]) -> Result<(), String> {
    if args.start.is_some() {
        return Err("`starts` runs every starting point (or those named with --only), not one --start".into());
    }
    let base = args.config()?;
    if base.months > MOST_MONTHS {
        return Err(format!("starting points are compared over at most {MOST_MONTHS} months (10 years)"));
    }
    if base.months < 12 {
        return Err("starting points are compared over a year at least (--months 12 or more)".into());
    }
    let chosen: Vec<Start> = if only.is_empty() {
        starts::starts()
    } else {
        only.iter().map(|name| starts::find(name)).collect::<Result<_, _>>()?
    };
    let forced = args.forced()?;
    let configs: Vec<Config> = chosen.iter().map(|s| starts::apply(&base, s)).collect::<Result<_, _>>()?;
    let runs = args.runs.max(1);
    let arms = if forced.is_empty() { 1 } else { 2 };
    let mut all = Vec::new();
    for config in &configs {
        for arm in 0..arms {
            let mut c = config.clone();
            if arm == 1 {
                c.scheduled_events.extend_from_slice(&forced);
            }
            all.extend((0..runs).map(|i| with_seed(&c, base.seed + i as i64)));
        }
    }
    let size: i64 = base.villages.iter().map(|v| v.population).sum();
    let event = if forced.is_empty() {
        String::new()
    } else {
        let names: Vec<String> = forced.iter().map(|e| format!("{} in month {}", e.event, e.month)).collect();
        format!(", each without and with {}", names.join(" and "))
    };
    println!(
        "{} starting points for {} of {} people, {} months, {} run{} each from seed {}{event}:",
        chosen.len(),
        if base.villages.len() > 1 { "a region" } else { "a village" },
        thousands(size),
        base.months,
        runs,
        if runs != 1 { "s" } else { "" },
        base.seed
    );
    list_chosen(&chosen);
    let mut results = run_many(all)?;
    let mut outcomes: Vec<(Outcome, Option<Outcome>)> = Vec::new();
    for config in &configs {
        let people: f64 = config.villages.iter().map(|v| v.population as f64).sum();
        let mine: Vec<Run> = results.drain(..runs * arms).collect();
        let base_runs = outcome(&mine[..runs], people);
        let event_runs = (arms == 2).then(|| outcome(&mine[runs..], people));
        outcomes.push((base_runs, event_runs));
    }
    print_where_they_lead(&chosen, &outcomes, runs);
    print_people_by_year(&chosen, &outcomes);
    if arms == 2 {
        print_event(&chosen, &outcomes);
    }
    let out = Path::new(&args.out);
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    write_csv(&out.join("starts.csv"), &chosen, &outcomes).map_err(|e| format!("{}: {e}", out.display()))?;
    println!("\nWrote starts.csv to {}/", args.out);
    Ok(())
}

fn list_chosen(chosen: &[Start]) {
    let width = chosen.iter().map(|s| s.name.len()).max().unwrap_or(0);
    for start in chosen {
        println!("  {:width$}  {}", start.name, start.about);
    }
}

fn name_width(chosen: &[Start]) -> usize {
    chosen.iter().map(|s| s.name.len()).max().unwrap_or(0).max(14)
}

fn print_where_they_lead(chosen: &[Start], outcomes: &[(Outcome, Option<Outcome>)], runs: usize) {
    let w = name_width(chosen);
    println!(
        "\nWhere each start leads ({}; rates a year over the whole run, price and pay in its last year):",
        if runs == 1 { "one run".to_string() } else { format!("average of {runs} runs") }
    );
    println!(
        "{:w$} {:>21} {:>7} {:>7} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>8} {:>6}",
        "",
        "People at the end",
        "Deaths",
        "Births",
        "Left",
        "Grain",
        "Pay",
        "Worst",
        "Poor",
        "Hungry",
        "Land-",
        "Debt a",
        "In"
    );
    println!(
        "{:w$} {:>21} {:>7} {:>7} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>8} {:>6}",
        "",
        "(change)",
        "/1,000",
        "/1,000",
        "/1,000",
        "price",
        "covers",
        "month",
        "fifth",
        "months",
        "less",
        "family",
        "towns"
    );
    for (start, (o, _)) in chosen.iter().zip(outcomes) {
        let change = 100.0 * (o.people_end / o.people_start - 1.0);
        println!(
            "{:w$} {:>12} ({:>+5.1}%) {:>7.1} {:>7.1} {:>6.1} {:>6.2} {:>6.2} {:>6} {:>6} {:>6.1} {:>6} {:>8.2} {:>6}",
            start.name,
            thousands(o.people_end.round() as i64),
            change,
            o.deaths,
            o.births,
            o.left,
            o.grain_price,
            o.pay_covers,
            percent(o.worst_month),
            percent(o.poorest_worst),
            o.hungry_months,
            percent(o.landless),
            o.debt,
            percent(o.in_towns),
        );
    }
    println!(
        "(Pay covers: food a month's pay buys, over a worker's share of the need; worst month: the share of\n \
         its need the region ate in its hungriest month, and the poorest fifth's; hungry months: under 97%)"
    );
}

fn print_people_by_year(chosen: &[Start], outcomes: &[(Outcome, Option<Outcome>)]) {
    let w = name_width(chosen);
    let years = outcomes[0].0.yearly.len();
    let mut header = format!("\nPeople at the end of each year:\n{:w$} {:>11}", "", "At start");
    for y in 1..=years {
        header += &format!(" {:>11}", format!("Year {y}"));
    }
    println!("{header}");
    for (start, (o, _)) in chosen.iter().zip(outcomes) {
        let mut line = format!("{:w$} {:>11}", start.name, thousands(o.people_start.round() as i64));
        for p in &o.yearly {
            line += &format!(" {:>11}", thousands(p.round() as i64));
        }
        println!("{line}");
    }
}

fn print_event(chosen: &[Start], outcomes: &[(Outcome, Option<Outcome>)]) {
    let w = name_width(chosen);
    println!("\nWhat the event does from each start:");
    println!(
        "{:w$} {:>12} {:>9} {:>12} {:>15} {:>15} {:>19}",
        "", "Extra deaths", "per 1,000", "People lost", "Worst month", "Highest price", "People who left"
    );
    for (start, (a, b)) in chosen.iter().zip(outcomes) {
        let b = b.as_ref().expect("runs with the event");
        let extra = b.total_deaths - a.total_deaths;
        println!(
            "{:w$} {:>12} {:>9.1} {:>12} {:>15} {:>15} {:>19}",
            start.name,
            thousands(extra.round() as i64),
            1000.0 * extra / a.people_start,
            thousands((a.people_end - b.people_end).round() as i64),
            format!("{} -> {}", percent(a.worst_month), percent(b.worst_month)),
            format!("{:.2} -> {:.2}", a.highest_price, b.highest_price),
            format!("{} -> {}", thousands(a.total_left.round() as i64), thousands(b.total_left.round() as i64)),
        );
    }
}

fn write_csv(path: &Path, chosen: &[Start], outcomes: &[(Outcome, Option<Outcome>)]) -> std::io::Result<()> {
    let mut text = String::from(
        "start,arm,people_start,people_end,deaths_per_1000,births_per_1000,left_per_1000,grain_price,pay_covers,\
         worst_month,poorest_worst_month,hungry_months,debt_per_family,landless_share,town_share",
    );
    let years = outcomes[0].0.yearly.len();
    for y in 1..=years {
        text += &format!(",people_year_{y}");
    }
    text.push('\n');
    for (start, (a, b)) in chosen.iter().zip(outcomes) {
        for (arm, o) in [("base", Some(a)), ("event", b.as_ref())] {
            let Some(o) = o else { continue };
            text += &format!(
                "{},{arm},{},{},{:.3},{:.3},{:.3},{:.4},{:.4},{:.4},{:.4},{:.2},{:.4},{:.4},{:.4}",
                start.name,
                o.people_start,
                o.people_end.round(),
                o.deaths,
                o.births,
                o.left,
                o.grain_price,
                o.pay_covers,
                o.worst_month,
                o.poorest_worst,
                o.hungry_months,
                o.debt,
                o.landless,
                o.in_towns
            );
            for p in &o.yearly {
                text += &format!(",{}", p.round());
            }
            text.push('\n');
        }
    }
    std::fs::write(path, text)
}
