//! A region on a map: weather shared by neighbours, events by reach,
//! migration nearby, towns fed by their villages, wool, and the books kept
//! (Python's tests/test_geography.py, test_towns.py and test_network.py).
//! Regions are read from the configs exported from Python.

use econ_core::checks::broken_invariants;
use econ_core::config::{
    configs_dir, default_config, Config, Duration, EffectValue, EventSpec, ScheduledEvent, VillageConfig,
};
use econ_core::events::{self, EventState, LastStarted, Weather};
use econ_core::geography::{self, Geography, Routes};
use econ_core::rng::{RandomStreams, Rng};
use econ_core::world::create_world;
use econ_core::{migration, Simulation};

fn region(name: &str) -> Config {
    Config::from_file(configs_dir().join("regions").join(format!("{name}.json"))).unwrap()
}

/// Villages at (x, y) and market towns (numbered after them) on a map.
fn on_map(villages: &[(f64, f64)], towns: &[(f64, f64)], roads: &[(usize, usize)]) -> Config {
    let mut c = default_config();
    let base = c.villages[0].clone();
    let mut places: Vec<VillageConfig> = villages
        .iter()
        .enumerate()
        .map(|(i, &(x, y))| VillageConfig { name: format!("v{i}"), population: 200, land: 70.0, x, y, ..base.clone() })
        .collect();
    places.extend(towns.iter().enumerate().map(|(j, &(x, y))| VillageConfig {
        name: format!("t{j}"),
        population: 100,
        land: 0.0,
        initial_food_months: 0.0,
        x,
        y,
        kind: "town".into(),
        ..base.clone()
    }));
    c.villages = places;
    c.map.enabled = true;
    c.map.roads = roads.to_vec();
    c
}

fn spec(name: &str, chance: f64, reach: &str, end: &str, mult: f64) -> EventSpec {
    EventSpec {
        name: name.into(),
        scope: "location".into(),
        chance,
        effects: [("production_mult".to_string(), EffectValue::Fixed(mult))].into_iter().collect(),
        duration: Duration::Fixed(1),
        months: None,
        group: Some("weather".into()),
        repeat_chance: None,
        businesses: None,
        message: String::new(),
        reach: reach.into(),
        end: end.into(),
    }
}

/// The (event, place) pairs that start in one roll of these events.
fn roll(config: &Config, geo: &Geography, seed: u64, on_map: bool) -> Vec<(String, usize)> {
    let n = config.villages.len();
    let mut active = Vec::new();
    let mut last = LastStarted::new(config.events.len(), n);
    let mut state =
        EventState { active: &mut active, last_started: &mut last, month: 0, month_of_year: 4, n_locations: n };
    let mut rng = Rng::from_pair(seed, 0);
    let mut weather_rng = Rng::from_pair(seed, 1);
    let weather = on_map.then_some(Weather { geo, scale_km: 250.0, rng: &mut weather_rng });
    events::start_location_events(&mut state, config, &mut rng, weather);
    active.iter().map(|e| (config.events[e.spec].name.clone(), e.location)).collect()
}

#[test]
fn the_weather_is_shared_by_neighbours_and_keeps_its_usual_chances() {
    let config = on_map(&[(0.0, 0.0), (2.0, 0.0), (2000.0, 0.0)], &[(0.0, 0.0)], &[]);
    let geo = geography::build(&config).unwrap();
    let mut rng = Rng::named(0, "weather");
    let draws: Vec<Vec<f64>> = (0..3000).map(|_| geography::weather(&geo, 250.0, &mut rng)).collect();
    let corr = |a: usize, b: usize| {
        let (x, y): (Vec<f64>, Vec<f64>) = draws.iter().map(|d| (d[a], d[b])).unzip();
        let (mx, my) = (x.iter().sum::<f64>() / 3000.0, y.iter().sum::<f64>() / 3000.0);
        let cov: f64 = x.iter().zip(&y).map(|(p, q)| (p - mx) * (q - my)).sum();
        let vx: f64 = x.iter().map(|p| (p - mx).powi(2)).sum();
        let vy: f64 = y.iter().map(|q| (q - my).powi(2)).sum();
        cov / (vx * vy).sqrt()
    };
    assert!(corr(0, 1) > 0.99);
    assert!(corr(0, 2).abs() < 0.1);
    let low = draws.iter().filter(|d| d[0] < 0.1).count() as f64 / 3000.0;
    assert!((low - 0.1).abs() < 0.02, "{low}");
}

