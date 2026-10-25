//! People on the move across a region (Phase 3, step 5): townsfolk out in
//! the villages' fields at harvest, and families fleeing a famine to the
//! towns.

use econ_core::checks::broken_invariants;
use econ_core::config::{configs_dir, default_config, Config, ScheduledEvent, VillageConfig};
use econ_core::rng::{RandomStreams, Rng};
use econ_core::world::create_world;
use econ_core::{economy, migration, work, Simulation};

fn region(name: &str) -> Config {
    Config::from_file(configs_dir().join("regions").join(format!("{name}.json"))).unwrap()
}

fn drought(month: i64) -> ScheduledEvent {
    ScheduledEvent { event: "drought".into(), month, village: None, near: None }
}

/// Villages at (x, y) and market towns (numbered after them) on a map,
/// every village joined to every town by road.
fn on_map(villages: &[(f64, f64)], towns: &[(f64, f64)]) -> Config {
    let mut c = default_config();
    let base = c.villages[0].clone();
    let mut places: Vec<VillageConfig> = villages
        .iter()
        .enumerate()
        .map(|(i, &(x, y))| VillageConfig { name: format!("v{i}"), population: 200, land: 70.0, x, y, ..base.clone() })
        .collect();
    places.extend(towns.iter().enumerate().map(|(j, &(x, y))| VillageConfig {
        name: format!("t{j}"),
        population: 300,
        land: 0.0,
        initial_food_months: 0.0,
        x,
        y,
        kind: "town".into(),
        ..base.clone()
    }));
    let n = villages.len();
    c.map.roads = (0..n).flat_map(|v| (0..towns.len()).map(move |t| (v, n + t))).collect();
    c.villages = places;
    c.map.enabled = true;
    c
}

#[test]
fn townsfolk_go_out_to_their_towns_villages_at_harvest_more_to_the_near_ones() {
    // Two villages of one town, 3 and 15 km from it.
    let mut c = on_map(&[(3.0, 0.0), (15.0, 0.0)], &[(0.0, 0.0)]);
    c.harvest_hands.enabled = true;
    let world = create_world(&c, &mut RandomStreams::new(1)).unwrap();
    let n = world.n_locations();
    let force = economy::workforce(&world.population, n, &c);
    let harvest = c.work.harvest_months[0];
    let help = work::harvest_help(&world.population, n, world.households.len(), harvest, &c, &force);
    let hands = work::harvest_hands(&help.households, &world.households, &world.land, &world.geo, &c);
    let in_town: f64 =
        (0..world.households.len()).filter(|&h| world.households.location[h] == 2).map(|h| help.households[h]).sum();
    assert!(in_town > 0.0);
    assert!((hands.hands[2] - c.harvest_hands.town_share * in_town).abs() < 1e-9);
    assert!((hands.visiting[0] + hands.visiting[1] - hands.hands[2]).abs() < 1e-9);
    assert!(hands.visiting[0] > 2.0 * hands.visiting[1]);
    assert_eq!(hands.visiting[2], 0.0);
    // Outside the harvest nobody goes.
    let quiet = work::harvest_help(&world.population, n, world.households.len(), 3, &c, &force);
    let none = work::harvest_hands(&quiet.households, &world.households, &world.land, &world.geo, &c);
    assert!(none.visiting.iter().all(|&v| v == 0.0));
}

#[test]
fn harvest_hands_bring_grain_home_to_the_town() {
    let mut c = region("city3");
    c.months = 9; // through the harvest months of the first summer
    c.start_month = 1;
    let towns: Vec<usize> = (0..c.villages.len()).filter(|&v| c.villages[v].kind == "town").collect();
    let town_grain = |sim: &Simulation| -> f64 {
        let hh = &sim.world.households;
        (0..hh.len()).filter(|&h| towns.contains(&(hh.location[h] as usize))).map(|h| hh.grain[h]).sum()
    };
    let mut more = 0;
    for seed in 0..4 {
        let mut without = c.clone();
        without.seed = seed;
        let mut with = without.clone();
        with.harvest_hands.enabled = true;
        let (mut a, mut b) = (Simulation::new(without).unwrap(), Simulation::new(with).unwrap());
        // (the runs part before the harvest: the outlook counts on the hands)
        a.run();
        b.run();
        more += (town_grain(&b) > town_grain(&a)) as usize;
    }
    assert_eq!(more, 4, "towns' families hold more grain after a harvest worked by their hands");
}

#[test]
fn families_fleeing_a_famine_go_to_their_market_town() {
    let mut c = on_map(&[(0.0, 0.0), (4.0, 0.0)], &[(2.0, 0.0)]);
    c.refugees.enabled = true;
    c.refugees.to_town = 1.0;
    c.migration.to_town = 0.0;
    c.migration.stay_in_region = 0.0; // nobody finds a better village
    let world = create_world(&c, &mut RandomStreams::new(1)).unwrap();
    let origin = vec![0usize; 200];
    let fleeing: Vec<bool> = (0..200).map(|j| j % 2 == 0).collect();
    let place = vec![1.0; 3];
    let people = vec![200.0, 200.0, 300.0];
    let dest =
        migration::destinations(&origin, &place, &people, &c, &mut Rng::named(0, "test"), &world.geo, Some(&fleeing));
    for j in 0..200 {
        assert_eq!(dest[j], if fleeing[j] { 2 } else { -1 }, "family {j}");
    }
}

#[test]
fn towns_crowded_with_famine_refugees_bury_more() {
    let mut c = region("city3");
    c.months = 30;
    c.scheduled_events = vec![drought(4), drought(16)];
    let towns: Vec<usize> = (0..c.villages.len()).filter(|&v| c.villages[v].kind == "town").collect();
    let deaths_in_towns = |c: Config| -> f64 {
        let mut sim = Simulation::new(c).unwrap();
        sim.run();
        let deaths = sim.place_series().into_iter().find(|(name, _)| *name == "deaths").unwrap().1;
        deaths.iter().map(|month| towns.iter().map(|&t| month[t]).sum::<f64>()).sum()
    };
    let (mut without, mut with) = (0.0, 0.0);
    for seed in 0..8 {
        let mut a = c.clone();
        a.seed = seed;
        let mut b = a.clone();
        b.refugees.enabled = true;
        without += deaths_in_towns(a);
        with += deaths_in_towns(b);
    }
    assert!(with > without, "{with} deaths in towns with refugees, {without} without");
}

#[test]
fn invariants_hold_every_month_with_harvest_hands_and_refugees() {
    for (name, mut c) in [("city and 3 towns", region("city3")), ("region of 20", region("region20"))] {
        c.months = 30;
        c.harvest_hands.enabled = true;
        c.refugees.enabled = true;
        c.scheduled_events = vec![drought(4), drought(16)];
        let mut sim = Simulation::new(c).unwrap();
        let money = sim.world.money();
        for _ in 0..sim.config.months {
            sim.step();
            let broken = broken_invariants(&sim.world);
            assert!(broken.is_empty(), "{name}, month {}: {broken:?}", sim.world.month);
            assert!((sim.world.money() - money).abs() <= 1e-9 * money, "{name}: money not conserved");
        }
    }
}
