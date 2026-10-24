//! The village council: it forms once the village is big enough, then taxes
//! wages, employs officials, keeps a food reserve filled by a grain levy, and
//! gives famine relief.
//!
//! State is one value per place. Council staff have job codes past the
//! businesses: officials are `official_job(config)`.

use crate::config::Config;
use crate::economy::in_business;
use crate::grid::div_or;
use crate::population::{Population, NO_JOB};
use crate::rng::Rng;

pub fn official_job(config: &Config) -> i16 {
    config.businesses.len() as i16
}

#[derive(Clone, Debug, PartialEq)]
pub struct Councils {
    pub formed: Vec<bool>,      // whether each place has a council
    pub months_ready: Vec<i64>, // months the place has been big enough
    pub treasury: Vec<f64>,     // coins
    pub reserve: Vec<f64>,      // food held for famine relief, in rations
}

impl Councils {
    pub fn none(n: usize) -> Councils {
        Councils { formed: vec![false; n], months_ready: vec![0; n], treasury: vec![0.0; n], reserve: vec![0.0; n] }
    }
}

/// Count months each place has been big enough; returns places whose council forms now.
pub fn check_formation(councils: &mut Councils, people: &[f64], config: &Config) -> Vec<bool> {
    let cfg = &config.council;
    let n = people.len();
    if !cfg.enabled {
        return vec![false; n];
    }
    (0..n)
        .map(|v| {
            let big = people[v] >= cfg.forms_at_population as f64;
            councils.months_ready[v] = if big { councils.months_ready[v] + 1 } else { 0 };
            let new = !councils.formed[v] && councils.months_ready[v] >= cfg.forms_after_months;
            councils.formed[v] |= new;
            new
        })
        .collect()
}

/// Hire workers from the trades into a council job, or let extras go, to
/// reach `per_1000` staff per 1,000 people where a council has formed.
pub fn staff(
    population: &mut Population,
    councils: &Councils,
    job: i16,
    per_1000: f64,
    config: &Config,
    rng: &mut Rng,
) {
    let n = councils.formed.len();
    let mut people = vec![0.0; n];
    let mut have = vec![0.0; n];
    for i in 0..population.len() {
        let v = population.location[i] as usize;
        people[v] += population.count[i] as f64;
        if population.job[i] == job && population.count[i] > 0 {
            have[v] += population.count[i] as f64;
        }
    }
    let wanted: Vec<f64> = (0..n)
        .map(|v| if councils.formed[v] { (per_1000 * people[v] / 1000.0).round_ties_even() } else { 0.0 })
        .collect();
    let changing: Vec<bool> = (0..n).map(|v| have[v] != wanted[v]).collect();
    if !changing.iter().any(|&c| c) {
        return;
    }
    // Rows of the places that hire or let go, by place, in their order.
    let mut rows_there: Vec<Vec<usize>> = vec![Vec::new(); n];
    for i in 0..population.len() {
        let v = population.location[i] as usize;
        if changing[v] {
            rows_there[v].push(i);
        }
    }
    for v in 0..n {
        if !changing[v] {
            continue;
        }
        let rows = &rows_there[v];
        if have[v] > wanted[v] {
            let mut current: Vec<usize> =
                rows.iter().cloned().filter(|&i| population.job[i] == job && population.count[i] > 0).collect();
            rng.shuffle(&mut current);
            for &i in current.iter().take((have[v] - wanted[v]) as usize) {
                population.job[i] = NO_JOB; // they rejoin a trade next month
            }
        } else {
            let mut pool: Vec<usize> = rows
                .iter()
                .cloned()
                .filter(|&i| in_business(population.job[i], config) && population.count[i] == 1)
                .collect();
            rng.shuffle(&mut pool);
            for &i in pool.iter().take((wanted[v] - have[v]) as usize) {
                population.job[i] = job;
            }
        }
    }
}

/// Take tax from families' wages where a council sits and its treasury is
/// below `target`, except in a `famine` (per place). Changes `income`;
/// returns tax per place.
pub fn collect_taxes(
    income: &mut [f64],
    location: &[u32],
    councils: &mut Councils,
    target: &[f64],
    famine: &[bool],
    config: &Config,
) -> Vec<f64> {
    let n = councils.formed.len();
    let taxing: Vec<bool> =
        (0..n).map(|v| councils.formed[v] && councils.treasury[v] < target[v] && !famine[v]).collect();
    let mut collected = vec![0.0; n];
    for (h, x) in income.iter_mut().enumerate() {
        let v = location[h] as usize;
        if taxing[v] {
            let tax = *x * config.council.tax_rate;
            *x -= tax;
            collected[v] += tax;
        }
    }
    for v in 0..n {
        councils.treasury[v] += collected[v];
    }
    collected
}

