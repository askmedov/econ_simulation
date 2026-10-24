//! Families at the start and as people marry (Python's tests/test_households.py).

use econ_core::config::{default_config, Config};
use econ_core::households::{self, Households};
use econ_core::population::{Population, NO_JOB};
use econ_core::rng::{RandomStreams, Rng};
use econ_core::world::{create_world, World};
use econ_core::{economy, grid::Grid, rules};

fn village(population: i64, villages: usize, seed: i64) -> (World, Config) {
    let mut config = default_config();
    config.seed = seed;
    let mut v = config.villages[0].clone();
    v.population = population;
    config.villages = (0..villages)
        .map(|i| {
            let mut v = v.clone();
            v.name = format!("v{i}");
            v
        })
        .collect();
    (create_world(&config, &mut RandomStreams::new(seed)).unwrap(), config)
}

#[test]
fn everyone_belongs_to_a_household_of_their_village() {
    let (world, _) = village(1000, 2, 0);
    let pop = &world.population;
    for i in 0..pop.len() {
        assert_eq!(world.households.location[pop.household[i] as usize], pop.location[i]);
    }
}

#[test]
fn households_are_family_sized() {
    let (world, _) = village(1000, 1, 0);
    let size = households::sizes(&world.population, world.households.len());
    let mean = size.iter().sum::<f64>() / size.len() as f64;
    assert!(3.5 < mean && mean < 6.5, "{mean}");
    assert!(size.iter().all(|&s| s >= 1.0));
}

#[test]
fn every_household_starts_with_a_woman_of_family_age() {
    let (world, _) = village(1000, 1, 0);
    let pop = &world.population;
    let mut headed = vec![false; world.households.len()];
    for i in 0..pop.len() {
        let age = pop.age_years(i);
        if pop.female[i] && (18..=64).contains(&age) {
            headed[pop.household[i] as usize] = true;
        }
    }
    assert!(headed.iter().all(|&h| h));
}

#[test]
fn children_live_with_a_woman_old_enough_to_be_their_mother() {
    let (world, _) = village(1000, 1, 0);
    let pop = &world.population;
    let mut head_age = vec![-1; world.households.len()];
    for i in 0..pop.len() {
        let age = pop.age_years(i);
        if pop.female[i] && pop.married[i] && (18..=64).contains(&age) {
            head_age[pop.household[i] as usize] = age;
        }
    }
    let children: Vec<usize> = (0..pop.len()).filter(|&i| pop.age_years(i) < 18).collect();
    let ok = children
        .iter()
        .filter(|&&c| (18..=45).contains(&(head_age[pop.household[c] as usize] - pop.age_years(c))))
        .count();
    assert!(ok as f64 / children.len() as f64 > 0.9);
}

#[test]
fn a_village_without_women_of_family_age_lives_together() {
    let (world, _) = village(3, 1, 5);
    assert!(!world.households.is_empty());
}

#[test]
fn newborns_join_their_mothers_household() {
    let (mut world, mut config) = village(1000, 1, 0);
    config.demography.annual_birth_chance = 1.0;
    let before = world.population.len();
    rules::births(&mut world.population, &[1.0], &config, &mut Rng::named(0, "test"), 1);
    let pop = &world.population;
    assert!(pop.len() > before);
    for baby in before..pop.len() {
        let h = pop.household[baby];
        assert!((0..before).any(|i| pop.household[i] == h && pop.female[i] && (16..=45).contains(&pop.age_years(i))));
    }
}

#[test]
fn working_age_people_have_jobs_and_others_do_not() {
    let (world, config) = village(1000, 1, 0);
    let pop = &world.population;
    for i in 0..pop.len() {
        assert_eq!(pop.job[i] != NO_JOB, rules::is_working_age(pop, i, &config));
    }
}

#[test]
fn jobs_follow_people_as_they_age() {
    let (mut world, config) = village(1000, 1, 0);
    let pop = &mut world.population;
    let wanted = Grid::from_rows(&[vec![0.0, 0.0, 10.0, 0.0]], 4); // only weaving is short of workers
    pop.age_months.iter_mut().for_each(|a| *a = 14 * 12 + 11); // everyone about to turn 15
    economy::assign_new_workers(pop, &wanted, &config, None);
    assert!(pop.job.iter().all(|&j| j == NO_JOB));
    rules::grow_older(pop);
    economy::assign_new_workers(pop, &wanted, &config, None);
    let weaving = config.business_index("weaving") as i16;
    assert!(pop.job.iter().all(|&j| j == weaving));
    pop.age_months.iter_mut().for_each(|a| *a = 60 * 12); // everyone retires
    economy::assign_new_workers(pop, &wanted, &config, None);
    assert!(pop.job.iter().all(|&j| j == NO_JOB));
}

