//! Businesses: what they make, what they buy, and who works in them.
//!
//! Each place has one business per trade (farming, woodcutting, ...), made
//! up of everyone working in it. Tables are (places x businesses) or
//! (places x products).

use crate::config::Config;
use crate::grid::{clip, div_or, Grid, Grid3};
use crate::population::{Population, NO_JOB};
use crate::rng::{cumulative, Rng};
use crate::rules::is_working_age;

/// How much of a full day's work each row's people manage, given their health.
#[inline]
pub fn work_factor(health: f64, config: &Config) -> f64 {
    let floor = config.health.work_at_zero_health;
    floor + (1.0 - floor) * health / config.health.maximum
}

/// Whether a row works in a business (not children, the elderly or council staff).
#[inline]
pub fn in_business(job: i16, config: &Config) -> bool {
    job >= 0 && (job as usize) < config.businesses.len()
}

/// Who works in which business and how much, worked out once for a stretch
/// of the month in which nobody joins, leaves or changes trade.
#[derive(Clone, Debug)]
pub struct Workforce {
    /// Per row: place x businesses + job; rows outside a business: the last bucket.
    pub bucket: Vec<u32>,
    /// Per row: count x skill (at full health).
    pub skill_effort: Vec<f64>,
    /// Per row: count x skill x how much their health lets them work.
    pub effort: Vec<f64>,
    pub n_locations: usize,
    pub n_business: usize,
}

impl Workforce {
    #[inline]
    pub fn working(&self, row: usize) -> bool {
        (self.bucket[row] as usize) < self.n_locations * self.n_business
    }

    /// Sum a per-row value for each business (places x businesses).
    pub fn by_business(&self, value: impl Fn(usize) -> f64) -> Grid {
        let size = self.n_locations * self.n_business;
        let mut sums = vec![0.0; size + 1];
        for (i, &b) in self.bucket.iter().enumerate() {
            sums[b as usize] += value(i);
        }
        sums.truncate(size);
        Grid { rows: self.n_locations, cols: self.n_business, data: sums }
    }
}

pub fn workforce(population: &Population, n_locations: usize, config: &Config) -> Workforce {
    let n_business = config.businesses.len();
    let outside = (n_locations * n_business) as u32;
    let mut bucket = Vec::with_capacity(population.len());
    let mut skill_effort = Vec::with_capacity(population.len());
    let mut effort = Vec::with_capacity(population.len());
    for i in 0..population.len() {
        let job = population.job[i];
        bucket.push(if in_business(job, config) {
            population.location[i] * n_business as u32 + job as u32
        } else {
            outside
        });
        let se = population.count[i] as f64 * population.skill[i];
        skill_effort.push(se);
        effort.push(se * work_factor(population.health[i], config));
    }
    Workforce { bucket, skill_effort, effort, n_locations, n_business }
}

/// Workers in each business (places x businesses).
pub fn headcount(population: &Population, n_locations: usize, config: &Config, force: Option<&Workforce>) -> Grid {
    match force {
        Some(force) => force.by_business(|i| population.count[i] as f64),
        None => workforce(population, n_locations, config).by_business(|i| population.count[i] as f64),
    }
}

/// Effective workers in each business: headcount x skill x health (x luck).
///
/// Without an `rng` there are no good or bad days; `at_full_health` ignores
/// how weak workers are right now (both used for estimates).
pub fn labor(
    population: &Population,
    force: &Workforce,
    config: &Config,
    rng: Option<&mut Rng>,
    at_full_health: bool,
) -> Grid {
    let effort = if at_full_health { &force.skill_effort } else { &force.effort };
    match rng {
        None => force.by_business(|i| effort[i]),
        Some(rng) => {
            // (luck only matters to those at work, so only they draw)
            let noise = config.food.output_noise;
            let lucky: Vec<f64> = (0..population.len())
                .map(|i| {
                    if force.working(i) {
                        let sd = noise / (population.count[i].max(1) as f64).sqrt();
                        effort[i] * (1.0 + rng.normal(0.0, sd)).max(0.0)
                    } else {
                        0.0
                    }
                })
                .collect();
            force.by_business(|i| lucky[i])
        }
    }
}