/// Pay council staff `pay` each (per place) from the treasury, or what it
/// can afford. Returns (income per household, paid per place).
pub fn pay_staff(
    population: &Population,
    councils: &mut Councils,
    job: i16,
    pay: &[f64],
    n_households: usize,
) -> (Vec<f64>, Vec<f64>) {
    let n = councils.formed.len();
    let mut heads = vec![0.0; n];
    for i in 0..population.len() {
        if population.job[i] == job {
            heads[population.location[i] as usize] += population.count[i] as f64;
        }
    }
    let mut rate = vec![0.0; n];
    let mut paid = vec![0.0; n];
    for v in 0..n {
        paid[v] = (heads[v] * pay[v]).min(councils.treasury[v]);
        rate[v] = div_or(paid[v], heads[v], 0.0);
        councils.treasury[v] -= paid[v];
    }
    let mut income = vec![0.0; n_households];
    for i in 0..population.len() {
        if population.job[i] == job {
            income[population.household[i] as usize] +=
                population.count[i] as f64 * rate[population.location[i] as usize];
        }
    }
    (income, paid)
}

/// Take a share of the harvest into the council's reserve (a tithe in
/// kind) until it holds its target, except in a famine. Changes `granary`;
/// returns rations taken per place.
pub fn levy_grain(
    councils: &mut Councils,
    produced: &[f64],
    granary: &mut [f64],
    need: &[f64],
    famine: &[bool],
    config: &Config,
) -> Vec<f64> {
    let cfg = &config.council;
    (0..produced.len())
        .map(|v| {
            let room = (cfg.reserve_months * need[v] - councils.reserve[v]).max(0.0);
            let mut taken =
                if councils.formed[v] && !famine[v] { (cfg.grain_levy * produced[v]).min(room) } else { 0.0 };
            taken = taken.min(granary[v]);
            councils.reserve[v] += taken;
            granary[v] -= taken;
            taken
        })
        .collect()
}

/// Families who couldn't buy enough food get the rest free from the
/// reserve, shared out if it runs short. Returns rations given per household.
pub fn give_relief(
    councils: &mut Councils,
    bought: &[f64],
    need: &[f64],
    location: &[u32],
    config: &Config,
) -> Vec<f64> {
    if !config.council.relief {
        return vec![0.0; bought.len()];
    }
    let n = councils.formed.len();
    let gap: Vec<f64> = need.iter().zip(bought).map(|(&nd, &b)| (nd - b).max(0.0)).collect();
    let wanted = crate::market::by_village(&gap, location, n);
    let mut share = vec![0.0; n];
    for v in 0..n {
        let given = if councils.formed[v] { wanted[v].min(councils.reserve[v]) } else { 0.0 };
        share[v] = div_or(given, wanted[v], 0.0);
        councils.reserve[v] -= given;
    }
    gap.iter().zip(location).map(|(&g, &l)| g * share[l as usize]).collect()
}

/// Give coins to families who can't afford this month's food and firewood,
/// from whatever the treasury holds beyond `keep` (per place). Changes
/// `money`; returns coins given per place.
pub fn cash_relief(
    councils: &mut Councils,
    money: &mut [f64],
    shortfall: &[f64],
    location: &[u32],
    keep: &[f64],
    config: &Config,
) -> Vec<f64> {
    let n = councils.formed.len();
    if !config.council.relief {
        return vec![0.0; n];
    }
    let wanted = crate::market::by_village(shortfall, location, n);
    let mut given = vec![0.0; n];
    let mut share = vec![0.0; n];
    for v in 0..n {
        let spare = (councils.treasury[v] - keep[v]).max(0.0);
        given[v] = if councils.formed[v] { wanted[v].min(spare) } else { 0.0 };
        share[v] = div_or(given[v], wanted[v], 0.0);
        councils.treasury[v] -= given[v];
    }
    for (h, m) in money.iter_mut().enumerate() {
        *m += shortfall[h] * share[location[h] as usize];
    }
    given
}
