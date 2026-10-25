//! Migration: who leaves a place and who comes, by how good a place it is.
//!
//! How good a place to live a village is (1 = an ordinary one) depends on
//! its livelihood (what a wage or a share of the land feeds), on what people
//! remember of it (recent hunger, and danger), on its size and on how much
//! its lord takes.
//!
//! Young single people leave each year by chance, and more when the place is
//! worse than ordinary; families leave too, the landless more readily, and
//! people follow those who went before. In a famine the hungriest families
//! flee; while plague or raiders are about, families flee them too. With
//! several places, half of those who leave settle in another that is a
//! better place (on a map, most of them nearby), bringing their coins and
//! grain; the rest go to the town or beyond, taking theirs. Young people
//! from beyond come to a place that is clearly better than ordinary, as
//! servants of its landholders.

use crate::config::Config;
use crate::geography::Geography;
use crate::grid::{clip, div_or};
use crate::households::{sizes, Households};
use crate::population::{Population, NO_JOB};
use crate::rng::Rng;
use crate::rules::{self, monthly_chance};

/// What people in and around a place remember of it, per place.
#[derive(Clone, Debug, PartialEq)]
pub struct Memory {
    pub hunger: Vec<f64>,     // recent share of need not eaten
    pub danger: Vec<f64>,     // recent deaths beyond the usual (a yearly share), and plunder
    pub leaving: Vec<f64>,    // recent emigration (a yearly share of the place)
    pub danger_now: Vec<f64>, // this month's danger
}

impl Memory {
    pub fn calm(n: usize) -> Memory {
        Memory { hunger: vec![0.0; n], danger: vec![0.0; n], leaving: vec![0.0; n], danger_now: vec![0.0; n] }
    }
}

/// Add a month's hunger and danger to the memory (older months fade).
/// Danger is the yearly share of people lost beyond the usual: deaths beyond
/// what the usual mortality and chance would bring (in a hamlet one death
/// more is luck, not famine), or, while an epidemic or raiders are known to
/// be about, the deaths they are known to bring (`mortality_mult`),
/// whichever is more; plus grain plundered.
pub fn remember(
    memory: &mut Memory,
    ration: &[f64],
    deaths: &[f64],
    people: &[f64],
    plundered: &[f64],
    config: &Config,
    mortality_mult: Option<&[f64]>,
) {
    let cfg = &config.migration;
    let fade = 1.0 / cfg.memory_months;
    for v in 0..people.len() {
        let usual = people[v] * cfg.normal_mortality / 12.0;
        let beyond = (deaths[v] - usual - 2.0 * usual.sqrt()).max(0.0);
        let mut danger = div_or(12.0 * beyond, people[v], 0.0);
        if let Some(mult) = mortality_mult {
            danger = danger.max((mult[v] - 1.0) * cfg.normal_mortality);
        }
        danger += cfg.plunder_weight * 12.0 * plundered[v];
        memory.hunger[v] += fade * ((1.0 - ration[v]).max(0.0) - memory.hunger[v]);
        memory.danger[v] += fade * (danger - memory.danger[v]);
        memory.danger_now[v] = danger;
    }
}

/// Add a month's departures (a year's memory).
pub fn remember_leaving(memory: &mut Memory, left: &[f64], people: &[f64]) {
    for v in 0..left.len() {
        let yearly = div_or(12.0 * left[v], people[v] + left[v], 0.0);
        memory.leaving[v] += (yearly - memory.leaving[v]) / 12.0;
    }
}

/// How good a place to live each place is, 1 being an ordinary one.
pub fn appeal(
    prospects: &[f64],
    memory: &Memory,
    people: &[f64],
    land: &[f64],
    lord_take: &[f64],
    config: &Config,
) -> Vec<f64> {
    let cfg = &config.migration;
    (0..people.len())
        .map(|v| {
            let livelihood = prospects[v] / cfg.usual_prospects;
            let safety = (-cfg.hunger_weight * memory.hunger[v] - cfg.danger_weight * memory.danger[v]).exp();
            let size = clip(people[v] / cfg.small_village, 0.0, 1.0).powf(cfg.size_exponent);
            // Land to spare draws people and crowding sends them away: what a
            // family's share of the land would yield, against the usual density.
            let usual_land = people[v] / config.environment.people_per_plot;
            let spare = div_or(land[v], usual_land, 1.0);
            let room = clip(spare.powf(1.0 - config.food.labor_share), cfg.land_range.0, cfg.land_range.1);
            let lightness = div_or(cfg.usual_take, lord_take[v], f64::INFINITY);
            let burden = clip(lightness.powf(cfg.burden_exponent), cfg.burden_range.0, cfg.burden_range.1);
            livelihood * safety * size * room * burden
        })
        .collect()
}