/// Families from (household, age, female, married) tuples; married men and
/// women of each household are couples in age order.
fn families(members: &[(u32, i32, bool, bool)], money: Option<&[f64]>) -> (Population, Households) {
    let n = members.len();
    let mut pop = Population {
        count: vec![1; n],
        age_months: members.iter().map(|m| m.1 * 12).collect(),
        female: members.iter().map(|m| m.2).collect(),
        health: vec![100.0; n],
        skill: vec![1.0; n],
        location: vec![0; n],
        household: members.iter().map(|m| m.0).collect(),
        job: vec![NO_JOB; n],
        married: members.iter().map(|m| m.3).collect(),
        couple: vec![-1; n],
    };
    pair_up(&mut pop);
    let n_households = pop.household.iter().max().unwrap() + 1;
    let mut hh = Households::with_locations(vec![0; n_households as usize]);
    if let Some(m) = money {
        hh.money = m.to_vec();
    }
    (pop, hh)
}

fn pair_up(pop: &mut Population) {
    pop.couple.iter_mut().for_each(|c| *c = -1);
    let mut next = 0;
    let largest = *pop.household.iter().max().unwrap();
    for h in 0..=largest {
        let mut men: Vec<usize> =
            (0..pop.len()).filter(|&i| pop.household[i] == h && pop.married[i] && !pop.female[i]).collect();
        let mut women: Vec<usize> =
            (0..pop.len()).filter(|&i| pop.household[i] == h && pop.married[i] && pop.female[i]).collect();
        men.sort_by_key(|&i| -pop.age_months[i]);
        women.sort_by_key(|&i| -pop.age_months[i]);
        for (&m, &w) in men.iter().zip(&women) {
            pop.couple[m] = next;
            pop.couple[w] = next;
            next += 1;
        }
    }
}

fn sure_weddings() -> Config {
    let mut config = default_config();
    config.demography.marriage_chance = 1.0;
    config
}

#[test]
fn some_young_people_start_out_single() {
    let (world, _) = village(1000, 1, 0);
    let pop = &world.population;
    let young: Vec<usize> =
        (0..pop.len()).filter(|&i| pop.female[i] && (18..=22).contains(&pop.age_years(i))).collect();
    let single = young.iter().filter(|&&i| !pop.married[i]).count() as f64 / young.len() as f64;
    assert!(0.2 < single && single < 0.9, "{single}");
    assert!((0..pop.len()).filter(|&i| pop.age_years(i) >= 40).all(|i| pop.married[i]));
    assert!((0..pop.len()).filter(|&i| pop.age_years(i) < 18).all(|i| !pop.married[i]));
}

#[test]
fn the_first_to_marry_stays_as_heir_and_the_rest_leave() {
    let config = sure_weddings();
    let (mut pop, mut hh) = families(
        &[
            (0, 50, false, true),
            (0, 48, true, true),
            (0, 22, false, false),
            (0, 21, false, false),
            (1, 52, false, true),
            (1, 47, true, true),
            (1, 20, true, false),
            (1, 19, true, false),
        ],
        Some(&[80.0, 80.0]),
    );
    let weddings = households::marry(&mut pop, &mut hh, &config, &mut Rng::named(0, "test"), None);
    assert_eq!(weddings, 2);
    // One couple stays with the groom's family, the other with the bride's.
    assert_eq!(hh.len(), 2);
    let mut sons = vec![pop.household[2], pop.household[3]];
    let mut daughters = vec![pop.household[6], pop.household[7]];
    sons.sort();
    daughters.sort();
    assert_eq!((sons, daughters), (vec![0, 1], vec![0, 1]));
    assert!(pop.married.iter().all(|&m| m));
    // A third wedding into either family would set up a new household.
    let (more, _) = families(&[(0, 20, false, false), (1, 18, true, false)], None);
    let mut more = more;
    more.couple = vec![-1, -1];
    pop.append(&more);
    assert_eq!(households::marry(&mut pop, &mut hh, &config, &mut Rng::named(1, "test"), None), 1);
    let n = pop.len();
    assert_eq!(hh.len(), 3);
    assert_eq!((pop.household[n - 2], pop.household[n - 1]), (2, 2));
}

