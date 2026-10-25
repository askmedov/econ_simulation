//! How fast is the engine at the sizes Phase 3 aims for? (The port of
//! `scripts/benchmark.py`, up to ten million people.)
//!
//! Each size is a region of villages of 1,000 people (one village below
//! that), without a map, as in Python. It times setting up the world and
//! simulating a few months on one thread, then projects the targets:
//!
//!   region    ~100,000 people: 100 paired three-year runs in under 10 minutes
//!   province  ~1,000,000 people: one paired three-year run in under a minute,
//!             20 of them in under 20 minutes; a month in under 0.2 s
//!   country   ~10,000,000 people: one paired three-year run in under 5 minutes
//!
//! Paired runs go on separate threads (`--workers`); the projections for
//! them come from timing that many runs at once, as they share the memory
//! bus.

use std::time::Instant;

use econ_core::config::{default_config, Config};
use econ_core::Simulation;

pub const MONTHS_RUN: f64 = 36.0; // a what-if's horizon

pub fn region(people: usize, months: i64) -> Config {
    let mut c = default_config();
    c.seed = 1;
    c.months = months;
    c.start_month = 6;
    let villages = (people / 1000).max(1);
    let each = people / villages;
    let base = c.villages[0].clone();
    c.villages = (0..villages)
        .map(|i| {
            let mut v = base.clone();
            v.name = format!("v{i}");
            v.population = each as i64;
            v.land = 0.35 * each as f64;
            v
        })
        .collect();
    c
}

/// Peak memory of this process so far, in MB.
pub fn peak_memory_mb() -> f64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    status
        .lines()
        .find(|l| l.starts_with("VmHWM:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|kb| kb.parse::<f64>().ok())
        .map_or(0.0, |kb| kb / 1024.0)
}

/// (seconds to set up, seconds a month), on one thread.
pub fn measure(people: usize, months: i64) -> Result<(f64, f64), String> {
    let start = Instant::now();
    let mut sim = Simulation::new(region(people, months))?;
    let setup = start.elapsed().as_secs_f64();
    let start = Instant::now();
    sim.run();
    Ok((setup, start.elapsed().as_secs_f64() / months as f64))
}

/// (seconds to set up, seconds a month) of `k` runs at once on `k` threads,
/// half of them with a drought (the arms of paired what-ifs): runs side by
/// side share the memory bus, so each goes slower than on its own.
pub fn measure_together(people: usize, months: i64, k: usize) -> Result<(f64, f64), String> {
    let configs: Vec<Config> = (0..k)
        .map(|j| {
            let mut c = region(people, months);
            if j % 2 == 1 {
                c.scheduled_events.push(econ_core::config::ScheduledEvent {
                    event: "drought".into(),
                    month: 2,
                    village: None,
                    near: None,
                });
            }
            c
        })
        .collect();
    let start = Instant::now();
    let built: Vec<Result<Simulation, String>> = std::thread::scope(|scope| {
        let handles: Vec<_> = configs.iter().map(|c| scope.spawn(|| Simulation::new(c.clone()))).collect();
        handles.into_iter().map(|h| h.join().expect("set-up thread")).collect()
    });
    let mut sims = built.into_iter().collect::<Result<Vec<_>, _>>()?;
    let setup = start.elapsed().as_secs_f64();
    let start = Instant::now();
    std::thread::scope(|scope| {
        for sim in sims.iter_mut() {
            scope.spawn(move || sim.run());
        }
    });
    Ok((setup, start.elapsed().as_secs_f64() / months as f64))
}

/// Wall-clock minutes for `pairs` paired runs of MONTHS_RUN months on
/// `workers` threads, from timings of `workers` runs at once.
pub fn paired_runs_minutes(setup: f64, per_month: f64, pairs: usize, workers: usize) -> f64 {
    let one = setup + MONTHS_RUN * per_month;
    let rounds = (2 * pairs).div_ceil(workers);
    rounds as f64 * one / 60.0
}

pub fn run(sizes: &[usize], months: i64, workers: usize) -> Result<bool, String> {
    println!("{:>12} {:>9} {:>8} {:>10} {:>12}", "People", "Villages", "Setup", "Per month", "Peak memory");
    let mut timings = Vec::new();
    for &people in sizes {
        let (setup, per_month) = measure(people, months)?;
        timings.push((people, setup, per_month));
        println!(
            "{:>12} {:>9} {:>7.2}s {:>9.4}s {:>9.0} MB",
            econ_core::geography::thousands(people as i64),
            econ_core::geography::thousands((people / 1000).max(1) as i64),
            setup,
            per_month,
            peak_memory_mb()
        );
    }
    // Paired runs side by side, as a what-if runs them.
    println!("\n{:>12} {:>9} {:>8} {:>10}", "People", "At once", "Setup", "Per month");
    let mut together = Vec::new();
    for &people in sizes.iter().filter(|&&p| p >= 100_000) {
        // (ten million people at a time on more than two threads would not fit in memory)
        let at_once: Vec<usize> = if people >= 10_000_000 { vec![2] } else { vec![2, workers] };
        for k in at_once.into_iter().filter(|&k| k >= 2) {
            let (setup, per_month) = measure_together(people, months, k)?;
            together.push((people, k, setup, per_month));
            println!(
                "{:>12} {:>9} {:>7.2}s {:>9.4}s",
                econ_core::geography::thousands(people as i64),
                k,
                setup,
                per_month
            );
        }
    }
    println!("\nTargets ({workers} threads, runs of {MONTHS_RUN} months):");
    let mut failed = false;
    let mut target = |name: &str, value: f64, limit: f64, unit: &str| {
        let ok = value <= limit;
        failed |= !ok;
        println!("  {} {name}: {value:.2} {unit} (target {limit})", if ok { "ok  " } else { "MISS" });
    };
    let alone = |people: usize| timings.iter().find(|t| t.0 == people).map(|&(_, s, m)| (s, m));
    let at_once =
        |people: usize, k: usize| together.iter().find(|t| t.0 == people && t.1 == k).map(|&(_, _, s, m)| (s, m));
    let many = workers.max(2);
    if let Some((s, m)) = at_once(100_000, many) {
        target("region, 100 paired runs of 100,000 people", paired_runs_minutes(s, m, 100, many), 10.0, "min");
    }
    if let Some((_, m)) = alone(1_000_000) {
        target("province, a month of 1,000,000 people on one thread", m, 0.2, "s");
    }
    if let Some((s, m)) = at_once(1_000_000, 2) {
        target("province, 1 paired run of 1,000,000", paired_runs_minutes(s, m, 1, 2), 1.0, "min");
    }
    if let Some((s, m)) = at_once(1_000_000, many) {
        target("province, 20 paired runs of 1,000,000", paired_runs_minutes(s, m, 20, many), 20.0, "min");
    }
    if let Some((s, m)) = at_once(10_000_000, 2) {
        target("country, 1 paired run of 10,000,000", paired_runs_minutes(s, m, 1, 2), 5.0, "min");
    }
    println!("\n(this machine has {} cores)", std::thread::available_parallelism().map_or(1, |n| n.get()));
    Ok(!failed)
}
