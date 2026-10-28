//! The monthly rules of the village economy: food need, rationing, health,
//! deaths, births and ageing.
//!
//! Each rule works on whole columns of the population table and on
//! headcounts: "how many of this row's people die" is one binomial draw,
//! which is the same as rolling once per person.

use crate::config::{Config, SkillConfig};
use crate::grid::{clip, Grid};
use crate::population::{Population, NO_JOB};
use crate::rng::Rng;

/// Married people whose husband or wife is still living (per row). In a
/// row standing for many people, married people count as couples.
pub fn spouse_alive(population: &Population) -> Vec<bool> {
    // Living rows of each couple, at couple + 1 (slot 0 gathers the rows of
    // no couple; what it holds is never used). Written without branches:
    // whether a row is married or alive is as good as random from row to row.
    let largest = population.couple.iter().cloned().max().unwrap_or(-1);
    let mut living = vec![0u32; (largest + 2).max(1) as usize];
    for (&c, &count) in population.couple.iter().zip(&population.count) {
        let slot = (c + 1).max(0) as usize;
        living[slot] = living[slot].wrapping_add((count > 0) as u32);
    }
    (0..population.len())
        .map(|i| {
            let c = population.couple[i];
            let by_couple = living[(c + 1).max(0) as usize] >= 2;
            let alive = (c >= 0) & by_couple | (c < 0) & (population.count[i] > 1);
            alive & population.married[i]
        })
        .collect()
}

pub fn monthly_chance(annual: f64) -> f64 {
    1.0 - (1.0 - annual).powf(1.0 / 12.0)
}

/// Sum a per-row value for each place.
pub fn by_location(population: &Population, n_locations: usize, value: impl Fn(usize) -> f64) -> Vec<f64> {
    crate::grid::sum_by(n_locations, &population.location, value)
}

/// People in each place.
pub fn people_by_location(population: &Population, n_locations: usize) -> Vec<f64> {
    by_location(population, n_locations, |i| population.count[i] as f64)
}

/// Average skill of a row of `count` people: larger groups average out
/// closer to the mean.
pub fn draw_skill(count: u32, cfg: &SkillConfig, rng: &mut Rng) -> f64 {
    let sd = cfg.sd / (count.max(1) as f64).sqrt();
    clip(rng.normal(cfg.mean, sd), cfg.minimum, cfg.maximum)
}

/// Rations each row needs this month.
pub fn food_need(population: &Population, config: &Config) -> Vec<f64> {
    let adult = config.demography.adult_age as i32;
    (0..population.len())
        .map(|i| {
            let per_person =
                if population.age_years(i) < adult { config.food.child_need } else { config.food.adult_need };
            population.count[i] as f64 * per_person
        })
        .collect()
}

pub fn is_working_age(population: &Population, row: usize, config: &Config) -> bool {
    population.working_age(row, config.demography.adult_age, config.demography.retirement_age)
}

/// Output multiplier for each calendar month (index 0 = January), averaging 1.
pub fn season_factors(config: &Config) -> Vec<f64> {
    let s = &config.food.seasonality;
    let mean = s.iter().sum::<f64>() / s.len() as f64;
    s.iter().map(|v| v / mean).collect()
}

/// Food each place expects to grow in each of the coming months: what
/// today's workers grow in normal health, by season, times the effect of
/// events already under way for as long as they last (places x months).
pub fn harvest_outlook(normal_capacity: &[f64], event_outlook: &Grid, month_of_year: i64, config: &Config) -> Grid {
    let season = season_factors(config);
    let mut out = Grid::zeros(event_outlook.rows, event_outlook.cols);
    for r in 0..out.rows {
        for m in 0..out.cols {
            let upcoming = ((month_of_year as usize) + m) % 12;
            out.set(r, m, normal_capacity[r] * season[upcoming] * event_outlook.get(r, m));
        }
    }
    out
}

