//! What a run or a what-if prints, as Python's command line prints it.

use econ_core::config::{Config, Duration, EffectValue, ScheduledEvent};
use econ_core::geography::{self, thousands};
use econ_core::scenarios::{effect_by, series, spread_by_month, Comparison, Effect, Run, Spread};

const MONTH_ABBR: [&str; 13] = ["", "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const MONTH_NAME: [&str; 13] = [
    "",
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// A share as a whole percentage ("97%").
fn pct(x: f64) -> String {
    format!("{:.0}%", 100.0 * x)
}

/// A number with commas between thousands and `decimals` decimals.
fn commas(x: f64, decimals: usize) -> String {
    let text = format!("{:.*}", decimals, x.abs());
    let (whole, frac) = match text.split_once('.') {
        Some((w, f)) => (w.to_string(), format!(".{f}")),
        None => (text.clone(), String::new()),
    };
    let grouped = thousands(whole.parse::<i64>().unwrap_or(0));
    let sign = if x < 0.0 && text.chars().any(|c| c.is_ascii_digit() && c != '0') { "-" } else { "" };
    format!("{sign}{grouped}{frac}")
}

/// A number as Python's `g` format shows it (2, 0.5, 1.5e+03).
fn g(x: f64) -> String {
    if x == x.trunc() && x.abs() < 1e16 {
        format!("{}", x as i64)
    } else {
        let text = format!("{x}");
        if text.len() > 12 {
            format!("{x:.6}").trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            text
        }
    }
}

fn label(month_number: i64, month: i64, year: i64) -> String {
    format!("{month_number:>3}  {} Y{year}", MONTH_ABBR[month as usize])
}

pub fn event_label(forced: &[ScheduledEvent]) -> String {
    forced.iter().map(|e| format!("{} in month {}", e.event.replace('_', " "), e.month)).collect::<Vec<_>>().join(", ")
}

fn forced_label(e: &ScheduledEvent) -> String {
    let place = match e.near {
        Some((x, y, km)) => format!(" within {} km of ({}, {})", g(km), g(x), g(y)),
        None => String::new(),
    };
    format!("{} in month {}{place}", e.event.replace('_', " "), e.month)
}

pub fn print_header(config: &Config, forced: &[ScheduledEvent], runs: usize) {
    let v = &config.villages[0];
    if config.map.enabled {
        let geo = geography::build(config).expect("a valid map");
        println!("{}", geography::describe(config, &geo));
    } else {
        println!(
            "Village of {} people, {} plots of land, {} months of food in store",
            v.population,
            g(v.land),
            g(v.initial_food_months)
        );
    }
    let randomness = if config.random_events { "random events on" } else { "no random events" };
    println!(
        "{} months from {} of year 1; seed {}; {runs} run{}; {randomness}",
        config.months,
        MONTH_NAME[config.start_month as usize],
        config.seed,
        if runs != 1 { "s" } else { "" }
    );
    let cc = &config.council;
    if !cc.enabled {
        println!("No council");
    } else if config.villages.len() > 1 {
        let big = config.villages.iter().filter(|v| v.population >= cc.forms_at_population).count();
        let start = if cc.established_at_start { format!("{big} start with one") } else { "none at the start".into() };
        println!("Councils in villages of {} people or more ({start})", cc.forms_at_population);
    } else if v.population >= cc.forms_at_population && cc.established_at_start {
        let care = if config.healthcare.enabled {
            format!("{} healers per 1,000", g(config.healthcare.healers_per_1000))
        } else {
            "no healers".into()
        };
        println!(
            "Council: {} tax, {}-month food reserve, relief {}, {care}",
            pct(cc.tax_rate),
            g(cc.reserve_months),
            if cc.relief { "on" } else { "off" }
        );
    } else {
        println!(
            "A council forms once the village has {} people for {} months",
            cc.forms_at_population, cc.forms_after_months
        );
    }
    if !forced.is_empty() {
        println!("Forced: {}", forced.iter().map(forced_label).collect::<Vec<_>>().join(", "));
    }
    println!();
}

fn range(s: &Spread, f: &dyn Fn(f64) -> String) -> String {
    format!("{} ({}-{})", f(s.mean), f(s.low), f(s.high))
}

pub fn print_months(runs: &[Run]) {
    if runs.len() == 1 {
        println!(
            "{:<12} {:>9} {:>6} {:>6} {:>11} {:>6} {:>6} {:>7} {:>8} {:>7}  Events",
            "Month", "Pop", "Born", "Died", "Stock", "Price", "Wage", "Ration", "Poorest", "Health"
        );
        for r in &runs[0].records {
            println!(
                "{:<12} {:>9} {:>6} {:>6} {:>11} {:>6.2} {:>6.2} {:>7} {:>8} {:>7.0}  {}",
                label(r.month_number, r.month, r.year),
                commas(r.population as f64, 0),
                r.births,
                r.deaths,
                commas(r.food_stock, 0),
                r.food_price,
                r.wage,
                pct(r.ration),
                pct(r.poorest_fifth_ration),
                r.avg_health,
                r.events.replace('_', " ").replace('+', ", ")
            );
        }
        return;
    }
    // Many runs: averages, with the range 80% of runs fall into.
    println!("Averages across runs (80% of runs fall in the range in brackets)");
    println!(
        "{:<12} {:>18} {:>16} {:>17} {:>17} {:>17} {:>7}",
        "Month", "Pop", "Died so far", "Food price", "Ration", "Poorest fifth", "Health"
    );
    let cumulative: Vec<Vec<f64>> = series(runs, "deaths")
        .iter()
        .map(|v| {
            let mut total = 0.0;
            v.iter()
                .map(|x| {
                    total += x;
                    total
                })
                .collect()
        })
        .collect();
    let pop = spread_by_month(&series(runs, "population"));
    let deaths = spread_by_month(&cumulative);
    let price = spread_by_month(&series(runs, "food_price"));
    let ration = spread_by_month(&series(runs, "ration"));
    let poorest = spread_by_month(&series(runs, "poorest_fifth_ration"));
    let health = spread_by_month(&series(runs, "avg_health"));
    for (i, r) in runs[0].records.iter().enumerate() {
        println!(
            "{:<12} {:>18} {:>16} {:>17} {:>17} {:>17} {:>7.0}",
            label(r.month_number, r.month, r.year),
            range(&pop[i], &|x| format!("{x:.0}")),
            range(&deaths[i], &|x| format!("{x:.1}")),
            range(&price[i], &|x| format!("{x:.2}")),
            range(&ration[i], &pct),
            range(&poorest[i], &pct),
            health[i].mean
        );
    }
}

fn mean_by_month(runs: &[Run], metric: &str) -> Vec<f64> {
    spread_by_month(&series(runs, metric)).iter().map(|s| s.mean).collect()
}

fn cumulative_gap(a: &[Run], b: &[Run], metric: &str) -> Vec<f64> {
    let (x, y) = (series(a, metric), series(b, metric));
    let runs = x.len() as f64;
    let months = x.first().map_or(0, |v| v.len());
    let mut out = Vec::with_capacity(months);
    let mut total = 0.0;
    for m in 0..months {
        total += (0..x.len()).map(|r| y[r][m] - x[r][m]).sum::<f64>() / runs;
        out.push(total);
    }
    out
}

pub fn print_comparison(baseline: &[Run], scenario: &[Run]) {
    let averaged = baseline.len() > 1;
    if averaged {
        println!("Averages of {} paired runs", baseline.len());
    }
    println!(
        "{:<12} {:>21} {:>15} {:>15} {:>13} {:>9} {:>9}",
        "", "Food in store", "Food price", "Ration", "Health", "Extra", "Fewer"
    );
    println!(
        "{:<12} {:>10}{:>11} {:>8}{:>7} {:>8}{:>7} {:>8}{:>5} {:>9} {:>9}",
        "Month", "without", "with", "without", "with", "without", "with", "without", "with", "deaths", "births"
    );
    let stock = (mean_by_month(baseline, "food_stock"), mean_by_month(scenario, "food_stock"));
    let price = (mean_by_month(baseline, "food_price"), mean_by_month(scenario, "food_price"));
    let ration = (mean_by_month(baseline, "ration"), mean_by_month(scenario, "ration"));
    let health = (mean_by_month(baseline, "avg_health"), mean_by_month(scenario, "avg_health"));
    let extra_deaths = cumulative_gap(baseline, scenario, "deaths");
    let fewer_births = cumulative_gap(scenario, baseline, "births");
    let count = |x: f64| if averaged { format!("{x:>9.1}") } else { format!("{x:>9.0}") };
    for (i, r) in baseline[0].records.iter().enumerate() {
        println!(
            "{:<12} {:>10}{:>11} {:>8.2}{:>7.2} {:>8}{:>7} {:>8.0}{:>5.0} {} {}",
            label(r.month_number, r.month, r.year),
            commas(stock.0[i], 0),
            commas(stock.1[i], 0),
            price.0[i],
            price.1[i],
            pct(ration.0[i]),
            pct(ration.1[i]),
            health.0[i],
            health.1[i],
            count(extra_deaths[i]),
            count(fewer_births[i])
        );
    }
}

/// Python's way of showing a whole effect: a sign if asked, no "-0".
fn number(value: f64, sign: bool, decimals: usize) -> String {
    let scale = 10f64.powi(decimals as i32);
    let rounded = (value * scale).round() / scale + 0.0;
    if rounded == 0.0 {
        return "0".into();
    }
    if sign {
        format!("{rounded:+.decimals$}")
    } else {
        format!("{rounded:.decimals$}")
    }
}

pub fn print_effect(result: &Effect, forced: &[ScheduledEvent], runs: usize, months: i64) {
    print!("\nEffect of {} over {months} months", event_label(forced));
    if runs == 1 {
        println!(" (one run):");
    } else {
        println!(" (average of {runs} runs; 80% of runs fall in the range in brackets):");
    }
    let show = |name: &str, s: &Spread, sign: bool| {
        let mut text = number(s.mean, sign, if runs > 1 { 1 } else { 0 });
        if runs > 1 {
            text += &format!("  ({} to {})", number(s.low, sign, 0), number(s.high, sign, 0));
        }
        println!("  {name:<28}{text}");
    };
    show("Extra deaths", &result.extra_deaths, true);
    show("Fewer births", &result.fewer_births, false);
    show("Population at the end", &result.population_change, true);
    let pair = |name: &str, (a, b): (f64, f64), f: &dyn Fn(f64) -> String| {
        println!("  {name:<28}{} without, {} with", f(a), f(b));
    };
    pair("Highest food price", result.highest_price, &|x| format!("{x:.2}"));
    pair("Lowest ration", result.lowest_ration, &pct);
    pair("Poorest fifth, worst month", result.poorest_lowest_ration, &pct);
    pair("Landless, worst month", result.landless_lowest_ration, &pct);
    let months_fmt = |x: f64| if runs > 1 { format!("{x:.1}") } else { format!("{x:.0}") };
    pair("Months on short rations", result.hungry_months, &months_fmt);
    pair("Lowest average health", result.lowest_health, &|x| format!("{x:.0}"));
    pair("People who left", result.left, &|x| format!("{x:.0}"));
    if result.relief.0 > 0.0 || result.relief.1 > 0.0 {
        pair("Council relief (rations)", result.relief, &|x| format!("{x:.0}"));
    }
    if result.lord_relief.0 > 0.0 || result.lord_relief.1 > 0.0 {
        pair("Lord relief (rations)", result.lord_relief, &|x| format!("{x:.0}"));
    }
    if result.treated.0 > 0.0 || result.treated.1 > 0.0 {
        pair("People treated by healers", result.treated, &|x| format!("{x:.0}"));
    }
}

/// The effect where the forced events struck and elsewhere, in villages and
/// towns, and in villages by terrain and by distance from market.
pub fn print_places(comparison: &Comparison, config: &Config, forced: &[ScheduledEvent]) {
    let geo = geography::build(config).expect("a valid map");
    let n = config.villages.len();
    let village: Vec<bool> = geo.town.iter().map(|t| !t).collect();
    let cost: Vec<f64> = geo.transport.iter().map(|&t| if t.is_finite() { t } else { 9.0 }).collect();
    let village_costs: Vec<f64> = (0..n).filter(|&i| village[i]).map(|i| cost[i]).collect();
    let cuts = if village_costs.is_empty() {
        [0.0, 0.0]
    } else {
        [
            econ_core::scenarios::percentile(&village_costs, 100.0 / 3.0),
            econ_core::scenarios::percentile(&village_costs, 200.0 / 3.0),
        ]
    };
    let thirds: Vec<i64> =
        (0..n).map(|i| if village[i] { cuts.iter().filter(|&&c| c <= cost[i]).count() as i64 } else { -1 }).collect();
    let terrains: Vec<String> = config.map.terrains.iter().map(|t| t.name.clone()).collect();
    let mut tables: Vec<(String, Vec<i64>, Vec<String>)> = vec![
        (
            "kind of place".into(),
            geo.town.iter().map(|&t| t as i64).collect(),
            vec!["villages".into(), "market towns".into()],
        ),
        (
            "terrain (villages)".into(),
            (0..n).map(|i| if village[i] { geo.terrain[i] as i64 } else { -1 }).collect(),
            terrains,
        ),
        (
            "cost of carting grain to market (villages)".into(),
            thirds,
            vec![
                format!("nearest third (under {})", pct(cuts[0])),
                "middle third".into(),
                format!("farthest third (over {})", pct(cuts[1])),
            ],
        ),
    ];
    let mut struck = vec![false; n];
    for e in forced {
        if let Some(near) = e.near {
            for (i, yes) in geo.near(near).iter().enumerate() {
                struck[i] |= *yes;
            }
        } else if let Some(v) = e.village {
            struck[v] = true;
        } else {
            struck.iter_mut().for_each(|s| *s = true);
        }
    }
    if !struck.iter().all(|&s| s) {
        tables.insert(
            0,
            (
                "where the forced events struck".into(),
                struck.iter().map(|&s| (!s) as i64).collect(),
                vec!["struck".into(), "elsewhere".into()],
            ),
        );
    }
    let runs = comparison.baseline.len();
    for (title, groups, labels) in tables {
        println!("\nBy {title}:");
        println!(
            "  {:<34}{:>9}{:>11}{:>14}{:>10}{:>17}{:>21}",
            "", "Places", "People", "Extra deaths", "per 1,000", "Worst month", "Left"
        );
        for e in effect_by(comparison, &groups, &labels) {
            let per_1000 = 1000.0 * e.extra_deaths.mean / e.people.max(1.0);
            let deaths =
                if runs > 1 { format!("{:+.1}", e.extra_deaths.mean) } else { format!("{:+.0}", e.extra_deaths.mean) };
            let worst = format!("{} -> {}", pct(e.lowest_ration.0), pct(e.lowest_ration.1));
            let left = format!("{} -> {}", commas(e.left.0, 0), commas(e.left.1, 0));
            println!(
                "  {:<34}{:>9}{:>11}{:>14}{:>10.1}{:>17}{:>21}",
                e.label,
                e.villages,
                commas(e.people, 0),
                deaths,
                per_1000,
                worst,
                left
            );
        }
    }
    println!("  (worst month: the share of their need its places ate in their hungriest month, without -> with)");
}

pub fn print_outlook(runs: &[Run]) {
    let pop = series(runs, "population");
    let total =
        |metric: &str| series(runs, metric).iter().map(|v| v.iter().sum::<f64>()).sum::<f64>() / runs.len() as f64;
    let hungry = series(runs, "ration").iter().map(|v| v.iter().filter(|&&r| r < 0.97).count() as f64).sum::<f64>()
        / runs.len() as f64;
    let first = &runs[0].records[0];
    let start = first.population - first.births + first.deaths;
    let months = pop[0].len();
    let end = pop.iter().map(|v| v[months - 1]).sum::<f64>() / runs.len() as f64;
    let average = if runs.len() == 1 { String::new() } else { format!(" (average of {} runs)", runs.len()) };
    println!("\nAfter {months} months{average}:");
    println!("  Population {start} -> {end:.0}; births {:.0}, deaths {:.0}", total("births"), total("deaths"));
    println!("  Months the village ate under 97% of its food need: {hungry:.1}");
}

pub fn print_events(config: &Config) {
    println!("{:<14} {:<9} {:<8} {:>8}  {:<14} {:<8} Effects", "Event", "Scope", "Reach", "Chance", "When", "Lasts");
    for spec in &config.events {
        let when = match &spec.months {
            Some(m) => m.iter().map(|&m| MONTH_ABBR[m as usize]).collect::<Vec<_>>().join(", "),
            None => "any month".into(),
        };
        let lasts = match spec.duration {
            Duration::Fixed(d) => d.to_string(),
            Duration::Range(a, b) => format!("{a}-{b}"),
        };
        let mut effects = spec
            .effects
            .iter()
            .map(|(k, v)| match v {
                EffectValue::Fixed(x) => format!("{k} {x:?}"),
                EffectValue::Range(a, b) => format!("{k} ({a:?}, {b:?})"),
            })
            .collect::<Vec<_>>()
            .join(", ");
        if let Some(r) = spec.repeat_chance {
            effects += &format!("; {} the year after one", pct(r));
        }
        let reach = if spec.scope == "location" { spec.reach.as_str() } else { "" };
        println!(
            "{:<14} {:<9} {:<8} {:>8}  {:<14} {:<8} {effects}",
            spec.name,
            spec.scope,
            reach,
            format!("{:.1}%", 100.0 * spec.chance),
            when,
            format!("{lasts} mo")
        );
    }
    println!("\nChance: per eligible month, in the long run.");
    println!(
        "Reach, in a region (--config with a region): a village on its own, the weather (shared by neighbours), or the whole region."
    );
    println!("Force a village event with --event NAME@MONTH, e.g. --event drought@4");
}

/// The first run's chronicle of each arm: as text, from rank 1 down to
/// `most_rank`, and every entry as JSON.
pub fn chronicles(arms: &[(&str, &econ_core::scenarios::Run)], most_rank: u8) -> (String, String) {
    let mut text = String::from("What happened, year by year and month by month, the most important first. Ranks:\n");
    for rank in econ_core::chronicle::RANKS.iter().take(most_rank as usize) {
        text += &format!("  [{rank}\n").replacen(':', "]", 1);
    }
    let mut json = serde_json::Map::new();
    json.insert("ranks".into(), serde_json::json!(econ_core::chronicle::RANKS));
    for (title, run) in arms {
        text += &format!("\n{title} (seed {}):\n", run.config.seed);
        text += &run.chronicle.text(&run.records, most_rank);
        let key = if title.starts_with("With") { "with_forced_events" } else { "without_forced_events" };
        json.insert(key.into(), serde_json::json!(run.chronicle.entries));
    }
    (text, serde_json::Value::Object(json).to_string() + "\n")
}
