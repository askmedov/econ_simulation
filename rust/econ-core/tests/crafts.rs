//! Crafts beyond the village's four: iron dug and smelted where a place has
//! it, sold across the region to the smiths, who need it for their tools;
//! and the region's crafts sold beyond it at the border towns.

use econ_core::config::{configs_dir, Config};
use econ_core::Simulation;

fn region(name: &str) -> Config {
    Config::from_file(configs_dir().join("regions").join(format!("{name}.json"))).unwrap()
}

fn business(sim: &Simulation, name: &str) -> usize {
    sim.config.businesses.iter().position(|b| b.name == name).unwrap()
}

/// The places without iron, where it is brought in.
fn without_iron(sim: &Simulation) -> Vec<usize> {
    let ironworks = business(sim, "ironworks");
    (0..sim.world.lacking.len()).filter(|&v| sim.world.lacking[v][ironworks]).collect()
}

#[test]
fn nobody_works_iron_where_there_is_none_and_the_smiths_there_get_it_all_the_same() {
    let mut c = region("city3");
    c.months = 24;
    let mut sim = Simulation::new(c).unwrap();
    let (ironworks, smithing) = (business(&sim, "ironworks"), business(&sim, "smithing"));
    let iron = sim.config.product_index("iron");
    let lacking = without_iron(&sim);
    assert!(!lacking.is_empty() && lacking.len() < sim.config.villages.len());
    let mut made_there = 0.0;
    for _ in 0..sim.config.months {
        sim.step();
        let pop = &sim.world.population;
        for i in 0..pop.len() {
            let v = pop.location[i] as usize;
            assert!(!(pop.job[i] == ironworks as i16 && sim.world.lacking[v][ironworks]), "ironworkers in {v}");
        }
        made_there += lacking.iter().map(|&v| sim.world.stock.get(v, iron)).sum::<f64>();
    }
    assert_eq!(made_there, 0.0, "no iron is made, or held, where there is none");
    let held: f64 = lacking.iter().map(|&v| sim.world.supplies.get(v, smithing, iron)).sum();
    assert!(held > 0.0, "the smiths without iron of their own have it");
    let ironworkers = sim.world.population.job.iter().filter(|&&j| j == ironworks as i16).count();
    assert!(ironworkers > 0);
}

#[test]
fn when_the_iron_hills_make_little_iron_tools_grow_dear_across_the_region() {
    let mut c = region("city3");
    c.months = 18;
    let tools_and_iron = |seed: i64, hills: f64| -> (f64, f64) {
        let mut c = c.clone();
        c.seed = seed;
        let mut sim = Simulation::new(c).unwrap();
        let ironworks = business(&sim, "ironworks");
        for v in 0..sim.config.villages.len() {
            let skill = sim.world.skill.get(v, ironworks);
            sim.world.skill.set(v, ironworks, hills * skill);
        }
        // (the prices where it is brought in, over the last year)
        let lacking = without_iron(&sim);
        let (tool, iron) = (sim.config.product_for("tool"), sim.config.product_index("iron"));
        let (mut tools, mut irons) = (0.0, 0.0);
        for month in 0..sim.config.months {
            sim.step();
            if month >= sim.config.months - 12 {
                let mean = |p| lacking.iter().map(|&v| sim.world.prices.get(v, p)).sum::<f64>() / lacking.len() as f64;
                tools += mean(tool) / 12.0;
                irons += mean(iron) / 12.0;
            }
        }
        (tools, irons)
    };
    for seed in 0..3 {
        let (tools, iron) = tools_and_iron(seed, 1.0);
        let (tools_short, iron_short) = tools_and_iron(seed, 0.1);
        assert!(iron_short > 3.0 * iron, "seed {seed}, iron where there is none: {iron_short}, usually {iron}");
        assert!(tools_short > 1.1 * tools, "seed {seed}, tools where there is no iron: {tools_short}, usually {tools}");
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
fn a_village_off_the_map_makes_its_own_iron() {
    let mut c = Config::from_file(configs_dir().join("village.json")).unwrap();
    c.months = 12;
    let mut sim = Simulation::new(c).unwrap();
    let ironworks = business(&sim, "ironworks");
    sim.run();
    assert!(sim.world.supply.iter().all(Option::is_none));
    assert!(sim.world.population.job.contains(&(ironworks as i16)));
}