/// This month's moves.
#[derive(Clone, Debug)]
pub struct Moves {
    pub left: Vec<f64>,     // people who left, per place
    pub arrived: Vec<f64>,  // people who came, from beyond or from another place, per place
    pub refugees: Vec<f64>, // of them, people of families fleeing a famine, per place
    pub families: u64,      // whole families that left
    pub within: u64,        // people who moved to another place of the run
    pub coins: f64,         // coins taken beyond the places
    pub grain: f64,         // rations taken beyond the places
}

impl Moves {
    fn none(n: usize) -> Moves {
        Moves {
            left: vec![0.0; n],
            arrived: vec![0.0; n],
            refugees: vec![0.0; n],
            families: 0,
            within: 0,
            coins: 0.0,
            grain: 0.0,
        }
    }
}

/// For each leaver from `origin`, another place that is a better place
/// (chosen by its people times how much better, and on a map, the nearer
/// the likelier), or -1 for beyond.
pub fn destinations(
    origin: &[usize],
    place: &[f64],
    people: &[f64],
    config: &Config,
    rng: &mut Rng,
    geo: &Geography,
    refugees: Option<&[bool]>,
) -> Vec<i64> {
    let n = place.len();
    let mut dest = vec![-1i64; origin.len()];
    if n < 2 || origin.is_empty() {
        return dest;
    }
    let stays: Vec<bool> = origin.iter().map(|_| rng.random() < config.migration.stay_in_region).collect();
    let mut better = vec![0.0; n];
    for (j, &o) in origin.iter().enumerate() {
        if !stays[j] {
            continue;
        }
        let mut total = 0.0;
        for d in 0..n {
            let mut b = (place[d] - place[o]).max(0.0) * people[d];
            if geo.on_map {
                b *= (-geo.distance(o, d) / config.migration.distance_km).exp();
            }
            if d == o {
                b = 0.0;
            }
            better[d] = b;
            total += b;
        }
        if total > 0.0 {
            let u = rng.random();
            let mut running = 0.0;
            let mut pick = 0;
            for &b in better.iter() {
                running += b;
                if u >= running / total {
                    pick += 1;
                }
            }
            dest[j] = pick.min(n - 1) as i64;
        }
    }
    if geo.has_towns() {
        // Of those going beyond, some go to their own market town; families
        // fleeing a famine more readily (`RefugeesConfig`).
        for (j, &o) in origin.iter().enumerate() {
            let town = geo.market[o];
            let u = rng.random();
            let to_town =
                if refugees.is_some_and(|r| r[j]) { config.refugees.to_town } else { config.migration.to_town };
            if dest[j] < 0 && town >= 0 && town != o as i64 && u < to_town {
                dest[j] = town;
            }
        }
    }
    dest
}

/// Families who take in a newcomer: landholders, and in a town any family
/// (a servant, an apprentice, a journeyman).
fn can_host(households: &Households, h: usize, living: bool, geo: &Geography) -> bool {
    living && (households.land[h] > 0.0 || (geo.has_towns() && geo.town[households.location[h] as usize]))
}

/// Host families of each place, in order.
fn hosts_by_place(
    households: &Households,
    living: impl Fn(usize) -> bool,
    geo: &Geography,
    n: usize,
) -> Vec<Vec<usize>> {
    let mut hosts = vec![Vec::new(); n];
    for h in 0..households.len() {
        if can_host(households, h, living(h), geo) {
            hosts[households.location[h] as usize].push(h);
        }
    }
    hosts
}

