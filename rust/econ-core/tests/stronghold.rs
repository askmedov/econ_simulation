//! The region's stronghold: the state pays its garrison and officials there
//! from its tax, and that pay is what keeps a city (a market town lives on
//! its countryside).

use econ_core::config::{configs_dir, Config};
use econ_core::Simulation;

fn region(name: &str) -> Config {
    Config::from_file(configs_dir().join("regions").join(format!("{name}.json"))).unwrap()
}

fn stronghold(c: &Config) -> usize {
    c.villages.iter().position(|v| v.stronghold).expect("a stronghold")
}

#[test]
fn the_state_pays_its_garrison_in_the_stronghold_from_the_last_years_tax() {
    let mut c = region("city3");
    c.start_month = 1;
    let city = stronghold(&c);
    let mut sim = Simulation::new(c.clone()).unwrap();
    let money = sim.world.money();
    sim.step();
    // Before the first tax the pay is reckoned from what the tax would
    // bring, and the state brings it from its purse.
    let pay = sim.world.lord.garrison_pay.unwrap();
    assert!(pay > 0.0);
    assert!((sim.world.lord.state_purse[city] + pay).abs() <= 1e-9 * pay);
    let mut collected = 0.0;
    while sim.world.month_of_year() != c.state.collection_month + 1 {
        collected += sim.step().state_tax;
    }
    assert!(collected > 0.0);
    let pay = sim.world.lord.garrison_pay.unwrap();
    assert!((pay - c.state.garrison * collected / 12.0).abs() <= 1e-9 * pay);
    assert!((sim.world.money() - money).abs() <= 1e-9 * money, "money is conserved");
}

#[test]
fn without_a_garrison_the_whole_tax_leaves_the_region() {
    let mut c = region("city3");
    c.start_month = 1;
    c.state.garrison = 0.0;
    let mut sim = Simulation::new(c).unwrap();
    let mut collected = 0.0;
    for _ in 0..12 {
        collected += sim.step().state_tax;
    }
    assert!(sim.world.lord.garrison_pay.is_none());
    let purse: f64 = sim.world.lord.state_purse.iter().sum();
    assert!(collected > 0.0 && (purse - collected).abs() <= 1e-9 * collected);
}

#[test]
fn a_stronghold_keeps_more_people_with_its_garrison_paid_there() {
    let mut c = region("city3");
    c.months = 48;
    let city = stronghold(&c);
    let people_left = |c: Config| -> f64 {
        let mut sim = Simulation::new(c).unwrap();
        sim.run();
        let people = sim.place_series().into_iter().find(|(name, _)| *name == "population").unwrap().1;
        people.last().unwrap()[city]
    };
    for seed in 0..3 {
        let mut with = c.clone();
        with.seed = seed;
        let mut without = with.clone();
        without.state.garrison = 0.0;
        let (a, b) = (people_left(with), people_left(without));
        assert!(a > b, "seed {seed}: {a} people in the stronghold with its garrison, {b} without");
    }
}