/// Share of the full ration one family (or place) allows itself this month:
/// the largest steady ration that keeps it from running out over the
/// coming months, counting its `stock` (which keeps spoiling) and the
/// harvests `outlook(month)` (next month first), capped at `most`.
/// See `plan_ration`.
#[inline]
pub fn plan_ration_one(
    stock: f64,
    need: f64,
    months: usize,
    outlook: impl Fn(usize) -> f64,
    config: &Config,
    most: f64,
) -> f64 {
    if config.food.planning_months <= 0 {
        return 1.0;
    }
    let keep = 1.0 - config.food.spoilage;
    let mut available = stock;
    let mut required = need;
    let mut affordable = if required > 0.0 { available / required } else { 1.0 };
    for month in 0..months {
        available = available * keep + outlook(month);
        required = required * keep + need;
        let ratio = if required > 0.0 { available / required } else { 1.0 };
        affordable = affordable.min(ratio);
    }
    clip(affordable, 0.0, most)
}

/// Share of the full ration each place allows itself this month.
///
/// The place eats the largest steady ration that keeps it from running out
/// over the coming months, counting the stock (which keeps spoiling) and the
/// harvests in `outlook` (one column per coming month). When a poor harvest
/// leaves the granary short, it tightens belts early and spreads the
/// shortage instead of eating well until it is empty. The result is capped
/// at `most` (above 1: how much more than needed could safely be sold).
pub fn plan_ration(stock: &[f64], need: &[f64], outlook: &Grid, config: &Config, most: f64) -> Vec<f64> {
    (0..need.len())
        .map(|r| plan_ration_one(stock[r], need[r], outlook.cols, |m| outlook.get(r, m), config, most))
        .collect()
}

/// The smallest store that lets one family eat its full `need` this month
/// and every coming month, given its expected harvest income `outlook(month)`,
/// allowing for spoilage: anything beyond it is spare. The inverse of
/// `plan_ration` at a full ration.
#[inline]
pub fn stock_to_keep_one(need: f64, months: usize, outlook: impl Fn(usize) -> f64, config: &Config) -> f64 {
    let keep = 1.0 - config.food.spoilage;
    let mut smallest = need;
    if config.food.planning_months <= 0 {
        return smallest;
    }
    let mut income = 0.0;
    let mut required = need;
    let mut kept = 1.0;
    for month in 0..months {
        income = income * keep + outlook(month);
        required = required * keep + need;
        kept *= keep;
        smallest = smallest.max((required - income) / kept);
    }
    smallest.max(0.0)
}

pub fn stock_to_keep(need: &[f64], outlook: &Grid, config: &Config) -> Vec<f64> {
    (0..need.len()).map(|r| stock_to_keep_one(need[r], outlook.cols, |m| outlook.get(r, m), config)).collect()
}

/// Plan rations knowing that hungry workers grow less.
///
/// `normal_outlook` assumes workers in full health. A smaller ration weakens
/// them and shrinks future harvests, so re-plan with the work they could do
/// on the planned ration, a few times until the plan and the harvest agree.
pub fn plan_ration_realistically(
    stock: &[f64],
    need: &[f64],
    normal_outlook: &Grid,
    config: &Config,
    rounds: usize,
    most: f64,
) -> Vec<f64> {
    let mut ration = plan_ration(stock, need, normal_outlook, config, most);
    for _ in 0..rounds {
        ration = (0..need.len())
            .map(|r| {
                let effort = expected_work_factor(ration[r].min(1.0), config).powf(config.food.labor_share);
                plan_ration_one(
                    stock[r],
                    need[r],
                    normal_outlook.cols,
                    |m| normal_outlook.get(r, m) * effort,
                    config,
                    most,
                )
            })
            .collect();
    }
    ration
}

/// The health people settle at on a given share of their food need.
#[inline]
pub fn target_health(food_share: f64, config: &Config) -> f64 {
    let cfg = &config.health;
    let starving = cfg.starvation_ration;
    cfg.maximum * clip((food_share - starving) / (1.0 - starving), 0.0, 1.0)
}

/// Work output, as a share of full, of people settled on a given ration.
#[inline]
pub fn expected_work_factor(food_share: f64, config: &Config) -> f64 {
    let floor = config.health.work_at_zero_health;
    floor + (1.0 - floor) * target_health(food_share, config) / config.health.maximum
}

/// Take a share `rate` of each store as spoiled; returns what spoiled.
pub fn spoil(granary: &mut [f64], rate: f64) -> Vec<f64> {
    granary
        .iter_mut()
        .map(|g| {
            let spoiled = *g * rate;
            *g -= spoiled;
            spoiled
        })
        .collect()
}