/// Work each household puts into one business: headcount x skill x health.
pub fn household_effort(population: &Population, business: usize, n_households: usize, force: &Workforce) -> Vec<f64> {
    let mut effort = vec![0.0; n_households];
    for i in 0..population.len() {
        if force.working(i) && population.job[i] as usize == business {
            effort[population.household[i] as usize] += force.effort[i];
        }
    }
    effort
}

/// Output multiplier from tools: 1 + boost x share of workers with a tool.
pub fn tool_factor(tools: &Grid, workers: &Grid, config: &Config) -> Grid {
    let mut out = Grid::zeros(tools.rows, tools.cols);
    for r in 0..tools.rows {
        for (b, spec) in config.businesses.iter().enumerate() {
            let coverage = clip(div_or(tools.get(r, b), workers.get(r, b), 1.0), 0.0, 1.0);
            out.set(r, b, 1.0 + spec.tool_boost * coverage);
        }
    }
    out
}

/// What each business could make in an average month, before supplies run short.
///
/// Farming has diminishing returns on the land in use (`land_in_use`) that
/// was sown (the share of the seed `sown`), and its output (gross of seed)
/// is scaled by `farm_mult` per place (plough animals); other trades make
/// the same amount per worker however many there are.
#[allow(clippy::too_many_arguments)]
pub fn capacity(
    labor_now: &Grid,
    tools: &Grid,
    workers: &Grid,
    land: &[f64],
    production_mult: &Grid,
    config: &Config,
    farm_mult: Option<&[f64]>,
    sown: Option<&[f64]>,
) -> Grid {
    let tf = tool_factor(tools, workers, config);
    let a = config.food.labor_share;
    let farm = config.farming();
    let farmed = farm_land_with(land, &tf, workers, config, farm_mult);
    let mut made = Grid::zeros(labor_now.rows, labor_now.cols);
    for r in 0..made.rows {
        for (b, spec) in config.businesses.iter().enumerate() {
            made.set(r, b, spec.output * tf.get(r, b) * production_mult.get(r, b) * labor_now.get(r, b));
        }
        let boost = tf.get(r, farm) * farm_mult.map_or(1.0, |m| m[r]);
        // plots left unsown: the farmers work the rest harder
        let plots = farmed[r] * sown.map_or(1.0, |s| s[r]);
        made.set(
            r,
            farm,
            config.businesses[farm].output
                * boost
                * production_mult.get(r, farm)
                * labor_now.get(r, farm).powf(a)
                * plots.powf(1.0 - a),
        );
    }
    made
}

/// Plots each place farms, given its farmers, tools and `farm_mult` (plough animals).
pub fn farm_land(land: &[f64], tools: &Grid, workers: &Grid, config: &Config, farm_mult: Option<&[f64]>) -> Vec<f64> {
    farm_land_with(land, &tool_factor(tools, workers, config), workers, config, farm_mult)
}

fn farm_land_with(land: &[f64], tf: &Grid, workers: &Grid, config: &Config, farm_mult: Option<&[f64]>) -> Vec<f64> {
    let farm = config.farming();
    let output = config.businesses[farm].output;
    (0..land.len())
        .map(|r| {
            let boost = tf.get(r, farm) * farm_mult.map_or(1.0, |m| m[r]);
            land_in_use_one(land[r], workers.get(r, farm), config, 12.0 * output * boost)
        })
        .collect()
}

/// Plots worth farming: all the place's land, unless there are so few
/// farmers that the last plots would not repay their seed. Farmers stop
/// adding plots where a plot's share of the harvest, (1 - labour share) x
/// output per plot, falls to the seed it takes; `per_plot_output` is the
/// yearly output per plot of one farmer on one plot (with tools and animals).
#[inline]
pub fn land_in_use_one(land: f64, farmers: f64, config: &Config, per_plot_output: f64) -> f64 {
    let a = config.food.labor_share;
    let worth = ((1.0 - a) * per_plot_output / config.food.seed_per_plot).max(1e-9).powf(1.0 / a);
    land.min(farmers * worth)
}