/// This month's departures: young singles and whole families, by how good
/// a place their village is (`place`), and the hungriest families in a
/// famine. Those settling in another place move there; the rest leave.
#[allow(clippy::too_many_arguments)]
pub fn move_people(
    population: &mut Population,
    households: &mut Households,
    place: &[f64],
    memory: &Memory,
    family_share: &[f64],
    config: &Config,
    rng: &mut Rng,
    geo: &Geography,
) -> Moves {
    let cfg = &config.migration;
    let n = place.len();
    let n_hh = households.len();
    if !cfg.enabled || population.is_empty() {
        return Moves::none(n);
    }
    let worse: Vec<f64> = place.iter().map(|p| (1.0 - p).max(0.0)).collect();
    let follow: Vec<f64> = memory.leaving.iter().map(|l| 1.0 + cfg.chain * l).collect();

    // Young single people.
    let single_chance: Vec<f64> =
        (0..n).map(|v| monthly_chance(cfg.leave_chance) * (1.0 + cfg.push * worse[v]) * follow[v]).collect();
    // (only they roll: rows emptied this month, dropped at its end, don't)
    let mut goes = vec![false; population.len()];
    for i in 0..population.len() {
        let age = population.age_years(i) as i64;
        let young =
            (population.count[i] == 1) & !population.married[i] & (age >= cfg.young_ages.0) & (age <= cfg.young_ages.1);
        if young {
            goes[i] = rng.random() < single_chance[population.location[i] as usize];
        }
    }

    // Whole families, the landless more readily, and the hungriest in a famine.
    let size = sizes(population, n_hh);
    let decide = households.decide();
    let mut families = vec![false; n_hh];
    for h in 0..n_hh {
        let v = households.location[h] as usize;
        let rooted = if households.land[h] > 0.0 { cfg.rooted } else { 1.0 };
        let yearly = (cfg.family_leave_chance * rooted * ((1.0 + cfg.push * worse[v]) * follow[v])).min(1.0);
        let yearly = decide.leave_chance(h, yearly);
        families[h] = size[h] > 0.0 && rng.random() < monthly_chance(yearly);
    }
    let mut refugee = vec![false; n_hh];
    for h in 0..n_hh {
        let fled = rng.random() < cfg.flee_chance;
        refugee[h] = size[h] > 0.0 && family_share[h] < decide.flees_below(h, cfg.flee_below) && fled;
        families[h] |= refugee[h];
    }
    for h in 0..n_hh {
        let fled = rng.random() < cfg.flee_danger * memory.danger_now[households.location[h] as usize];
        families[h] |= size[h] > 0.0 && fled;
    }
    let in_family: Vec<bool> =
        (0..population.len()).map(|i| population.count[i] > 0 && families[population.household[i] as usize]).collect();
    for i in 0..population.len() {
        goes[i] &= !in_family[i];
    }
    if !goes.iter().any(|&g| g) && !families.iter().any(|&f| f) {
        return Moves::none(n);
    }

    // Where they go.
    let people = rules::people_by_location(population, n);
    let singles: Vec<usize> = (0..population.len()).filter(|&i| goes[i]).collect();
    let family_ids: Vec<usize> = (0..n_hh).filter(|&h| families[h]).collect();
    let single_from: Vec<usize> = singles.iter().map(|&i| population.location[i] as usize).collect();
    let family_from: Vec<usize> = family_ids.iter().map(|&h| households.location[h] as usize).collect();
    let mut to_single = destinations(&single_from, place, &people, config, rng, geo, None);
    let fleeing: Vec<bool> = family_ids.iter().map(|&h| refugee[h]).collect();
    let to_family =
        destinations(&family_from, place, &people, config, rng, geo, config.refugees.enabled.then_some(&fleeing));
    // A single person settles with a landholding family there (or any town
    // family), if any: not one that is leaving itself.
    let hosts = hosts_by_place(households, |h| size[h] > 0.0 && !families[h], geo, n);
    let mut host = vec![usize::MAX; singles.len()];
    for (j, d) in to_single.iter_mut().enumerate() {
        if *d >= 0 && hosts[*d as usize].is_empty() {
            *d = -1;
        }
        if *d >= 0 {
            let there = &hosts[*d as usize];
            host[j] = there[(rng.random() * there.len() as f64).floor() as usize];
        }
    }

    // What they take: a single person their share of the family's coins and
    // grain, a family all of it.
    let mut share = vec![0.0; n_hh];
    for &i in &singles {
        share[population.household[i] as usize] += 1.0;
    }
    for h in 0..n_hh {
        share[h] = div_or(share[h], size[h], 0.0);
    }
    for &h in &family_ids {
        share[h] = 1.0;
    }
    let coins: Vec<f64> = (0..n_hh).map(|h| households.money[h] * share[h]).collect();
    let grain: Vec<f64> = (0..n_hh).map(|h| households.grain[h] * share[h]).collect();
    for h in 0..n_hh {
        households.money[h] -= coins[h];
        households.grain[h] -= grain[h];
    }
    let left =
        rules::by_location(population, n, |i| if goes[i] || in_family[i] { population.count[i] as f64 } else { 0.0 });
    let mut arrived = vec![0.0; n];
    let mut refugees = vec![0.0; n];
    let mut beyond_coins = 0.0;
    let mut beyond_grain = 0.0;

    // Singles: beyond, or to their host's family.
    let mut moved: Vec<usize> = Vec::new();
    for (j, &i) in singles.iter().enumerate() {
        let h = population.household[i] as usize;
        let each = (size[h] * share[h]).max(1.0);
        let (each_coins, each_grain) = (coins[h] / each, grain[h] / each);
        if to_single[j] < 0 {
            beyond_coins += each_coins;
            beyond_grain += each_grain;
            population.count[i] = 0;
        } else {
            households.money[host[j]] += each_coins;
            households.grain[host[j]] += each_grain;
            population.household[i] = host[j] as u32;
            population.location[i] = to_single[j] as u32;
            arrived[to_single[j] as usize] += 1.0;
            moved.push(i);
        }
    }

    // Families: beyond, or a new household in the place they chose.
    let mut new_home = vec![usize::MAX; n_hh];
    let mut beyond = vec![false; n_hh];
    for (j, &h) in family_ids.iter().enumerate() {
        if to_family[j] < 0 {
            beyond_coins += coins[h];
            beyond_grain += grain[h];
            beyond[h] = true;
        } else {
            let home = households.add(to_family[j] as u32);
            if let Some(minds) = &mut households.minds {
                minds.copy(h, home); // a family takes its mind with it
            }
            households.money[home] = coins[h];
            households.grain[home] = grain[h];
            new_home[h] = home;
        }
    }
    for i in 0..population.len() {
        if population.count[i] == 0 || !in_family[i] {
            continue;
        }
        let h = population.household[i] as usize;
        if beyond[h] {
            population.count[i] = 0;
        } else if new_home[h] != usize::MAX {
            let home = new_home[h];
            arrived[households.location[home] as usize] += population.count[i] as f64;
            if refugee[h] {
                refugees[households.location[home] as usize] += population.count[i] as f64;
            }
            population.household[i] = home as u32;
            population.location[i] = households.location[home];
            moved.push(i);
        }
    }
    // Council officials and healers serve their own place only; a farmer who
    // moves to a town looks for other work.
    let farming = config.farming() as i16;
    let nb = config.businesses.len() as i16;
    for &i in &moved {
        let job = population.job[i];
        let to_town = geo.has_towns() && geo.town[population.location[i] as usize] && job == farming;
        if job >= nb || to_town {
            population.job[i] = NO_JOB;
        }
    }
    let within = arrived.iter().sum::<f64>() as u64;
    Moves {
        left,
        arrived,
        refugees,
        families: family_ids.len() as u64,
        within,
        coins: beyond_coins,
        grain: beyond_grain,
    }
}

