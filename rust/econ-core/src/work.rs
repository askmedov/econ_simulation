//! Work beyond people's trades: everyone at harvest, cloth made at home,
//! and kin who help each other.
//!
//! At harvest every hand is needed: smiths, weavers and woodcutters spend
//! part of their days in the fields, and children and the old glean, bind
//! and carry. Their families earn a share of the harvest for it, and the
//! trades make less those months. The rest of the year women spin and weave
//! at home, so families wear homespun when they can't buy cloth. Kin, a
//! family and the families its sons founded, give each other grain before
//! anyone turns to the neighbours or to a lender.

use crate::config::Config;
use crate::economy::{work_factor, Workforce};
use crate::grid::{div_or, Grid};
use crate::households::Households;
use crate::population::{Population, NO_JOB};
use crate::rules::is_working_age;

pub fn harvest_time(month_of_year: i64, config: &Config) -> bool {
    config.work.enabled && config.work.harvest_months.contains(&month_of_year)
}

/// Who helps in the fields this month, and how much the trades lose.
#[derive(Clone, Debug)]
pub struct HarvestHelp {
    pub farm: Vec<f64>,       // effort added to the fields, per place
    pub kept: Vec<f64>,       // share of their effort each business keeps (per business)
    pub households: Vec<f64>, // effort each household adds to the fields
}

pub fn harvest_help(
    population: &Population,
    n_locations: usize,
    n_households: usize,
    month_of_year: i64,
    config: &Config,
    force: &Workforce,
) -> HarvestHelp {
    let cfg = &config.work;
    let nb = config.businesses.len();
    let mut kept = vec![1.0; nb];
    if !harvest_time(month_of_year, config) {
        return HarvestHelp { farm: vec![0.0; n_locations], kept, households: vec![0.0; n_households] };
    }
    let farming = config.farming();
    for (b, k) in kept.iter_mut().enumerate() {
        if b != farming {
            *k = 1.0 - cfg.craft_help;
        }
    }
    let mut farm = vec![0.0; n_locations];
    let mut households = vec![0.0; n_households];
    let retirement = config.demography.retirement_age as i32;
    for i in 0..population.len() {
        let job = population.job[i];
        let crafts = force.working(i) && job as usize != farming;
        let age = population.age_years(i);
        let young = age >= cfg.helper_ages.0 as i32 && age <= cfg.helper_ages.1 as i32;
        let old = age >= retirement && age <= cfg.old_helper_age as i32;
        let helper = job == NO_JOB && (young || old);
        let helping = if crafts { cfg.craft_help } else { 0.0 } + if helper { cfg.helper_effort } else { 0.0 };
        if helping > 0.0 {
            let help = force.effort[i] * helping;
            households[population.household[i] as usize] += help;
            farm[population.location[i] as usize] += help;
        }
    }
    HarvestHelp { farm, kept, households }
}

/// How much more the fields yield in a harvest month with everyone's help,
/// per place (diminishing returns on the land).
pub fn harvest_boost(farm_effort: &[f64], help_effort: &[f64], config: &Config) -> Vec<f64> {
    let a = config.food.labor_share;
    farm_effort.iter().zip(help_effort).map(|(&f, &h)| (1.0 + div_or(h, f, 0.0)).powf(a)).collect()
}

/// `boost` (per place) for each of the coming `months` that falls in the
/// harvest (places x months); next month first.
pub fn outlook_boost(boost: &[f64], month_of_year: i64, months: usize, config: &Config) -> Grid {
    let mut out = Grid::new(boost.len(), months, 1.0);
    for m in 0..months {
        let upcoming = (month_of_year + m as i64) % 12 + 1;
        if config.work.enabled && config.work.harvest_months.contains(&upcoming) {
            for (r, &b) in boost.iter().enumerate() {
                out.set(r, m, b);
            }
        }
    }
    out
}

/// Cloth each family's women spin and weave at home this month (none at harvest).
pub fn home_cloth(population: &Population, n_households: usize, month_of_year: i64, config: &Config) -> Vec<f64> {
    let cfg = &config.work;
    let mut made = vec![0.0; n_households];
    if !cfg.enabled || cfg.harvest_months.contains(&month_of_year) {
        return made;
    }
    for i in 0..population.len() {
        if population.female[i] && is_working_age(population, i, config) {
            made[population.household[i] as usize] +=
                population.count[i] as f64 * cfg.home_cloth * work_factor(population.health[i], config);
        }
    }
    made
}

/// Grain kin give each other this month. A family `short` of rations it
/// can't afford gets grain from its kin (the family it came from, and the
/// families its children founded) with grain to `spare`; each gives up to
/// `kin_share` of its spare, shared among its needy kin by their need.
/// Moves the grain; returns rations received per household (negative: given).
pub fn kin_help(
    households: &mut Households,
    alive: &[bool],
    short: &[f64],
    spare: &[f64],
    config: &Config,
) -> Vec<f64> {
    let n = households.len();
    let mut moved = vec![0.0; n];
    if !config.work.enabled {
        return moved;
    }
    // Help flows both ways along each link: parents give to children, then
    // children to parents.
    let mut links: Vec<(usize, usize)> = Vec::new();
    for child in 0..n {
        let parent = households.kin[child];
        if parent >= 0 && alive[child] && alive[parent as usize] {
            links.push((parent as usize, child));
        }
    }
    if links.is_empty() {
        return moved;
    }
    // (giver, taker, asked): only where the giver has grain to spare and the
    // taker is short does anything move, so the other pairs are left out.
    let (pairs, asked): (Vec<(usize, usize)>, Vec<f64>) = links
        .iter()
        .map(|&(p, c)| (p, c))
        .chain(links.iter().map(|&(p, c)| (c, p)))
        .filter_map(|(g, t)| {
            let asked = if spare[g] > 0.0 { short[t].max(0.0) } else { 0.0 };
            (asked > 0.0).then_some(((g, t), asked))
        })
        .unzip();
    let mut asked_of = vec![0.0; n];
    for (k, &(g, _)) in pairs.iter().enumerate() {
        asked_of[g] += asked[k];
    }
    let can_give: Vec<f64> = (0..n).map(|h| (config.work.kin_share * spare[h].max(0.0)).min(asked_of[h])).collect();
    let offered: Vec<f64> =
        pairs.iter().enumerate().map(|(k, &(g, _))| asked[k] * div_or(can_give[g], asked_of[g], 0.0)).collect();
    // Nobody takes more than they are short of.
    let mut offered_to = vec![0.0; n];
    for (k, &(_, t)) in pairs.iter().enumerate() {
        offered_to[t] += offered[k];
    }
    let taken: Vec<f64> = (0..n).map(|h| offered_to[h].min(short[h].max(0.0))).collect();
    for (k, &(g, t)) in pairs.iter().enumerate() {
        let given = offered[k] * div_or(taken[t], offered_to[t], 0.0);
        moved[g] -= given;
        moved[t] += given;
    }
    for (grain, m) in households.grain.iter_mut().zip(&moved) {
        *grain += m;
    }
    moved
}