pub fn land_in_use(land: &[f64], farmers: &[f64], config: &Config, per_plot_output: &[f64]) -> Vec<f64> {
    (0..land.len()).map(|r| land_in_use_one(land[r], farmers[r], config, per_plot_output[r])).collect()
}

/// Units of each product used per unit made (businesses x products).
pub fn input_needs(config: &Config) -> Grid {
    let mut needs = Grid::zeros(config.businesses.len(), config.products.len());
    for (b, spec) in config.businesses.iter().enumerate() {
        for (product, per_unit) in &spec.inputs {
            needs.set(b, config.product_index(product), *per_unit);
        }
    }
    needs
}

/// Cut planned output to what the supplies on hand allow, and use them up.
/// `supplies` is (places x businesses x products).
pub fn limit_by_supplies(planned: &Grid, supplies: &mut Grid3, config: &Config) -> Grid {
    let per_unit = input_needs(config);
    let mut made = planned.clone();
    for r in 0..planned.rows {
        for b in 0..planned.cols {
            let mut possible = f64::INFINITY;
            for p in 0..per_unit.cols {
                let need = per_unit.get(b, p);
                if need > 0.0 {
                    possible = possible.min(supplies.get(r, b, p) / need);
                }
            }
            let m = planned.get(r, b).min(possible);
            made.set(r, b, m);
            for p in 0..per_unit.cols {
                let s = supplies.at(r, b, p);
                *s = (*s - m * per_unit.get(b, p)).max(0.0); // (rounding dust)
            }
        }
    }
    made
}

/// Index of the product each business makes.
pub fn product_of(config: &Config) -> Vec<usize> {
    config.businesses.iter().map(|b| config.product_index(&b.product)).collect()
}

/// Index of the business that makes each product (None if none does).
pub fn seller_of(config: &Config) -> Vec<Option<usize>> {
    let mut seller = vec![None; config.products.len()];
    for (b, &p) in product_of(config).iter().enumerate() {
        seller[p] = Some(b);
    }
    seller
}

/// Normal output per worker in each business: full health, no events, an
/// average month; for farming, net of the seed its plots need. For a trade
/// nobody works in, what a first worker would make.
#[allow(clippy::too_many_arguments)]
pub fn productivity(
    population: &Population,
    force: &Workforce,
    tools: &Grid,
    workers: &Grid,
    land: &[f64],
    config: &Config,
    farm_mult: Option<&[f64]>,
    sown: Option<&[f64]>,
) -> Grid {
    let full = labor(population, force, config, None, true);
    productivity_from(&full, tools, workers, land, config, farm_mult, sown)
}

/// `productivity` given the effort at full health (`full`).
pub fn productivity_from(
    full: &Grid,
    tools: &Grid,
    workers: &Grid,
    land: &[f64],
    config: &Config,
    farm_mult: Option<&[f64]>,
    sown: Option<&[f64]>,
) -> Grid {
    let n = land.len();
    let nb = config.businesses.len();
    let normal = Grid::new(n, nb, 1.0);
    let farm = config.farming();
    let mut made = capacity(full, tools, workers, land, &normal, config, farm_mult, sown);
    let farmed = farm_land(land, tools, workers, config, farm_mult);
    let no_tools = Grid::zeros(n, nb);
    let mut first = capacity(&normal, &no_tools, &normal, land, &normal, config, farm_mult, sown);
    let first_farmed = farm_land(land, &no_tools, &normal, config, farm_mult);
    for r in 0..n {
        made.set(r, farm, net_of_seed(made.get(r, farm), farmed[r], config));
        first.set(r, farm, net_of_seed(first.get(r, farm), first_farmed[r], config));
    }
    let mut out = Grid::zeros(n, nb);
    for r in 0..n {
        for b in 0..nb {
            let w = workers.get(r, b);
            out.set(r, b, if w >= 1.0 { made.get(r, b) / w } else { first.get(r, b) });
        }
    }
    out
}

