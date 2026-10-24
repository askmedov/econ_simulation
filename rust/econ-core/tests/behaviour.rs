//! What whole runs show: markets, councils, healers, debts, migration,
//! woods, herds and coins behave as in Python's tests (test_market.py,
//! test_council.py, test_healthcare.py, test_credit.py, test_migration.py,
//! test_environment.py, test_livestock.py, test_farms.py, test_town.py).
//! Where a Python test leans on one seed's luck, the Rust one asks the same
//! of an average over seeds.

use econ_core::checks::broken_invariants;
use econ_core::config::{default_config, Config, EffectValue, ScheduledEvent, VillageConfig};
use econ_core::market::seasonal_buffer;
use econ_core::scenarios::{compare, effect, series};
use econ_core::Simulation;

fn config(seed: i64, months: i64) -> Config {
    let mut c = default_config();
    c.seed = seed;
    c.months = months;
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

fn run(c: Config) -> Simulation {
    let mut sim = Simulation::new(c).unwrap();
    sim.run();
    sim
}

fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}

fn lowest(v: &[f64]) -> f64 {
    v.iter().cloned().fold(f64::INFINITY, f64::min)
}

#[test]
fn drought_raises_food_price_and_hits_the_poorest_hardest() {
    // Without a council's relief, the market alone decides who eats.
    let mut c = config(0, 24);
    c.council.enabled = false;
    let result = compare(&c, &[event("drought", 4)], 8).unwrap();
    let peak = |runs| {
        series(runs, "food_price").iter().map(|v| v.iter().cloned().fold(f64::MIN, f64::max)).collect::<Vec<_>>()
    };
    let (base, with) = (peak(&result.baseline), peak(&result.scenario));
    let gaps: Vec<f64> = with.iter().zip(&base).map(|(w, b)| w - b).collect();
    // (a run whose baseline also has a bad year can hit the same ceiling)
    assert!(gaps.iter().all(|&g| g >= -1e-9));
    assert!(gaps.iter().filter(|&&g| g > 0.0).count() as f64 / gaps.len() as f64 >= 0.6);
    let poorest: Vec<f64> = series(&result.scenario, "poorest_fifth_ration").iter().map(|v| lowest(v)).collect();
    let all: Vec<f64> = series(&result.scenario, "ration").iter().map(|v| lowest(v)).collect();
    assert!(mean(&poorest) < mean(&all));
}

#[test]
fn firewood_is_stocked_up_before_winter() {
    assert!(seasonal_buffer(&[1.0; 12]).iter().all(|&b| b.abs() < 1e-12));
    let buffer = seasonal_buffer(&default_config().needs.firewood);
    let most = (0..12).fold(0, |best, m| if buffer[m] > buffer[best] { m } else { best });
    assert_eq!(most, 10); // most is needed going into November
    assert_eq!(buffer[3], 0.0); // by April the winter is over
}

#[test]
fn families_keep_warm_through_ordinary_winters() {
    let mut c = config(2, 60);
    c.random_events = false;
    let sim = run(c);
    // The poorest gather most of their fuel and go a little cold.
    assert!(sim.records.iter().all(|r| r.warmth > 0.85));
}

#[test]
fn customary_wage_follows_the_money_there_is() {
    let mut c = config(2, 120);
    c.money.initial_savings_months = 0.5;
    let mut sim = Simulation::new(c).unwrap();
    let start = sim.world.wage_level.clone();
    sim.run();
    assert!(sim.world.wage_level.iter().zip(&start).all(|(now, then)| now < then));
}

#[test]
fn money_is_never_created_or_destroyed() {
    let mut c = config(3, 36);
    c.villages = vec![village("A", 1000, 350.0), village("B", 300, 100.0)];
    c.scheduled_events = vec![event("drought", 4)];
    let mut sim = Simulation::new(c).unwrap();
    let total = sim.world.money();
    for _ in 0..36 {
        let r = sim.step();
        let coins =
            r.savings + r.business_cash + r.treasury + r.lord_purse + r.state_purse + r.town_purse + r.coins_lost;
        assert!((coins - total).abs() <= 1e-6 * total, "month {}", r.month_number);
    }
}

