//! What-ifs: forced events, paired runs, batches and their summaries
//! (Python's tests/test_scenarios.py).

use econ_core::config::{default_config, Config, ScheduledEvent, VillageConfig};
use econ_core::scenarios::{compare, effect, run_batch, series, spread, write_summary_csv};
use econ_core::Simulation;

/// A village near the limit of what its land can feed, where shocks bite.
fn crowded() -> Config {
    let mut c = default_config();
    c.seed = 10;
    c.months = 36;
    c.villages = vec![village("Village", 180, 50.0)];
    c
}

fn village(name: &str, population: i64, land: f64) -> VillageConfig {
    let mut v = default_config().villages[0].clone();
    v.name = name.into();
    v.population = population;
    v.land = land;
    v
}

fn event(name: &str, month: i64) -> ScheduledEvent {
    ScheduledEvent { event: name.into(), month, village: None, near: None }
}

#[test]
fn scheduled_event_starts_in_its_month_and_village() {
    let mut c = crowded();
    c.villages = vec![village("A", 1000, 350.0), village("B", 1000, 350.0)];
    c.random_events = false;
    c.scheduled_events = vec![ScheduledEvent { event: "disease".into(), month: 5, village: Some(1), near: None }];
    let mut sim = Simulation::new(c).unwrap();
    sim.run();
    let events: Vec<&str> = sim.records[3..5].iter().map(|r| r.events.as_str()).collect();
    assert_eq!(events, vec!["", "B:disease"]);
    assert!(sim.log[0].contains("B: Disease outbreak (scheduled)"), "{}", sim.log[0]);
}

#[test]
fn scheduled_event_ignores_its_usual_season() {
    // Droughts normally start only in April; forced, one starts in month 1 (January).
    let mut c = crowded();
    c.random_events = false;
    c.scheduled_events = vec![event("drought", 1)];
    let mut sim = Simulation::new(c).unwrap();
    sim.step();
    assert_eq!(sim.records[0].events, "drought");
}

#[test]
fn unknown_scheduled_event_is_rejected() {
    let mut c = crowded();
    c.scheduled_events = vec![event("meteor", 1)];
    let message = Simulation::new(c).err().expect("an error");
    assert!(message.contains("unknown village event"), "{message}");
}

#[test]
fn random_events_can_be_switched_off() {
    let mut c = crowded();
    c.months = 600;
    c.random_events = false;
    let mut sim = Simulation::new(c).unwrap();
    sim.run();
    assert!(sim.records.iter().all(|r| r.events.is_empty() && r.accidents == 0));
}

#[test]
fn start_month_sets_the_calendar() {
    let mut c = crowded();
    c.start_month = 10;
    let mut sim = Simulation::new(c).unwrap();
    for _ in 0..4 {
        sim.step();
    }
    let calendar: Vec<(i64, i64, i64)> = sim.records.iter().map(|r| (r.month_number, r.month, r.year)).collect();
    assert_eq!(calendar, vec![(1, 10, 1), (2, 11, 1), (3, 12, 1), (4, 1, 2)]);
}

#[test]
fn paired_runs_match_until_the_event_starts() {
    let result = compare(&crowded(), &[event("disease", 30)], 3).unwrap();
    for (base, with) in result.baseline.iter().zip(&result.scenario) {
        assert_eq!(base.records[..29], with.records[..29]);
    }
}

#[test]
fn drought_lowers_food_stock_in_every_run() {
    let result = compare(&crowded(), &[event("drought", 4)], 10).unwrap();
    let gap = result.difference("food_stock");
    // Before the drought nothing differs; after the harvest every run has less
    // food, except runs whose baseline had a drought that April anyway.
    for (run, base) in gap.iter().zip(&result.baseline) {
        assert!(run[..3].iter().all(|&g| g == 0.0));
        if base.records[3].events.contains("drought") {
            assert_eq!(run[9], 0.0);
        } else {
            assert!(run[9] < 0.0);
        }
    }
}

#[test]
fn drought_effect_is_harmful_on_average() {
    let result = effect(&compare(&crowded(), &[event("drought", 4)], 20).unwrap());
    assert!(result.extra_deaths.mean > 0.0);
    assert!(result.lowest_ration.1 < result.lowest_ration.0);
    assert!(result.hungry_months.1 > result.hungry_months.0);
    assert!(
        result.extra_deaths.low <= result.extra_deaths.mean && result.extra_deaths.mean <= result.extra_deaths.high
    );
}

#[test]
fn batch_uses_consecutive_seeds() {
    let mut c = crowded();
    c.months = 12;
    let runs = run_batch(&c, 3).unwrap();
    assert_eq!(runs.iter().map(|r| r.config.seed).collect::<Vec<_>>(), vec![10, 11, 12]);
    let pop = series(&runs, "population");
    assert_eq!((pop.len(), pop[0].len()), (3, 12));
}

#[test]
fn spread_reports_mean_and_middle_80_percent() {
    let values: Vec<f64> = (0..=100).map(|v| v as f64).collect();
    let s = spread(&values);
    assert_eq!((s.mean, s.low, s.high), (50.0, 10.0, 90.0));
}

#[test]
fn summary_csv_has_baseline_event_and_difference_columns() {
    let mut c = crowded();
    c.months = 6;
    let result = compare(&c, &[event("drought", 4)], 2).unwrap();
    let path = std::env::temp_dir().join(format!("econ-summary-{}.csv", std::process::id()));
    write_summary_csv(&path, &result.baseline, Some(&result.scenario)).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(&path).ok();
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().unwrap().split(',').collect();
    assert_eq!(lines.count(), 6);
    for column in ["deaths", "deaths_event", "deaths_diff_low", "ration_event_high"] {
        assert!(header.contains(&column), "{column}");
    }
}

#[test]
fn runs_on_several_threads_match_runs_on_one() {
    let mut c = default_config();
    c.seed = 3;
    c.months = 12;
    let pool = |threads| rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap();
    let one = pool(1).install(|| run_batch(&c, 4).unwrap());
    let many = pool(4).install(|| run_batch(&c, 4).unwrap());
    for (a, b) in one.iter().zip(&many) {
        assert_eq!(a.records, b.records);
        assert_eq!(a.log, b.log);
    }
}