/// Monthly farm output less the seed its plots take (never below a tenth of it).
#[inline]
fn net_of_seed(gross: f64, farmed: f64, config: &Config) -> f64 {
    (gross - config.food.seed_per_plot * farmed / 12.0).max(0.1 * gross)
}

/// What each business pays for the supplies of one unit it makes, at
/// `prices` (places x businesses).
pub fn supplies_cost(prices: &Grid, config: &Config) -> Grid {
    let needs = input_needs(config);
    let mut cost = Grid::zeros(prices.rows, needs.rows);
    for r in 0..prices.rows {
        for b in 0..needs.rows {
            cost.set(r, b, (0..needs.cols).map(|p| needs.get(b, p) * prices.get(r, p)).sum());
        }
    }
    cost
}

/// Cost of each product: the going wage over output per worker, plus supplies.
/// `wage` is per place; `prices` (last month's) value the supplies.
pub fn fair_prices(wage: &[f64], per_worker: &Grid, prices: &Grid, config: &Config) -> Grid {
    let supplies = supplies_cost(prices, config);
    let product = product_of(config);
    let mut fair = Grid::zeros(prices.rows, prices.cols);
    for r in 0..prices.rows {
        for (b, &p) in product.iter().enumerate() {
            fair.set(r, p, wage[r] / per_worker.get(r, b).max(1e-9) + supplies.get(r, b));
        }
    }
    fair
}

/// What a worker could earn in each trade: their output's value less supplies.
pub fn potential_pay(prices: &Grid, per_worker: &Grid, config: &Config) -> Grid {
    let cost = supplies_cost(prices, config);
    let product = product_of(config);
    let mut pay = Grid::zeros(per_worker.rows, per_worker.cols);
    for r in 0..pay.rows {
        for (b, &p) in product.iter().enumerate() {
            pay.set(r, b, per_worker.get(r, b) * (prices.get(r, p) - cost.get(r, b)));
        }
    }
    pay
}

/// How many workers each trade should have, given the work its orders need.
///
/// Trades making necessities get what they need first (shared out if there
/// aren't enough workers); comfort trades share whoever is left, in
/// proportion to their orders (or farming takes them if nobody wants
/// comforts). `work_needed` is in workers: orders over output per worker.
pub fn wanted_workers(work_needed: &Grid, workers: &Grid, config: &Config) -> Grid {
    let product = product_of(config);
    let necessity: Vec<bool> = product.iter().map(|&p| config.products[p].use_ != "comfort").collect();
    let farm = config.farming();
    let nb = necessity.len();
    let mut wanted = Grid::zeros(work_needed.rows, nb);
    for r in 0..work_needed.rows {
        let total: f64 = workers.row(r).iter().sum();
        let essential: f64 = (0..nb).filter(|&b| necessity[b]).map(|b| work_needed.get(r, b)).sum();
        let scale = 1.0f64.min(div_or(total, essential, 1.0));
        let mut given = 0.0;
        for b in 0..nb {
            if necessity[b] {
                let w = work_needed.get(r, b) * scale;
                wanted.set(r, b, w);
                given += w;
            }
        }
        let left = total - given;
        let comfort_total: f64 = (0..nb).filter(|&b| !necessity[b]).map(|b| work_needed.get(r, b)).sum();
        for b in 0..nb {
            let mut share =
                if !necessity[b] && comfort_total > 0.0 { work_needed.get(r, b) / comfort_total } else { 0.0 };
            if comfort_total <= 0.0 && b == farm {
                share = 1.0;
            }
            *wanted.at(r, b) += share * left;
        }
    }
    wanted
}

