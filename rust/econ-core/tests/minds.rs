//! Families with minds of their own (`minds.rs`), next to the rules.

use econ_core::checks::broken_invariants;
use econ_core::config::{default_config, Config, MindsConfig, ScheduledEvent};
use econ_core::Simulation;

fn config(seed: i64, months: i64) -> Config {
    let mut c = default_config();
    c.seed = seed;
    c.months = months;
    c
}

fn region20() -> Config {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../configs/regions/region20.json");
    Config::from_file(path).expect("region20.json")
}

fn drought(month: i64) -> ScheduledEvent {
    ScheduledEvent { event: "drought".into(), month, village: None, near: None }
}

/// Minds on, with no spread of traits and no responses.
fn neutral(mut c: Config) -> Config {
    c.minds = MindsConfig {
        enabled: true,
        trait_spread: 0.0,
        inherited_spread: 0.0,
        speculation: 0.0,
        hunger_saving: 0.0,
        loss_fear: 0.0,
        hunger_leaving: 0.0,
        ..c.minds
    };
    c
}

fn with_minds(mut c: Config) -> Config {
    c.minds.enabled = true;
    c
}

/// Every family by the rules (minds off).
fn by_rules(mut c: Config) -> Config {
    c.minds.enabled = false;
    c
}

#[test]
fn families_have_minds_unless_switched_off() {
    assert!(default_config().minds.enabled);
    assert!(Simulation::new(config(1, 1)).unwrap().world.households.minds.is_some());
    assert!(Simulation::new(by_rules(config(1, 1))).unwrap().world.households.minds.is_none());
}

fn run(c: Config) -> Simulation {
    let mut sim = Simulation::new(c).unwrap();
    sim.run();
    sim
}

#[test]
fn minds_with_no_spread_and_no_responses_decide_exactly_as_the_rules() {
    let mut village = config(3, 48);
    village.scheduled_events = vec![drought(4)];
    let mut region = region20();
    region.seed = 5;
    region.months = 24;
    region.scheduled_events = vec![drought(4)];
    for c in [village, region] {
        let rules = run(by_rules(c.clone()));
        let minds = run(neutral(c));
        assert_eq!(rules.records, minds.records);
        assert_eq!(rules.log, minds.log);
    }
}

#[test]
fn minds_change_what_happens() {
    let mut c = config(3, 36);
    c.scheduled_events = vec![drought(4)];
    assert_ne!(run(by_rules(c.clone())).records, run(with_minds(c)).records);
}

#[test]
fn invariants_hold_every_month_with_minds() {
    let mut drought_village = config(1, 60);
    drought_village.scheduled_events = vec![drought(4), drought(16)];
    let mut region = region20();
    region.months = 36;
    let mut hamlet = config(2, 72);
    hamlet.villages[0].population = 20;
    hamlet.villages[0].land = 7.0;
    for (name, setting) in [("droughts", drought_village), ("region of 20", region), ("hamlet", hamlet)] {
        for seed in 0..2 {
            let mut c = with_minds(setting.clone());
            c.seed = seed;
            let mut sim = Simulation::new(c).unwrap();
            let money = sim.world.money();
            for _ in 0..sim.config.months {
                sim.step();
                let broken = broken_invariants(&sim.world);
                assert!(broken.is_empty(), "{name}, seed {seed}, month {}: {broken:?}", sim.world.month);
                assert!((sim.world.money() - money).abs() <= 1e-9 * money, "{name}, seed {seed}: money not conserved");
            }
        }
    }
}

#[test]
fn families_come_to_expect_dearer_grain_in_a_drought_and_remember_its_hunger() {
    let mut rising = 0;
    for seed in 0..10 {
        let mut c = with_minds(config(seed, 7));
        c.scheduled_events = vec![drought(4)];
        let sim = run(c);
        let minds = sim.world.households.minds.as_ref().unwrap();
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        rising += (mean(&minds.trend) > 0.0) as usize;
        assert!(minds.hunger.iter().all(|&h| (0.0..=1.0).contains(&h)));
    }
    assert!(rising >= 8, "{rising} of 10 drought runs expected dearer grain");
}