#[test]
fn council_relief_softens_a_drought() {
    let drought = [event("drought", 4)];
    let with_council = effect(&compare(&config(42, 24), &drought, 12).unwrap());
    let mut none = config(42, 24);
    none.council.enabled = false;
    let without = effect(&compare(&none, &drought, 12).unwrap());
    assert!(with_council.poorest_lowest_ration.1 > without.poorest_lowest_ration.1);
}

#[test]
fn a_growing_village_forms_a_council_and_a_small_one_never_does() {
    let mut c = config(42, 12);
    c.villages = vec![village("Village", 520, 182.0)];
    c.council.established_at_start = false;
    let sim = run(c);
    assert!(sim.log.iter().any(|l| l.contains("formed a council")));
    let last = sim.records.last().unwrap();
    assert!(last.councils == 1 && last.officials > 0);
    let mut small = config(42, 24);
    small.villages = vec![village("Village", 100, 35.0)];
    assert_eq!(run(small).records.last().unwrap().councils, 0);
}

#[test]
fn healthcare_blunts_an_epidemic() {
    let outbreak = [event("disease", 3)];
    let mut c = config(42, 12);
    c.random_events = false;
    let with_care = compare(&c, &outbreak, 16).unwrap();
    c.healthcare.enabled = false;
    let without = compare(&c, &outbreak, 16).unwrap();
    let deaths = |runs| series(runs, "deaths").iter().map(|v| v.iter().sum::<f64>()).sum::<f64>();
    assert!(deaths(&with_care.scenario) < deaths(&without.scenario));
}

#[test]
fn bad_years_bring_debt_and_land_changes_hands() {
    let mut c = config(1, 120);
    c.council.enabled = false;
    c.scheduled_events = vec![event("drought", 4), event("drought", 28)];
    let sim = run(c);
    let r = &sim.records;
    assert!(r.iter().map(|x| x.debt).fold(0.0, f64::max) > 0.0 && r.iter().map(|x| x.borrowed).sum::<f64>() > 0.0);
    assert!(r.iter().map(|x| x.land_sold + x.land_foreclosed).sum::<f64>() > 0.0);
}

#[test]
fn a_scheduled_jubilee_wipes_the_slate() {
    let mut c = config(1, 60);
    c.council.enabled = false;
    c.scheduled_events = vec![event("drought", 4), event("debt_jubilee", 48)];
    let sim = run(c);
    let r = &sim.records[47];
    assert!(r.debt == 0.0 || r.debts_cancelled > 0.0);
}

#[test]
fn moving_between_villages_keeps_the_books() {
    let mut c = config(4, 120);
    c.villages =
        [60, 150, 400].iter().enumerate().map(|(i, &p)| village(&format!("v{i}"), p, 0.35 * p as f64)).collect();
    c.scheduled_events = vec![ScheduledEvent { event: "plague".into(), month: 6, village: Some(0), near: None }];
    let mut sim = Simulation::new(c).unwrap();
    let money = sim.world.money();
    let mut people = sim.world.population.size() as i64;
    for _ in 0..120 {
        let r = sim.step();
        people += r.births - r.deaths - r.emigrants + r.immigrants;
        assert_eq!(people, r.population);
        assert!(broken_invariants(&sim.world).is_empty());
        assert!((sim.world.money() - money).abs() <= 1e-9 * money);
    }
    assert!(sim.records.iter().map(|r| r.moved_within).sum::<i64>() > 0);
}