/// Young people come from beyond to places that are clearly better places
/// than ordinary (`place` above `welcome`), and settlers to holdings left
/// empty (the `room` on the land beyond the people there) where the place
/// is safe enough; they join landholding families. Returns arrivals per place.
pub fn arrive(
    population: &mut Population,
    households: &Households,
    place: &[f64],
    room: &[f64],
    config: &Config,
    rng: &mut Rng,
    geo: &Geography,
) -> Vec<f64> {
    let cfg = &config.migration;
    let n = place.len();
    let mut arrived = vec![0.0; n];
    if !cfg.enabled {
        return arrived;
    }
    let people = rules::people_by_location(population, n);
    let size = sizes(population, households.len());
    let hosts = hosts_by_place(households, |h| size[h] > 0.0, geo, n);
    for v in 0..n {
        let mut expected = cfg.arrive_rate * people[v] / 1000.0 * clip(place[v] - cfg.welcome, 0.0, cfg.max_pull);
        let safe_enough = clip((place[v] - cfg.settle_appeal) / (cfg.welcome - cfg.settle_appeal), 0.0, 1.0);
        expected += cfg.settle_rate / 12.0 * (room[v] - people[v]).max(0.0) * safe_enough;
        let count = rng.poisson(expected);
        // A place with no host families takes nobody.
        if count == 0 || hosts[v].is_empty() {
            continue;
        }
        arrived[v] = count as f64;
        for _ in 0..count {
            let home = hosts[v][(rng.random() * hosts[v].len() as f64).floor() as usize];
            let age = rng.integers(cfg.young_ages.0 * 12, cfg.young_ages.1 * 12) as i32;
            let female = rng.random() < 0.5;
            let skill = rules::draw_skill(1, &config.skill, rng);
            population.count.push(1);
            population.age_months.push(age);
            population.female.push(female);
            population.health.push(90.0);
            population.skill.push(skill);
            population.location.push(v as u32);
            population.household.push(home as u32);
            population.job.push(NO_JOB);
            population.married.push(false);
            population.couple.push(-1);
        }
    }
    arrived
}