#[test]
fn droughts_and_good_years_fall_on_neighbours_together() {
    let mut config = on_map(&[(0.0, 0.0), (2.0, 0.0), (4.0, 0.0), (3000.0, 0.0)], &[(0.0, 0.0)], &[]);
    config.events =
        vec![spec("good_weather", 0.3, "weather", "good", 1.25), spec("drought", 0.3, "weather", "bad", 0.6)];
    let geo = geography::build(&config).unwrap();
    let (mut apart, mut together) = (0, 0);
    for seed in 0..300 {
        let hit = roll(&config, &geo, seed, true);
        let has = |name: &str, v: usize| hit.iter().any(|(n, l)| n == name && *l == v);
        for name in ["drought", "good_weather"] {
            let near: Vec<bool> = (0..3).map(|v| has(name, v)).collect();
            together += (near.iter().all(|&x| x) || !near.iter().any(|&x| x)) as usize;
            apart += (has(name, 0) != has(name, 3)) as usize;
        }
        // Never a drought next door to a good year.
        assert!(!(has("drought", 0) && has("good_weather", 1)) && !(has("good_weather", 0) && has("drought", 1)));
    }
    assert!(together as f64 >= 0.97 * 600.0, "{together}");
    assert!(apart > 100, "{apart}"); // far away, the weather is another matter
}

#[test]
fn a_volcano_strikes_every_village_at_once() {
    let mut config = on_map(&[(0.0, 0.0), (500.0, 0.0), (1000.0, 0.0)], &[(0.0, 0.0)], &[]);
    config.events = vec![spec("cold_years", 0.5, "region", "bad", 0.7)];
    let geo = geography::build(&config).unwrap();
    let mut counts: Vec<usize> = (0..100).map(|seed| roll(&config, &geo, seed, true).len()).collect();
    counts.sort();
    counts.dedup();
    assert_eq!(counts, vec![0, 4]); // the three villages and the town
}

#[test]
fn without_a_map_villages_keep_their_own_weather() {
    let mut config = default_config();
    config.villages = vec![config.villages[0].clone(), config.villages[0].clone()];
    config.events = vec![spec("drought", 0.5, "weather", "bad", 0.6)];
    config.events[0].group = None;
    let geo = geography::build(&config).unwrap();
    assert!(!geo.on_map);
    let differ = (0..100).filter(|&seed| roll(&config, &geo, seed, false).len() == 1).count();
    assert!(differ > 30, "{differ}");
}

#[test]
fn a_scheduled_event_can_strike_one_part_of_the_map() {
    let mut config = on_map(&[(0.0, 0.0), (3.0, 0.0), (20.0, 0.0)], &[(10.0, 0.0)], &[(0, 3), (1, 3), (2, 3)]);
    config.months = 1;
    config.scheduled_events =
        vec![ScheduledEvent { event: "drought".into(), month: 1, village: None, near: Some((1.0, 0.0, 5.0)) }];
    let mut sim = Simulation::new(config).unwrap();
    sim.step();
    let mut struck: Vec<usize> = sim
        .world
        .active_events
        .iter()
        .filter(|e| sim.config.events[e.spec].name == "drought")
        .map(|e| e.location)
        .collect();
    struck.sort();
    assert_eq!(struck, vec![0, 1]);
}

#[test]
fn an_event_near_a_place_needs_a_map() {
    let mut config = default_config();
    config.scheduled_events =
        vec![ScheduledEvent { event: "drought".into(), month: 1, village: None, near: Some((1.0, 0.0, 5.0)) }];
    assert!(Simulation::new(config).err().unwrap().contains("needs a map"));
}

#[test]
fn people_who_move_go_mostly_to_villages_nearby() {
    let geo = Geography {
        x: vec![0.0, 5.0, 100.0],
        y: vec![0.0; 3],
        terrain: vec![0; 3],
        transport: vec![0.0; 3],
        market: vec![0; 3],
        pasture: vec![1.0; 3],
        woods: vec![1.0; 3],
        forage: vec![1.0; 3],
        on_map: true,
        town: vec![false; 3],
        border: vec![false; 3],
        routes: Routes::default(),
        arable: vec![1.0; 3],
        to_border: vec![f64::INFINITY; 3],
    };
    let mut config = default_config();
    config.migration.stay_in_region = 1.0;
    let origin = vec![0usize; 2000];
    let (place, people) = ([0.5, 1.0, 1.0], [100.0; 3]);
    let dest = migration::destinations(&origin, &place, &people, &config, &mut Rng::named(0, "test"), &geo, None);
    let near = dest.iter().filter(|&&d| d == 1).count();
    let far = dest.iter().filter(|&&d| d == 2).count();
    assert!(near > 10 * far && far > 0, "{near} {far}");
    let anywhere = Geography { on_map: false, ..geo };
    let dest = migration::destinations(&origin, &place, &people, &config, &mut Rng::named(0, "test"), &anywhere, None);
    let near = dest.iter().filter(|&&d| d == 1).count() as f64 / 2000.0;
    assert!((near - 0.5).abs() < 0.05, "{near}");
}

