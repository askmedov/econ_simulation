//! A run or a paired what-if, as Python's `python -m econ_sim`: print the
//! months and the effect, and write the same files (run1.csv,
//! run1_with_event.csv, summary.csv, places.csv, log.txt).

use std::path::Path;

use econ_core::geography;
use econ_core::metrics::write_csv;
use econ_core::scenarios::{about_number, compare, effect, run_batch, write_places_csv, write_summary_csv, Comparison};
use econ_core::Simulation;

use crate::options::RunArgs;
use crate::report;

pub fn run(args: &RunArgs) -> Result<(), String> {
    let config = args.config()?;
    if args.list_events {
        report::print_events(&config);
        return Ok(());
    }
    let forced = args.forced()?;
    // Check the forced events before running anything.
    let mut check = config.clone();
    check.months = 0;
    check.scheduled_events.extend_from_slice(&forced);
    Simulation::new(check)?;

    report::print_header(&config, &forced, args.runs);
    let (baseline, scenario) = if forced.is_empty() {
        (run_batch(&config, args.runs)?, None)
    } else {
        let pairs = compare(&config, &forced, args.runs)?;
        (pairs.baseline, Some(pairs.scenario))
    };
    match &scenario {
        None => report::print_months(&baseline),
        Some(s) => report::print_comparison(&baseline, s),
    }
    let shown = scenario.as_ref().unwrap_or(&baseline);
    if args.runs == 1 && !shown[0].log.is_empty() {
        println!("\nWhat happened:");
        for line in &shown[0].log {
            println!("  {line}");
        }
    }
    match &scenario {
        None => report::print_outlook(&baseline),
        Some(s) => {
            let comparison = Comparison { baseline: baseline.clone(), scenario: s.clone() };
            report::print_effect(&effect(&comparison), &forced, args.runs, config.months);
            if config.map.enabled {
                report::print_places(&comparison, &config, &forced);
            }
        }
    }

    let out = Path::new(&args.out);
    let io = |e: std::io::Error| format!("writing to {}: {e}", out.display());
    write_csv(&baseline[0].records, &out.join("run1.csv")).map_err(io)?;
    let mut log = vec!["Without forced events:".to_string()];
    log.extend(baseline[0].log.iter().cloned());
    let mut written = vec!["run1.csv", "summary.csv", "log.txt"];
    if let Some(s) = &scenario {
        write_csv(&s[0].records, &out.join("run1_with_event.csv")).map_err(io)?;
        log.push(String::new());
        log.push("With forced events:".into());
        log.extend(s[0].log.iter().cloned());
        written.insert(1, "run1_with_event.csv");
    }
    write_summary_csv(&out.join("summary.csv"), &baseline, scenario.as_deref()).map_err(io)?;
    std::fs::write(out.join("log.txt"), log.join("\n") + "\n").map_err(io)?;
    if config.map.enabled {
        let geo = geography::build(&config)?;
        let names: Vec<String> = config.villages.iter().map(|v| v.name.clone()).collect();
        let about = vec![
            ("kind".to_string(), config.villages.iter().map(|v| v.kind.clone()).collect()),
            ("terrain".to_string(), config.villages.iter().map(|v| v.terrain.clone()).collect()),
            ("x_km".to_string(), geo.x.iter().map(|&x| about_number(x)).collect()),
            ("y_km".to_string(), geo.y.iter().map(|&y| about_number(y)).collect()),
            ("carriage".to_string(), geo.transport.iter().map(|&t| about_number(t)).collect()),
            ("population_start".to_string(), config.villages.iter().map(|v| v.population.to_string()).collect()),
        ];
        write_places_csv(&out.join("places.csv"), &names, &about, &baseline, scenario.as_deref()).map_err(io)?;
        written.push("places.csv");
    }
    println!("\nWrote {} to {}/", written.join(", "), args.out);
    Ok(())
}
