//! Whole runs: reproducible, every ration and every person accounted for,
//! the invariants kept every month (Python's tests/test_simulation.py and
//! tests/test_invariants.py).

use econ_core::checks::broken_invariants;
use econ_core::config::{default_config, Config, ScheduledEvent};
use econ_core::population::Population;
use econ_core::rng::RandomStreams;
use econ_core::world::create_world;
use econ_core::Simulation;

fn config(seed: i64, months: i64) -> Config {
    let mut c = default_config();
    c.seed = seed;
    c.months = months;
    c
}

fn run(config: Config) -> Simulation {
    let mut sim = Simulation::new(config).unwrap();
    sim.run();
    sim
}

fn village(population: i64, land: f64) -> econ_core::config::VillageConfig {
    let mut v = default_config().villages[0].clone();
    v.population = population;
    v.land = land;
    v
}

#[test]
fn same_seed_gives_same_history() {
    let a = run(config(1, 240));
    let b = run(config(1, 240));
    assert_eq!(a.records, b.records);
    assert_eq!(a.log, b.log);
}

#[test]
fn different_seeds_give_different_histories() {
    let a = run(config(1, 240));
    let b = run(config(2, 240));
    let pa: Vec<i64> = a.records.iter().map(|r| r.population).collect();
    let pb: Vec<i64> = b.records.iter().map(|r| r.population).collect();
    assert_ne!(pa, pb);
}

#[test]
fn food_is_accounted_for() {
    let mut sim = Simulation::new(config(3, 240)).unwrap();
    let mut stock: f64 = sim.world.granary().iter().sum();
    for _ in 0..240 {
        let r = sim.step();
        // Families eat what they buy plus relief from the council's reserve
        // and the lord's barn.
        stock += r.food_produced + r.meat
            - (r.food_eaten - r.relief - r.lord_relief - r.foraged - r.lord_sold - r.grain_imported)
            - r.food_spoiled
            - r.food_lost
            - r.food_levied
            - r.food_to_seed
            - r.food_rent
            - r.tax_grain
            - r.food_requisitioned
            - r.grain_exported
            - r.food_emigrated;
        assert!((stock - r.food_stock).abs() <= 1e-6 * r.food_stock.abs().max(1.0), "month {}", r.month_number);
    }
}

#[test]
fn population_matches_births_and_deaths() {
    let mut sim = Simulation::new(config(4, 240)).unwrap();
    let mut people = sim.world.population.size() as i64;
    for _ in 0..240 {
        let r = sim.step();
        people += r.births - r.deaths - r.emigrants + r.immigrants;
        assert_eq!(people, r.population);
        assert_eq!(r.population, r.children + r.workers + r.elderly);
    }
}

#[test]
fn village_survives_a_century_with_default_settings() {
    // Long runs are a sanity check: no runaway growth, no collapse.
    for seed in 0..5 {
        let sim = run(config(seed, 1200));
        let end = sim.records.last().unwrap().population;
        assert!(300 < end && end < 4000, "seed {seed}: {end}");
    }
}

#[test]
fn droughts_happen_and_villages_go_hungry() {
    let sim = run(config(0, 1200));
    assert!(sim.records.iter().any(|r| r.events.contains("drought")));
    assert!(sim.records.iter().any(|r| r.ration < 1.0));
}

#[test]
fn villages_are_independent_without_trade() {
    let mut c = config(5, 120);
    let mut b = village(1000, 10.0);
    b.name = "B".into();
    let mut a = village(1000, 350.0);
    a.name = "A".into();
    c.villages = vec![a, b];
    let sim = run(c);
    let mut places: Vec<u32> = sim.world.population.location.clone();
    places.sort();
    places.dedup();
    assert_eq!(places, vec![0, 1]);
}

#[test]
fn grouped_population_runs_and_keeps_totals_consistent() {
    // The same rules must work when each row stands for many people.
    // (one household of 10,000 people: no migration, or it would flee as one)
    let mut c = config(6, 120);
    c.villages = vec![village(10_000, 5_000.0)];
    c.migration.enabled = false;
    let mut sim = Simulation::new(c).unwrap();
    let pop = &sim.world.population;
    let grouped = Population {
        count: vec![100; 100],
        age_months: pop.age_months[..100].to_vec(),
        female: pop.female[..100].to_vec(),
        health: pop.health[..100].to_vec(),
        skill: pop.skill[..100].to_vec(),
        location: pop.location[..100].to_vec(),
        household: vec![0; 100],
        job: vec![econ_core::population::NO_JOB; 100],
        married: (0..100).map(|i| pop.age_years(i) >= 18).collect(),
        couple: vec![-1; 100],
    };
    sim.world.population = grouped;
    let mut people = sim.world.population.size() as i64;
    for _ in 0..120 {
        let r = sim.step();
        people += r.births - r.deaths - r.emigrants + r.immigrants;
        assert_eq!(people, r.population);
        assert_eq!(r.population, sim.world.population.size() as i64);
    }
    assert!(sim.world.population.count.iter().max().unwrap() > &1);
}

