//! Starting points: the state a run starts from (Python's `starts.py`).

use econ_core::checks::broken_invariants;
use econ_core::config::{configs_dir, default_config, Config};
use econ_core::rng::RandomStreams;
use econ_core::starts::{self, apply};
use econ_core::world::create_world;
use econ_core::Simulation;

fn region(name: &str) -> Config {
    Config::from_file(configs_dir().join("regions").join(format!("{name}.json"))).unwrap()
}

#[test]
fn a_start_in_debt_has_the_landholders_owe_the_biggest_of_them() {
    let mut c = region("city3");
    c.credit.initial_debt_months = 18.0;
    let world = create_world(&c, &mut RandomStreams::new(1)).unwrap();
    let hh = &world.households;
    assert!(broken_invariants(&world).is_empty(), "debts and claims balance in every place");
    let owed: f64 = hh.debt.iter().sum();
    assert!(owed > 0.0);
    for v in 0..world.n_locations() {
        let here: Vec<usize> = (0..hh.len()).filter(|&h| hh.location[h] as usize == v && hh.land[h] > 0.0).collect();
        let smallest_lender =
            here.iter().filter(|&&h| hh.lent[h] > 0.0).map(|&h| hh.land[h]).fold(f64::INFINITY, f64::min);
        for &h in here.iter().filter(|&&h| hh.debt[h] > 0.0) {
            assert!(hh.land[h] <= smallest_lender, "a debtor holds no more land than any lender");
            assert_eq!(hh.lent[h], 0.0);
        }
    }
    let landless_owing = (0..hh.len()).filter(|&h| hh.land[h] == 0.0 && hh.debt[h] > 0.0).count();
    assert_eq!(landless_owing, 0);
}

#[test]
fn herds_soil_and_woods_start_as_the_starting_point_sets_them() {
    let c = region("city3");
    let mut worn = c.clone();
    worn.livestock.initial_herds = 0.5;
    worn.environment.initial_soil = 0.75;
    worn.environment.initial_woods = 0.5;
    let a = create_world(&c, &mut RandomStreams::new(1)).unwrap();
    let b = create_world(&worn, &mut RandomStreams::new(1)).unwrap();
    let herds = |w: &econ_core::world::World| w.households.animals.iter().sum::<f64>();
    assert!((herds(&b) - 0.5 * herds(&a)).abs() < 1e-9 * herds(&a));
    assert!(b.soil.iter().all(|&s| s == 0.75) && a.soil.iter().all(|&s| s == 1.0));
    for v in 0..a.n_locations() {
        assert!((b.woods[v] - 0.5 * a.woods[v]).abs() <= 1e-9 * a.woods[v].max(1.0));
        assert_eq!(a.woods_normal[v], b.woods_normal[v], "the woods' balance is the usual one");
    }
}

#[test]
fn invariants_hold_every_month_from_every_starting_point() {
    for start in starts::starts() {
        for base in [region("city3"), default_config()] {
            let mut c = apply(&base, &start).unwrap();
            c.months = 14;
            let mut sim = Simulation::new(c).unwrap();
            let money = sim.world.money();
            for _ in 0..sim.config.months {
                sim.step();
                let broken = broken_invariants(&sim.world);
                assert!(broken.is_empty(), "{}, month {}: {broken:?}", start.name, sim.world.month);
            }
            assert!((sim.world.money() - money).abs() <= 1e-9 * money, "{}: money is conserved", start.name);
        }
    }
}

#[test]
fn a_region_after_a_plague_has_land_to_spare_and_pays_better_than_a_crowded_one() {
    let base = region("region100");
    let mut fed = Vec::new();
    for name in ["after-plague", "crowded"] {
        let mut c = apply(&base, &starts::find(name).unwrap()).unwrap();
        c.months = 24;
        let mut sim = Simulation::new(c).unwrap();
        sim.run();
        let last: Vec<_> = sim.records.iter().rev().take(12).collect();
        let pay = last.iter().map(|m| m.wage_cover).sum::<f64>() / 12.0;
        let hungry = sim.records.iter().filter(|m| m.ration < 0.97).count();
        fed.push((pay, hungry));
    }
    assert!(fed[0].0 > 1.5 * fed[1].0, "pay covers {:.2} after a plague, {:.2} when crowded", fed[0].0, fed[1].0);
    assert!(fed[0].1 < fed[1].1, "{} hungry months after a plague, {} when crowded", fed[0].1, fed[1].1);
}