/// Health drifts toward a level set by how well people eat; events and cold
/// add their toll. `food_share(row)` and `health_delta(row)` are per row.
/// Full rations lead to full health; `starvation_ration` or less leads to 0.
/// Health rises at most `recovery` a month and falls at most `hunger_damage`
/// times the share of food missing, so mild shortages wear people down slowly.
pub fn update_health(
    population: &mut Population,
    food_share: impl Fn(usize) -> f64,
    health_delta: impl Fn(usize) -> f64,
    config: &Config,
) {
    let cfg = &config.health;
    for i in 0..population.len() {
        let share = food_share(i);
        let gap = target_health(share, config) - population.health[i];
        let change = if gap > 0.0 { gap.min(cfg.recovery) } else { gap.max(-cfg.hunger_damage * (1.0 - share)) };
        population.health[i] = clip(population.health[i] + change + health_delta(i), 0.0, cfg.maximum);
    }
}

/// The monthly chance of dying at each age band (from `annual_mortality`).
pub struct MortalityTable {
    from_ages: Vec<i32>,
    rates: Vec<f64>,
    by_age: Vec<f64>, // the rate at each age up to the last band's first
}

impl MortalityTable {
    pub fn new(config: &Config) -> MortalityTable {
        let table = &config.demography.annual_mortality;
        let mut mortality = MortalityTable {
            from_ages: table.iter().map(|&(age, _)| age as i32).collect(),
            rates: table.iter().map(|&(_, rate)| monthly_chance(rate)).collect(),
            by_age: Vec::new(),
        };
        let last = mortality.from_ages.last().copied().unwrap_or(0).max(0);
        mortality.by_age = (0..=last).map(|age| mortality.band_rate(age)).collect();
        mortality
    }

    fn band_rate(&self, age_years: i32) -> f64 {
        let band = self.from_ages.partition_point(|&a| a <= age_years);
        // (below the first band numpy wraps to the last; ages start at 0, so never)
        self.rates[if band == 0 { self.rates.len() - 1 } else { band - 1 }]
    }

    #[inline]
    pub fn rate(&self, age_years: i32) -> f64 {
        match self.by_age.get(age_years as usize) {
            Some(&rate) if age_years >= 0 => rate,
            _ => self.band_rate(age_years),
        }
    }
}

/// Monthly chance of dying for each row: age risk (scaled by events) plus
/// hunger risk, both scaled by `care` (per row; below 1 for people treated).
pub fn death_chance(
    population: &Population,
    mortality_mult: &[f64],
    config: &Config,
    care: Option<&[f64]>,
) -> Vec<f64> {
    let table = MortalityTable::new(config);
    let cfg = &config.health;
    (0..population.len())
        .map(|i| {
            let age_risk = table.rate(population.age_years(i)) * mortality_mult[population.location[i] as usize];
            let weakness = clip((cfg.danger_threshold - population.health[i]) / cfg.danger_threshold, 0.0, 1.0);
            let mut risk = age_risk + cfg.max_extra_mortality * weakness * weakness;
            if let Some(care) = care {
                risk *= care[i];
            }
            clip(risk, 0.0, 1.0)
        })
        .collect()
}

/// The oldest age a life table tells apart (in years): older people count with it.
pub const OLDEST: usize = 100;

/// People of each age in years (to `OLDEST`) and the deaths their monthly
/// chances make likely, for a life table (`metrics::life_table`); only
/// read, never used by a rule.
#[derive(Clone, Debug, PartialEq)]
pub struct AtRisk {
    pub people: Vec<f64>,
    pub dying: Vec<f64>,
}

impl Default for AtRisk {
    fn default() -> AtRisk {
        AtRisk { people: vec![0.0; OLDEST + 1], dying: vec![0.0; OLDEST + 1] }
    }
}

/// The people who die this month leave their rows (emptied rows are removed
/// at the end of the month); returns deaths per place, and adds the people
/// of each age and their chances of dying to `at_risk`.
pub fn deaths(
    population: &mut Population,
    mortality_mult: &[f64],
    config: &Config,
    rng: &mut Rng,
    n_locations: usize,
    care: Option<&[f64]>,
    at_risk: &mut AtRisk,
) -> Vec<f64> {
    let chance = death_chance(population, mortality_mult, config, care);
    for (i, &p) in chance.iter().enumerate() {
        let age = (population.age_years(i).max(0) as usize).min(OLDEST);
        let count = population.count[i] as f64;
        at_risk.people[age] += count;
        at_risk.dying[age] += count * p;
    }
    let mut by_village = vec![0.0; n_locations];
    for (i, &p) in chance.iter().enumerate() {
        let died = rng.binomial(population.count[i] as u64, p) as u32;
        if died > 0 {
            by_village[population.location[i] as usize] += died as f64;
            population.count[i] -= died;
        }
    }
    by_village
}