#[test]
fn an_empty_village_keeps_running_quietly() {
    let mut c = config(0, 120);
    let mut v = village(10, 0.0);
    v.initial_food_months = 0.0;
    c.villages = vec![v];
    let sim = run(c);
    assert!(sim.extinct() && sim.records.len() == 120);
    assert_eq!(sim.records.last().unwrap().population, 0);
    assert_eq!(sim.log.iter().filter(|l| l.contains("last villager")).count(), 1);
}

#[test]
fn csv_has_one_row_per_month() {
    let sim = run(config(0, 24));
    let path = std::env::temp_dir().join(format!("econ-months-{}.csv", std::process::id()));
    econ_core::metrics::write_csv(&sim.records, &path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(&path).ok();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 25);
    assert!(lines[0].starts_with("month_number,year,month,population"));
}

#[test]
fn starting_village_is_balanced_between_women_and_men() {
    let sim = Simulation::new(config(0, 36)).unwrap();
    let pop = &sim.world.population;
    let women: u64 = (0..pop.len()).filter(|&i| pop.female[i]).map(|i| pop.count[i] as u64).sum();
    assert_eq!(women, pop.size() / 2);
    let adults: Vec<usize> = (0..pop.len()).filter(|&i| pop.age_years(i) >= 15).collect();
    let adult_women = adults.iter().filter(|&&i| pop.female[i]).count() as f64;
    assert!((adult_women - adults.len() as f64 / 2.0).abs() <= 1.0);
}

#[test]
fn village_recovers_after_a_drought_ends() {
    // Python pins this to one seed (64); a hamlet of 100 can have a ruinous
    // run of years, so here: most runs eat well again in their fifth year
    // (Python: 82% of 200 runs above 80% in their worst month of it).
    let recovered = (0..40)
        .filter(|&seed| {
            let mut c = config(seed, 60);
            c.villages = vec![village(100, 35.0)];
            let sim = run(c);
            sim.records[sim.records.len() - 12..].iter().all(|r| r.ration > 0.8)
        })
        .count();
    assert!(recovered >= 28, "{recovered} of 40");
}

fn disasters() -> Vec<ScheduledEvent> {
    [
        ("drought", 4),
        ("plague", 9),
        ("army", 14),
        ("drought", 16),
        ("debasement", 20),
        ("debt_jubilee", 30),
        ("raid", 33),
    ]
    .iter()
    .map(|&(event, month)| ScheduledEvent { event: event.into(), month, village: None, near: None })
    .collect()
}

fn invariant_settings() -> Vec<(&'static str, Config)> {
    let base = config(0, 72);
    let mut five = base.clone();
    five.villages = (0..5)
        .map(|i| {
            let mut v = village(200 + 150 * i, 80.0 + 50.0 * i as f64);
            v.name = format!("v{i}");
            v
        })
        .collect();
    let mut no_council = base.clone();
    no_council.council.enabled = false;
    let mut disaster = config(0, 48);
    disaster.scheduled_events = disasters();
    let mut heirs = base.clone();
    heirs.land.partible = false;
    let mut hamlet = base.clone();
    hamlet.villages = vec![village(20, 7.0)];
    vec![
        ("default", base),
        ("five villages", five),
        ("no council", no_council),
        ("one disaster after another", disaster),
        ("heirs keep the land", heirs),
        ("hamlet", hamlet),
    ]
}

#[test]
fn invariants_hold_every_month() {
    for (name, setting) in invariant_settings() {
        for seed in 0..2 {
            let mut c = setting.clone();
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
fn the_checks_catch_broken_state() {
    fn broken(change: impl Fn(&mut econ_core::world::World)) -> Vec<String> {
        let c = config(1, 36);
        let mut world = create_world(&c, &mut RandomStreams::new(1)).unwrap();
        assert!(broken_invariants(&world).is_empty());
        change(&mut world);
        broken_invariants(&world)
    }
    assert_eq!(broken(|w| w.households.land[0] += 1.0), vec!["land not conserved"]);
    assert_eq!(broken(|w| w.households.debt[0] = 5.0), vec!["debts and claims don't balance"]);
    assert_eq!(broken(|w| w.households.grain[0] = -1.0), vec!["negative or not-a-number grain"]);
    assert_eq!(broken(|w| w.population.location[0] = 1), vec!["people living outside their household's village"]);
    assert_eq!(
        broken(|w| {
            for i in 0..w.population.len() {
                if w.population.couple[i] >= 0 {
                    w.population.married[i] = false;
                }
            }
        }),
        vec!["unmarried people in a couple"]
    );
    assert_eq!(
        broken(|w| w.households.kin[0] = w.households.len() as i32),
        vec!["kin links to no family or another village"]
    );
}
