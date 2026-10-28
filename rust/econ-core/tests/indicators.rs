//! Economic and welfare measures: only read from a run, they must agree
//! with each other and with the region's other records.

use econ_core::config::{Config, ScheduledEvent};
use econ_core::metrics::{inequality, life_table};
use econ_core::Simulation;

fn region(months: i64) -> Config {
    let mut config = Config::from_file(econ_core::config::configs_dir().join("regions").join("region20.json"))
        .expect("the region of 20 villages");
    config.months = months;
    config
}

fn run(config: Config) -> Simulation {
    let mut sim = Simulation::new(config).unwrap();
    sim.run();
    sim
}

fn close(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance * a.abs().max(b.abs()).max(1.0)
}

#[test]
fn one_chance_of_dying_at_every_age_gives_a_life_of_one_over_the_yearly_rate() {
    let monthly: f64 = 0.004;
    let people = vec![100.0; 101];
    let dying: Vec<f64> = people.iter().map(|p| p * monthly).collect();
    let (years, under_five) = life_table(&people, &dying, |_| monthly);
    let yearly_rate = -12.0 * (1.0 - monthly).ln();
    assert!(close(years, 1.0 / yearly_rate, 0.02), "{years} against {}", 1.0 / yearly_rate);
    let survive_a_year = (1.0 - monthly).powi(12);
    assert!(close(under_five, 1000.0 * (1.0 - survive_a_year.powi(5)), 1e-9));
}

#[test]
fn an_age_nobody_is_at_takes_its_usual_chance() {
    let mut people = vec![100.0; 101];
    let mut dying: Vec<f64> = people.iter().map(|p| p * 0.002).collect();
    people[3] = 0.0;
    dying[3] = 0.0;
    let (with_gap, _) = life_table(&people, &dying, |_| 0.002);
    let (full, _) = life_table(&vec![100.0; 101], &vec![0.2; 101], |_| 0.002);
    assert!(close(with_gap, full, 1e-12));
}

#[test]
fn wealth_shared_equally_is_not_unequal_and_all_in_one_hand_nearly_all_is() {
    let (gini, top) = inequality(&[5.0; 10], &[1.0; 10]);
    assert!(gini.abs() < 1e-12 && close(top, 0.1, 1e-12));
    let mut one_has_all = vec![0.0; 10];
    one_has_all[3] = 50.0;
    let (gini, top) = inequality(&one_has_all, &[1.0; 10]);
    assert!(close(gini, 0.9, 1e-12) && close(top, 1.0, 1e-12));
    // a family of two counts as two people with its wealth a head; a debt as nothing
    let (a, _) = inequality(&[1.0, 3.0], &[2.0, 1.0]);
    let (b, _) = inequality(&[1.0, 1.0, 3.0], &[1.0, 1.0, 1.0]);
    assert!(close(a, b, 1e-12));
    let (c, _) = inequality(&[-4.0, 1.0, 1.0, 3.0], &[1.0, 1.0, 1.0, 1.0]);
    let (d, _) = inequality(&[0.0, 1.0, 1.0, 3.0], &[1.0, 1.0, 1.0, 1.0]);
    assert!(close(c, d, 1e-12));
}

#[test]
fn the_places_measures_add_up_to_the_region() {
    let sim = run(region(15));
    let series = sim.place_series();
    let place = |name: &str| &series.iter().find(|(n, _)| *n == name).unwrap().1;
    for (t, record) in sim.records.iter().enumerate() {
        let people = &place("population")[t];
        let total = |name: &str| place(name)[t].iter().sum::<f64>();
        let by_people = |name: &str| place(name)[t].iter().zip(people).map(|(x, p)| x * p).sum::<f64>();
        assert!(close(total("output"), record.output, 1e-9));
        assert_eq!(total("underfed"), record.underfed as f64);
        assert!(close(by_people("wealth"), record.wealth * record.population as f64, 1e-9));
        assert!(close(by_people("cost_of_living") / people.iter().sum::<f64>(), record.cost_of_living, 1e-9));
        assert!(close(record.real_wage, record.wage / record.cost_of_living, 1e-9));
        if t > 0 {
            let before = &sim.records[t - 1];
            assert!(close(record.coins_held - before.coins_held, record.coins_gained, 1e-9));
        }
    }
}

#[test]
fn life_is_short_as_the_mortality_table_says_and_shorter_in_a_famine() {
    let config = region(30);
    let mut struck = config.clone();
    struck.scheduled_events.push(ScheduledEvent { event: "drought".into(), month: 4, village: None, near: None });
    let (calm, famine) = (run(config), run(struck));
    let lowest = |sim: &Simulation| sim.records.iter().map(|r| r.life_expectancy).fold(f64::INFINITY, f64::min);
    // (the usual chances of dying: 28% in the first year, 7% a year to five)
    let first = &calm.records[0];
    assert!((20.0..30.0).contains(&first.life_expectancy), "{}", first.life_expectancy);
    assert!((400.0..550.0).contains(&first.under_five_deaths), "{}", first.under_five_deaths);
    assert!(lowest(&famine) < lowest(&calm) - 2.0, "{} against {}", lowest(&famine), lowest(&calm));
    assert!(famine.records.iter().all(|r| (0.0..1.0).contains(&r.wealth_gini) && r.top_tenth_wealth <= 1.0));
}