#[test]
fn a_small_village_with_a_bad_economy_can_empty_out_but_a_big_one_does_not() {
    let died_out = |population: i64, seeds: i64| {
        (0..seeds)
            .filter(|&seed| {
                let mut c = config(seed, 360);
                c.lord.demesne = 0.4;
                c.lord.labour_service = 0.25;
                c.villages = vec![village("Village", population, 0.22 * population as f64)];
                run(c).records.last().unwrap().population == 0
            })
            .count()
    };
    assert!(died_out(80, 8) >= 1);
    assert_eq!(died_out(1000, 2), 0);
}

#[test]
fn no_migration_when_switched_off() {
    let mut c = config(1, 24);
    c.migration.enabled = false;
    let sim = run(c);
    assert_eq!(sim.records.iter().map(|r| r.emigrants + r.immigrants).sum::<i64>(), 0);
}

#[test]
fn a_forest_fire_burns_part_of_the_woods() {
    let mut c = config(0, 12);
    c.random_events = false;
    let base = run(c.clone());
    c.scheduled_events = vec![event("forest_fire", 6)];
    let fire = run(c);
    assert!(fire.records.iter().map(|r| r.woods_burned).sum::<f64>() > 0.0);
    assert_eq!(base.records.iter().map(|r| r.woods_burned).sum::<f64>(), 0.0);
    assert!(fire.records.last().unwrap().woods < base.records.last().unwrap().woods);
}

#[test]
fn a_severe_drought_cuts_next_years_sowing_and_a_mild_one_does_not() {
    let sown_after = |mult: f64| {
        let mut c = config(1, 12);
        c.random_events = false;
        for e in c.events.iter_mut().filter(|e| e.name == "drought") {
            e.effects = [("production_mult".to_string(), EffectValue::Fixed(mult))].into_iter().collect();
        }
        c.scheduled_events = vec![event("drought", 4)];
        run(c).world.sown[0]
    };
    assert_eq!(sown_after(0.8), 1.0);
    assert!(sown_after(0.2) < 0.9);
}

#[test]
fn animals_change_hands_in_a_famine() {
    let mut c = config(0, 36);
    c.scheduled_events = vec![event("drought", 4), event("drought", 16)];
    let sim = run(c);
    assert!(sim.records.iter().map(|r| r.animals_sold + r.meat).sum::<f64>() > 0.0);
}

#[test]
fn most_food_is_eaten_from_families_own_stores() {
    let sim = run(config(2, 36));
    let eaten: f64 = sim.records.iter().map(|r| r.food_eaten).sum();
    let own: f64 = sim.records.iter().map(|r| r.own_food).sum();
    assert!(0.6 < own / eaten && own / eaten < 0.95);
    assert!(sim.records.iter().map(|r| r.grain_sold).sum::<f64>() > 0.0);
}

#[test]
fn in_a_drought_the_landless_go_hungrier_than_the_village() {
    let (mut village_worst, mut landless_worst) = (Vec::new(), Vec::new());
    for seed in 0..8 {
        let mut c = config(seed, 24);
        c.council.enabled = false;
        c.scheduled_events = vec![event("drought", 4)];
        let sim = run(c);
        village_worst.push(lowest(&sim.records.iter().map(|r| r.ration).collect::<Vec<_>>()));
        landless_worst.push(lowest(&sim.records.iter().map(|r| r.landless_ration).collect::<Vec<_>>()));
    }
    assert!(mean(&landless_worst) < mean(&village_worst) - 0.05);
}

#[test]
fn a_debased_coinage_raises_prices_in_coins() {
    let mut c = config(1, 48);
    c.random_events = false;
    let before = run(c.clone());
    c.scheduled_events = vec![event("debasement", 3)];
    let after = run(c);
    let (b, a) = (before.records.last().unwrap(), after.records.last().unwrap());
    assert!((a.town_price - b.town_price / (1.0 - 0.25)).abs() < 1e-9 * a.town_price);
    let late = |s: &Simulation| mean(&s.records[36..48].iter().map(|r| r.food_price).collect::<Vec<_>>());
    assert!(late(&after) > late(&before));
}