/// Trades with more workers than they want let some go; they join trades
/// that are short, in proportion to how short. Returns how many moved.
pub fn move_workers(population: &mut Population, wanted: &Grid, config: &Config, rng: &mut Rng) -> u64 {
    let n = wanted.rows;
    let nb = wanted.cols;
    let workers = headcount(population, n, config, None);
    let surplus = workers.zip_map(wanted, |w, x| (w - x).max(0.0));
    let shortfall = wanted.zip_map(&workers, |x, w| (x - w).max(0.0));
    let rate = config.trade.hiring_rate;
    let mut leaving = vec![0u32; population.len()];
    let mut any = 0u64;
    for i in 0..population.len() {
        let job = population.job[i];
        if !in_business(job, config) {
            continue;
        }
        let loc = population.location[i] as usize;
        let chance = clip(div_or(rate * surplus.get(loc, job as usize), workers.get(loc, job as usize), 0.0), 0.0, 1.0);
        leaving[i] = rng.binomial(population.count[i] as u64, chance) as u32;
        any += leaving[i] as u64;
    }
    if any == 0 {
        return 0;
    }
    let rows = population.split(&leaving);
    // Each joins a trade of its place at random, in proportion to how short
    // it is (those whose place has no trade short stay where they are).
    for &row in &rows {
        let short = shortfall.row(population.location[row] as usize);
        let total: f64 = short.iter().sum();
        let u = rng.random();
        if total > 0.0 {
            let mut running = 0.0;
            let mut pick = 0;
            for &s in short {
                running += s;
                if u >= running / total {
                    pick += 1;
                }
            }
            population.job[row] = pick.min(nb - 1) as i16;
        }
    }
    any
}

/// Young people coming of age join the trade most short of workers (never
/// one `closed` to them there, places x businesses: farming in a town); the
/// old retire.
pub fn assign_new_workers(population: &mut Population, wanted: &Grid, config: &Config, closed: Option<&[Vec<bool>]>) {
    let n = wanted.rows;
    let mut any_new = false;
    for i in 0..population.len() {
        if population.job[i] == NO_JOB && is_working_age(population, i, config) {
            any_new = true;
            break;
        }
    }
    if any_new {
        let workers = headcount(population, n, config, None);
        let short = |r: usize, b: usize| {
            if closed.is_some_and(|c| c[r][b]) {
                f64::NEG_INFINITY
            } else {
                (wanted.get(r, b) - workers.get(r, b)) / wanted.get(r, b).max(1.0)
            }
        };
        // The most short (the first of equals, as numpy's argmax).
        let best: Vec<usize> = (0..n)
            .map(|r| (1..wanted.cols).fold(0, |best, b| if short(r, b) > short(r, best) { b } else { best }))
            .collect();
        for i in 0..population.len() {
            if population.job[i] == NO_JOB && is_working_age(population, i, config) {
                population.job[i] = best[population.location[i] as usize] as i16;
            }
        }
    }
    for i in 0..population.len() {
        if !is_working_age(population, i, config) {
            population.job[i] = NO_JOB;
        }
    }
}

/// Spread the starting workers over the trades by their starting shares
/// (in places marked `no_farming`, the towns, over the other trades).
pub fn assign_starting_jobs(population: &mut Population, config: &Config, rng: &mut Rng, no_farming: Option<&[bool]>) {
    let mut shares: Vec<f64> = config.businesses.iter().map(|b| b.initial_share).collect();
    let total: f64 = shares.iter().sum();
    let nb = shares.len();
    if total > 0.0 {
        shares.iter_mut().for_each(|s| *s /= total);
    } else {
        shares = vec![1.0 / nb as f64; nb];
    }
    let all = cumulative(&shares);
    let mut crafts = shares.clone();
    crafts[config.farming()] = 0.0;
    let towns_trades = cumulative(&crafts);
    let crafts_total: f64 = crafts.iter().sum();
    for i in 0..population.len() {
        population.job[i] = NO_JOB;
        if is_working_age(population, i, config) {
            population.job[i] = rng.choose_weighted(&all) as i16;
        }
    }
    if let Some(town) = no_farming {
        if crafts_total > 0.0 {
            for i in 0..population.len() {
                if is_working_age(population, i, config) && town[population.location[i] as usize] {
                    population.job[i] = rng.choose_weighted(&towns_trades) as i16;
                }
            }
        }
    }
}