#[test]
fn brothers_and_sisters_of_an_orphaned_family_do_not_all_stay() {
    let config = sure_weddings();
    // Parents gone: the eldest brother married in; his younger brother leaves.
    let (mut pop, mut hh) = families(
        &[(0, 30, false, true), (0, 28, true, true), (0, 24, false, false), (1, 45, true, true), (1, 20, true, false)],
        None,
    );
    pop.married[3] = true;
    households::marry(&mut pop, &mut hh, &config, &mut Rng::named(0, "test"), None);
    assert_eq!(pop.household[2], 1);
    assert_eq!(pop.household[4], 1); // with the bride's family
}

#[test]
fn whoever_leaves_takes_their_share_of_the_savings() {
    let config = sure_weddings();
    let (mut pop, mut hh) = families(
        &[
            (0, 40, true, true),
            (0, 42, false, true),
            (0, 20, true, false),
            (0, 2, true, false),
            (1, 70, true, true),
            (1, 68, false, true),
            (1, 40, true, true),
            (1, 41, false, true),
            (1, 21, false, false),
        ],
        Some(&[40.0, 100.0]),
    );
    households::marry(&mut pop, &mut hh, &config, &mut Rng::named(0, "test"), None);
    // The groom's family already has its heir couple; the bride's has room.
    assert_eq!(pop.household[8], 0);
    assert!((hh.money[1] - (100.0 - 100.0 / 5.0)).abs() < 1e-9);
    assert!((hh.money[0] - (40.0 + 100.0 / 5.0)).abs() < 1e-9);
    assert!((hh.money.iter().sum::<f64>() - 140.0).abs() < 1e-9);
}

#[test]
fn nobody_marries_when_no_one_is_willing() {
    let config = sure_weddings();
    let (mut pop, mut hh) =
        families(&[(0, 50, true, true), (0, 20, true, false), (1, 50, true, true), (1, 22, false, false)], None);
    assert_eq!(households::marry(&mut pop, &mut hh, &config, &mut Rng::named(0, "test"), Some(&[0.0])), 0);
    assert_eq!(households::marry(&mut pop, &mut hh, &config, &mut Rng::named(0, "test"), Some(&[1.0])), 1);
}

#[test]
fn families_in_a_long_run_stay_family_sized() {
    let mut config = default_config();
    config.seed = 3;
    config.months = 12 * 15;
    let mut sim = econ_core::Simulation::new(config).unwrap();
    sim.run();
    let size = households::sizes(&sim.world.population, sim.world.households.len());
    let living: Vec<f64> = size.into_iter().filter(|&s| s > 0.0).collect();
    let mean = living.iter().sum::<f64>() / living.len() as f64;
    assert!(3.5 < mean && mean < 6.5, "{mean}");
    assert!(sim.records.iter().map(|r| r.weddings).sum::<i64>() > 50);
}

#[test]
fn a_widow_remarries_and_her_new_husband_moves_in() {
    let config = sure_weddings();
    let (mut pop, mut hh) = families(
        &[(0, 30, true, true), (0, 8, false, false), (1, 50, true, true), (1, 52, false, true), (1, 26, false, false)],
        None,
    );
    pop.couple[0] = 99; // her husband has died
    let alive = rules::spouse_alive(&pop);
    assert!(!alive[0] && alive[2]);
    assert_eq!(households::marry(&mut pop, &mut hh, &config, &mut Rng::named(0, "test"), None), 1);
    assert_eq!(pop.household[4], 0);
    assert!(pop.couple[0] == pop.couple[4] && pop.couple[0] >= 0);
    let alive = rules::spouse_alive(&pop);
    assert!(alive[0] && alive[4]);
}

#[test]
fn couples_are_paired_at_the_start_and_only_they_have_children() {
    let (world, _) = village(1000, 1, 0);
    let pop = &world.population;
    let alive = rules::spouse_alive(pop);
    let paired: Vec<usize> = (0..pop.len()).filter(|&i| alive[i]).collect();
    assert!(paired.len() > 200 && paired.iter().all(|&i| pop.married[i]));
    let mut members = std::collections::HashMap::new();
    for &i in &paired {
        members.entry(pop.couple[i]).or_insert_with(Vec::new).push(i);
    }
    for rows in members.values() {
        assert_eq!(rows.len(), 2);
        assert_eq!(pop.household[rows[0]], pop.household[rows[1]]); // each couple lives together
    }
}
