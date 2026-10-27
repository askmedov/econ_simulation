//! The chronicle: what happened, ranked by importance, through time.

use econ_core::chronicle::{Chronicle, Month};
use econ_core::config::{configs_dir, default_config, Config, ScheduledEvent};
use econ_core::geography;
use econ_core::metrics::MonthRecord;
use econ_core::Simulation;

fn region(name: &str) -> Config {
    Config::from_file(configs_dir().join("regions").join(format!("{name}.json"))).unwrap()
}

fn drought(month: i64) -> ScheduledEvent {
    ScheduledEvent { event: "drought".into(), month, village: None, near: None }
}

#[test]
fn a_forced_drought_heads_the_chronicle_and_the_dearth_follows() {
    let mut c = region("region100");
    c.months = 24;
    c.scheduled_events = vec![drought(4)];
    let mut sim = Simulation::new(c).unwrap();
    sim.run();
    let entries = &sim.chronicle.entries;
    let forced = entries.iter().find(|e| e.what.contains("(forced)")).expect("the forced drought");
    assert_eq!((forced.rank, forced.month_number), (1, 4));
    assert!(forced.what.starts_with("Drought in"), "{}", forced.what);
    assert!(entries.iter().any(|e| e.month_number > 4 && matches!(e.kind, "dearth" | "famine" | "deaths")));
    let text = sim.chronicle.text(&sim.records, 3);
    let brief = text.split("In brief:").nth(1).expect("a year in brief");
    assert!(brief.lines().take(4).any(|l| l.contains("Drought in")));
}

/// The written chronicle's blocks (the year in brief, and each month) with
/// the ranks of their entries in order.
fn blocks(text: &str) -> Vec<(String, Vec<u8>)> {
    let mut blocks: Vec<(String, Vec<u8>)> = Vec::new();
    for line in text.lines() {
        if line.starts_with("  ") && !line.starts_with("   ") {
            blocks.push((line.trim().to_string(), Vec::new()));
        } else if let Some(rank) = line.strip_prefix("    [").and_then(|l| l.get(..1)).and_then(|d| d.parse().ok()) {
            if let Some(block) = blocks.last_mut() {
                block.1.push(rank);
            }
        }
    }
    blocks
}

#[test]
fn each_months_entries_go_from_the_most_important_down() {
    let mut c = region("region100");
    c.months = 24;
    c.scheduled_events = vec![drought(4)];
    let mut sim = Simulation::new(c).unwrap();
    sim.run();
    let months: Vec<(String, Vec<u8>)> =
        blocks(&sim.chronicle.text(&sim.records, 4)).into_iter().filter(|(head, _)| head != "In brief:").collect();
    assert!(months.len() > 5);
    for (month, ranks) in &months {
        assert!(ranks.windows(2).all(|w| w[0] <= w[1]), "{month}: {ranks:?}");
    }
    // Asked for less, it tells less.
    for most in 1..=3u8 {
        for (month, ranks) in blocks(&sim.chronicle.text(&sim.records, most)) {
            if month != "In brief:" {
                assert!(ranks.iter().all(|&r| r <= most), "{month}: {ranks:?} at level {most}");
            }
        }
    }
}

fn record(month_number: i64) -> MonthRecord {
    let mut c = default_config();
    c.months = 1;
    let mut sim = Simulation::new(c).unwrap();
    let mut r = sim.step();
    r.month_number = month_number;
    r
}

#[test]
fn a_famine_names_its_places_worst_first_and_counts_the_rest() {
    let mut c = region("region20");
    c.chronicle.places_listed = 3;
    let geo = geography::build(&c).unwrap();
    let n = c.villages.len();
    let people = vec![300.0; n];
    let none = vec![0.0; n];
    let mut ration = vec![1.0; n];
    for (v, r) in [(2, 0.7), (5, 0.5), (7, 0.75), (9, 0.6), (11, 0.65)] {
        ration[v] = r;
    }
    let r = record(1);
    let month =
        Month { record: &r, people: &people, deaths: &none, ration: &ration, left: &none, started: &[], councils: &[] };
    let mut chronicle = Chronicle::new(n);
    chronicle.month(&month, &c, &geo);
    let famine = chronicle.entries.iter().find(|e| e.kind == "famine").expect("a famine");
    let named: Vec<&str> = famine.places.iter().map(|d| d.place.as_str()).collect();
    let expect: Vec<&str> = [5, 9, 11].iter().map(|&v| c.villages[v].name.as_str()).collect();
    assert_eq!(named, expect);
    assert_eq!(famine.more_places, 2);
    assert!(famine.what.starts_with("Famine in 5 places (1,500 people)"), "{}", famine.what);
    // The same famine next month is no news; its end is.
    let r2 = record(2);
    let again = Month { record: &r2, ..month };
    chronicle.month(&again, &c, &geo);
    assert_eq!(chronicle.entries.iter().filter(|e| e.kind == "famine").count(), 1);
    let fed = vec![1.0; n];
    let r3 = record(3);
    chronicle.month(&Month { record: &r3, ration: &fed, ..again }, &c, &geo);
    assert!(chronicle.entries.iter().any(|e| e.kind == "famine over"));
}

#[test]
fn deaths_within_what_chance_brings_are_no_surge() {
    let c = default_config();
    let geo = geography::build(&c).unwrap();
    let people = vec![1000.0];
    let fed = vec![1.0];
    let none = vec![0.0];
    let mut chronicle = Chronicle::new(1);
    for (k, died) in [3.0, 3.0, 8.0, 3.0, 30.0].iter().enumerate() {
        let r = record(k as i64 + 1);
        let deaths = vec![*died];
        let month = Month {
            record: &r,
            people: &people,
            deaths: &deaths,
            ration: &fed,
            left: &none,
            started: &[],
            councils: &[],
        };
        chronicle.month(&month, &c, &geo);
    }
    let surges: Vec<i64> = chronicle.entries.iter().filter(|e| e.kind == "deaths").map(|e| e.month_number).collect();
    assert_eq!(surges, vec![5], "8 deaths where 3 are usual is luck; 30 is not");
}

#[test]
fn the_same_seed_tells_the_same_chronicle() {
    let mut c = region("city3");
    c.months = 18;
    let (mut a, mut b) = (Simulation::new(c.clone()).unwrap(), Simulation::new(c).unwrap());
    a.run();
    b.run();
    assert_eq!(a.chronicle.entries, b.chronicle.entries);
    assert!(!a.chronicle.entries.is_empty());
}
