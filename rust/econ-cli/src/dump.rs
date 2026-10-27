//! Raw output for analysis and for the parity checks against Python
//! (`scripts/parity.py`): every month of every run, or the starting state.

use std::io::Write;

use econ_core::config::Config;
use econ_core::scenarios::{self, Run};
use econ_core::world::World;
use econ_core::Simulation;
use serde_json::{json, Value};

/// Every month of every run as CSV on standard output, numbers in full:
/// `arm` is "base" or "event" (the paired run with the forced events),
/// `seed` the run's seed.
pub fn records(config: &Config, forced: &[econ_core::config::ScheduledEvent], runs: usize) -> Result<(), String> {
    let arms: Vec<(&str, Vec<Run>)> = if forced.is_empty() {
        vec![("base", scenarios::run_batch(config, runs)?)]
    } else {
        let pairs = scenarios::compare(config, forced, runs)?;
        vec![("base", pairs.baseline), ("event", pairs.scenario)]
    };
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    let mut header = false;
    for (arm, runs) in &arms {
        for run in runs {
            for record in &run.records {
                let flat = record.flat();
                if !header {
                    let names: Vec<&str> = flat.iter().map(|(n, _)| n.as_str()).collect();
                    writeln!(out, "arm,seed,{}", names.join(",")).map_err(|e| e.to_string())?;
                    header = true;
                }
                let row: Vec<String> = flat.iter().map(|(_, v)| v.full()).collect();
                writeln!(out, "{arm},{},{}", run.config.seed, row.join(",")).map_err(|e| e.to_string())?;
            }
        }
    }
    out.flush().map_err(|e| e.to_string())
}

/// The starting state of each run (seeds config.seed, config.seed + 1, ...)
/// as one line of JSON each.
pub fn worlds(config: &Config, runs: usize) -> Result<(), String> {
    use rayon::prelude::*;
    let lines: Vec<Result<String, String>> = (0..runs)
        .into_par_iter()
        .map(|i| {
            let sim = Simulation::new(scenarios::with_seed(config, config.seed + i as i64))?;
            Ok(state(&sim.world, sim.config.seed).to_string())
        })
        .collect();
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    for line in lines {
        writeln!(out, "{}", line?).map_err(|e| e.to_string())?;
    }
    out.flush().map_err(|e| e.to_string())
}

fn rows(grid: &econ_core::grid::Grid) -> Value {
    Value::from((0..grid.rows).map(|r| grid.row(r).to_vec()).collect::<Vec<_>>())
}

/// The world's state as JSON, with Python's names.
pub fn state(world: &World, seed: i64) -> Value {
    json!({
        "seed": seed,
        "month": world.month,
        "population": world.population,
        "households": world.households,
        "land": world.land,
        "stock": rows(&world.stock),
        "prices": rows(&world.prices),
        "markup": rows(&world.markup),
        "wage_level": world.wage_level,
        "cash": rows(&world.cash),
        "tools": rows(&world.tools),
        "pay": rows(&world.pay),
        "orders": rows(&world.orders),
        "supplies": world.supplies.data,
        "council": {
            "formed": world.council.formed,
            "months_ready": world.council.months_ready,
            "treasury": world.council.treasury,
            "reserve": world.council.reserve,
        },
        "lord": {"land": world.lord.land, "barn": world.lord.barn, "purse": world.lord.purse},
        "town_level": world.town.level,
        "woods": world.woods,
        "woods_capacity": world.woods_capacity,
        "woods_normal": world.woods_normal,
        "soil": world.soil,
        "dealers_grain": world.dealers_grain,
        "dealers_cash": world.dealers_cash,
        "arrivals": world.arrivals,
        "sends": world.sends,
        "unhealthy": world.unhealthy,
        "skill": rows(&world.skill),
    })
}

/// Each run's measures per place (`simulation::PLACE_SERIES`, months x
/// places), one line of JSON per run: `arm` is "base" or "event".
pub fn places(config: &Config, forced: &[econ_core::config::ScheduledEvent], runs: usize) -> Result<(), String> {
    let arms: Vec<(&str, Vec<Run>)> = if forced.is_empty() {
        vec![("base", scenarios::run_batch(config, runs)?)]
    } else {
        let pairs = scenarios::compare(config, forced, runs)?;
        vec![("base", pairs.baseline), ("event", pairs.scenario)]
    };
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    for (arm, runs) in &arms {
        for run in runs {
            let series: serde_json::Map<String, Value> =
                run.places.iter().map(|(name, values)| (name.to_string(), json!(values))).collect();
            writeln!(out, "{}", json!({"arm": arm, "seed": run.config.seed, "places": series}))
                .map_err(|e| e.to_string())?;
        }
    }
    out.flush().map_err(|e| e.to_string())
}

/// Each run's chronicle (from rank 1 down to `most_rank`), run after run;
/// with `flat`, its plain list of happenings instead, as Python's.
pub fn logs(
    config: &Config,
    forced: &[econ_core::config::ScheduledEvent],
    runs: usize,
    most_rank: u8,
    flat: bool,
) -> Result<(), String> {
    let mut with = config.clone();
    with.scheduled_events.extend_from_slice(forced);
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    for run in scenarios::run_batch(&with, runs)? {
        writeln!(out, "Seed {}:", run.config.seed).map_err(|e| e.to_string())?;
        if flat {
            for line in &run.log {
                writeln!(out, "  {line}").map_err(|e| e.to_string())?;
            }
        } else {
            write!(out, "{}", run.chronicle.text(&run.records, most_rank)).map_err(|e| e.to_string())?;
        }
    }
    out.flush().map_err(|e| e.to_string())
}