/// Food a typical month's pay buys, over the place's food need per worker.
pub fn wage_cover(average_pay: &[f64], food_price: &[f64], need: &[f64], workers: &[f64]) -> Vec<f64> {
    (0..need.len())
        .map(|i| {
            let bought = if food_price[i] > 0.0 { average_pay[i] / food_price[i] } else { 0.0 };
            let per_worker = if workers[i] > 0.0 { need[i] / workers[i] } else { f64::INFINITY };
            if per_worker.is_finite() && per_worker > 0.0 {
                bought / per_worker
            } else {
                0.0
            }
        })
        .collect()
}

/// How much being able to feed a family encourages people to marry.
pub fn marriage_factor(cover: &[f64], config: &Config) -> Vec<f64> {
    let demo = &config.demography;
    let (low, high) = demo.wage_cover_for_marriage;
    cover
        .iter()
        .map(|&c| {
            let room = clip((c - low) / (high - low), 0.0, 1.0);
            demo.crowded_marriage_factor + (1.0 - demo.crowded_marriage_factor) * room
        })
        .collect()
}

/// Add this month's newborns; returns births per place.
///
/// Married women of fertile age whose husband is alive give birth with
/// `annual_birth_chance` times their age's `fertility_by_age`, lowered by
/// poor health and scaled by `fertility_mult` per place (from events).
/// Newborns join their mother's household, with her health and a fresh
/// skill draw.
pub fn births(
    population: &mut Population,
    fertility_mult: &[f64],
    config: &Config,
    rng: &mut Rng,
    n_locations: usize,
) -> Vec<f64> {
    let demo = &config.demography;
    let (youngest, oldest) = demo.fertile_ages;
    let (low, high) = config.health.fertility_health;
    let from_ages: Vec<i64> = demo.fertility_by_age.iter().map(|&(a, _)| a).collect();
    // Each age band's monthly chance, worked out once.
    let band_chance: Vec<f64> =
        demo.fertility_by_age.iter().map(|&(_, by_age)| monthly_chance(demo.annual_birth_chance * by_age)).collect();
    let partnered = spouse_alive(population);
    let mut by_village = vec![0.0; n_locations];
    let mut mothers: Vec<(usize, u32)> = Vec::new();
    for i in 0..population.len() {
        let age = population.age_years(i) as i64;
        // (tested without branching: see households::marry)
        if !(population.female[i] & (age >= youngest) & (age <= oldest) & (population.count[i] > 0) & partnered[i]) {
            continue;
        }
        let band = from_ages.partition_point(|&a| a <= age).max(1) - 1;
        let health_factor = clip((population.health[i] - low) / (high - low), 0.0, 1.0);
        let chance = band_chance[band] * health_factor * fertility_mult[population.location[i] as usize];
        let babies = rng.binomial(population.count[i] as u64, chance) as u32;
        if babies > 0 {
            by_village[population.location[i] as usize] += babies as f64;
            mothers.push((i, babies));
        }
    }
    let girls: Vec<u32> =
        mothers.iter().map(|&(_, babies)| rng.binomial(babies as u64, demo.female_share_at_birth) as u32).collect();
    // Girls first, then boys, as in Python.
    for (girl, k) in [(true, 0), (false, 1)] {
        for (m, &(mother, babies)) in mothers.iter().enumerate() {
            let count = if k == 0 { girls[m] } else { babies - girls[m] };
            if count == 0 {
                continue;
            }
            let skill = draw_skill(count, &config.skill, rng);
            population.count.push(count);
            population.age_months.push(0);
            population.female.push(girl);
            population.health.push(population.health[mother]);
            population.skill.push(skill);
            population.location.push(population.location[mother]);
            population.household.push(population.household[mother]);
            population.job.push(NO_JOB);
            population.married.push(false);
            population.couple.push(-1);
        }
    }
    by_village
}

pub fn grow_older(population: &mut Population) {
    for age in population.age_months.iter_mut() {
        *age += 1;
    }
}