/// A run of `config` checking the books every month; returns the simulation.
fn keeps_the_books(config: Config) -> Simulation {
    let mut sim = Simulation::new(config).unwrap();
    let money = sim.world.money();
    let mut people = sim.world.population.size() as i64;
    for _ in 0..sim.config.months {
        let r = sim.step();
        people += r.births - r.deaths - r.emigrants + r.immigrants;
        assert_eq!(people, r.population);
        let broken = broken_invariants(&sim.world);
        assert!(broken.is_empty(), "month {}: {broken:?}", r.month_number);
        assert!((sim.world.money() - money).abs() <= 1e-9 * money, "month {}: money", r.month_number);
    }
    sim
}

#[test]
fn a_region_keeps_the_books_and_each_village_its_record() {
    let mut config = region("region20");
    config.months = 36;
    config.seed = 3;
    let sim = keeps_the_books(config);
    let places = &sim.places[0]; // population, month by month
    assert_eq!((places.len(), places[0].len()), (36, sim.config.villages.len()));
    for (month, r) in places.iter().zip(&sim.records) {
        assert_eq!(month.iter().sum::<f64>() as i64, r.population);
    }
    let deaths: f64 = sim.places[2].iter().flatten().sum();
    assert_eq!(deaths as i64, sim.records.iter().map(|r| r.deaths).sum::<i64>());
    let r = &sim.records;
    assert!(r.iter().all(|x| x.town_people > 0));
    assert!(r.iter().map(|x| x.grain_exported).sum::<f64>() > 0.0); // villages fed the town
    assert!(r.iter().map(|x| x.town_ration).sum::<f64>() / r.len() as f64 > 0.9);
}

#[test]
fn a_region_with_a_city_keeps_the_books_and_its_towns_trade() {
    let mut config = region("city3");
    config.months = 36;
    config.seed = 1;
    let sim = keeps_the_books(config);
    assert!(sim.records.iter().map(|r| r.grain_between_towns).sum::<f64>() > 0.0);
    assert!(sim.records.iter().map(|r| r.wool_sold).sum::<f64>() > 0.0);
}

#[test]
fn a_town_lives_on_its_villages_grain() {
    let mut config = region("region20");
    config.months = 12;
    config.seed = 3;
    config.random_events = false;
    let mut sim = Simulation::new(config).unwrap();
    sim.run();
    let town_need: f64 =
        sim.records.iter().map(|r| r.food_needed * r.town_people as f64 / r.population.max(1) as f64).sum();
    let carted: f64 = sim.records.iter().map(|r| r.grain_exported).sum();
    assert!(carted > 0.5 * town_need, "{carted} {town_need}");
}

#[test]
fn wool_is_sold_beyond_the_border_in_june_and_a_slump_cuts_it() {
    let mut config = region("region20");
    config.months = 6;
    config.start_month = 1;
    config.random_events = false;
    let mut sim = Simulation::new(config.clone()).unwrap();
    let money = sim.world.money();
    sim.run();
    let wool: Vec<f64> = sim.records.iter().map(|r| r.wool_sold).collect();
    assert!(wool[5] > 0.0 && wool[..5].iter().sum::<f64>() == 0.0);
    assert!((sim.world.money() - money).abs() <= 1e-9 * money); // the coins came from beyond (the town's purse)
    config.scheduled_events = vec![ScheduledEvent { event: "wool_slump".into(), month: 1, village: None, near: None }];
    let mut slump = Simulation::new(config.clone()).unwrap();
    slump.run();
    let ratio = slump.records[5].wool_sold / wool[5];
    assert!((ratio - 0.4).abs() < 0.08, "{ratio}");
    config.scheduled_events.clear();
    for v in config.villages.iter_mut() {
        v.border = false;
    }
    let mut no_border = Simulation::new(config).unwrap();
    no_border.run();
    assert_eq!(no_border.records[5].wool_sold, 0.0);
}

#[test]
fn a_wool_slump_is_never_rolled_without_a_map() {
    let mut config = default_config();
    let mut slump = config.events.iter().find(|e| e.name == "wool_slump").unwrap().clone();
    assert!(slump.effects.keys().all(|e| events::MAP_ONLY_EFFECTS.contains(&e.as_str())));
    slump.chance = 1.0;
    config.events = vec![slump];
    let geo = geography::build(&config).unwrap();
    assert!(roll(&config, &geo, 0, false).is_empty());
}

#[test]
fn a_region_is_set_up_with_its_towns_and_no_farmland_in_them() {
    let config = region("city3");
    let world = create_world(&config, &mut RandomStreams::new(1)).unwrap();
    let towns: Vec<usize> = (0..config.villages.len()).filter(|&i| world.geo.town[i]).collect();
    assert_eq!(towns.len(), 3);
    assert!(towns.iter().all(|&t| world.land[t] == 0.0 && world.dealers_grain[t] > 0.0));
    let farming = config.farming() as i16;
    let pop = &world.population;
    assert!((0..pop.len()).all(|i| !world.geo.town[pop.location[i] as usize] || pop.job[i] != farming));
}
