//! Crafts beyond the village's four: iron, salt, pots and lead, each made
//! only where a place has its resource and sold across the region to whoever
//! needs it (the smiths, every family, the potters); and the region's crafts
//! sold beyond it at the border towns.

use econ_core::config::{configs_dir, Config};
use econ_core::Simulation;

fn region(name: &str) -> Config {
    Config::from_file(configs_dir().join("regions").join(format!("{name}.json"))).unwrap()
}

fn business(sim: &Simulation, name: &str) -> usize {
    sim.config.businesses.iter().position(|b| b.name == name).unwrap()
}

/// The places that lack `trade`'s resource, where its good is brought in.
fn lacking(sim: &Simulation, trade: &str) -> Vec<usize> {
    let b = business(sim, trade);
    (0..sim.world.lacking.len()).filter(|&v| sim.world.lacking[v][b]).collect()
}

const RESOURCE_TRADES: [&str; 4] = ["ironworks", "saltworks", "potting", "leadworks"];

#[test]
fn nobody_works_a_resource_where_there_is_none_and_its_buyers_there_get_it_all_the_same() {
    let mut c = region("city3");
    c.months = 24;
    let mut sim = Simulation::new(c).unwrap();
    for trade in RESOURCE_TRADES {
        let without = lacking(&sim, trade);
        assert!(!without.is_empty() && without.len() < sim.config.villages.len(), "{trade}");
    }
    let mut made_there = 0.0;
    for _ in 0..sim.config.months {
        sim.step();
        for trade in RESOURCE_TRADES {
            let b = business(&sim, trade);
            let pop = &sim.world.population;
            for i in 0..pop.len() {
                let v = pop.location[i] as usize;
                assert!(!(pop.job[i] == b as i16 && sim.world.lacking[v][b]), "{trade} worked in {v}");
            }
            let good = sim.config.product_index(&sim.config.businesses[b].product);
            made_there += lacking(&sim, trade).iter().map(|&v| sim.world.stock.get(v, good)).sum::<f64>();
        }
    }
    assert_eq!(made_there, 0.0, "nothing is made, or held, where its resource is not");
    // The smiths without iron have it, and the potters without lead.
    for (trade, user, good) in [("ironworks", "smithing", "iron"), ("leadworks", "potting", "lead")] {
        let (user, good) = (business(&sim, user), sim.config.product_index(good));
        let held: f64 = lacking(&sim, trade).iter().map(|&v| sim.world.supplies.get(v, user, good)).sum();
        assert!(held > 0.0, "{trade}");
    }
    for trade in RESOURCE_TRADES {
        let b = business(&sim, trade) as i16;
        assert!(sim.world.population.job.contains(&b), "{trade} has workers");
    }
}

#[test]
fn every_family_needs_salt_and_most_get_it_though_few_places_boil_it() {
    let mut c = region("city3");
    c.months = 24;
    let mut sim = Simulation::new(c).unwrap();
    assert!(lacking(&sim, "saltworks").len() * 10 > 9 * sim.config.villages.len(), "salt is boiled in few places");
    let records = sim.run().to_vec();
    assert!(records.iter().all(|r| r.staples_bought > 0.3 && r.staples_bought <= 1.0 + 1e-9));
    let mean = records.iter().map(|r| r.staples_bought).sum::<f64>() / records.len() as f64;
    assert!(mean > 0.6, "families got {mean:.2} of their salt");
}

/// The mean prices of `goods` over `places` in the last year of 18 months,
/// with `trade` working at `hills` of its usual pace.
fn prices_when_scarce(seed: i64, trade: &str, hills: f64, goods: &[&str], places_lacking: &str) -> Vec<f64> {
    let mut c = region("city3");
    c.months = 18;
    c.seed = seed;
    let mut sim = Simulation::new(c).unwrap();
    let b = business(&sim, trade);
    for v in 0..sim.config.villages.len() {
        let skill = sim.world.skill.get(v, b);
        sim.world.skill.set(v, b, hills * skill);
    }
    let places = lacking(&sim, places_lacking);
    let goods: Vec<usize> = goods.iter().map(|g| sim.config.product_index(g)).collect();
    let mut means = vec![0.0; goods.len()];
    for month in 0..sim.config.months {
        sim.step();
        if month >= sim.config.months - 12 {
            for (k, &p) in goods.iter().enumerate() {
                means[k] +=
                    places.iter().map(|&v| sim.world.prices.get(v, p)).sum::<f64>() / places.len() as f64 / 12.0;
            }
        }
    }
    means
}

#[test]
fn when_the_iron_hills_make_little_iron_tools_grow_dear_across_the_region() {
    for seed in 0..3 {
        let usual = prices_when_scarce(seed, "ironworks", 1.0, &["tools", "iron"], "ironworks");
        let short = prices_when_scarce(seed, "ironworks", 0.1, &["tools", "iron"], "ironworks");
        assert!(short[1] > 3.0 * usual[1], "seed {seed}, iron where there is none: {short:?}, usually {usual:?}");
        assert!(short[0] > 1.1 * usual[0], "seed {seed}, tools where there is no iron: {short:?}, usually {usual:?}");
    }
}

#[test]
fn when_the_lead_hills_make_little_lead_pots_grow_dear_where_they_are_brought_in() {
    for seed in 0..3 {
        let usual = prices_when_scarce(seed, "leadworks", 1.0, &["pottery"], "potting");
        let short = prices_when_scarce(seed, "leadworks", 0.1, &["pottery"], "potting");
        assert!(short[0] > 1.1 * usual[0], "seed {seed}, pots where there is no clay: {short:?}, usually {usual:?}");
    }
}

#[test]
fn the_world_beyond_buys_the_regions_crafts_at_its_border_towns() {
    let mut c = region("city3");
    c.months = 24;
    let mut sim = Simulation::new(c.clone()).unwrap();
    let records = sim.run().to_vec();
    let sold: f64 = records.iter().map(|r| r.crafts_sold).sum();
    assert!(sold > 0.0);
    c.crafts.exports.clear();
    let mut sim = Simulation::new(c).unwrap();
    assert!(sim.run().iter().all(|r| r.crafts_sold == 0.0));
}

#[test]
fn without_the_crafts_the_iron_in_the_hills_changes_nothing() {
    let mut c = region("city3");
    c.months = 12;
    c.crafts.enabled = false;
    let mut bare = c.clone();
    for v in &mut bare.villages {
        v.resources.clear();
    }
    let mut sim = Simulation::new(c).unwrap();
    assert_eq!(sim.config.businesses.len(), 4);
    assert!(!sim.world.lacking.iter().flatten().any(|&l| l));
    assert_eq!(sim.run().to_vec(), Simulation::new(bare).unwrap().run().to_vec());
}

#[test]
fn a_village_off_the_map_has_every_resource_and_boils_its_own_salt() {
    let mut c = Config::from_file(configs_dir().join("village.json")).unwrap();
    c.months = 24;
    let mut sim = Simulation::new(c).unwrap();
    assert!(sim.world.lacking.iter().flatten().all(|&l| !l));
    assert!(sim.world.supply.iter().all(Option::is_none), "nothing is brought in");
    let records = sim.run().to_vec();
    let mean = records.iter().map(|r| r.staples_bought).sum::<f64>() / records.len() as f64;
    assert!(mean > 0.6, "families got {mean:.2} of their salt");
    assert!(sim.world.population.job.contains(&(business(&sim, "saltworks") as i16)));
}
