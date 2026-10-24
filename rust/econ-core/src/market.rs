//! Money and markets: families buy with coins, businesses pay wages.
//!
//! All money is held by families and businesses; it changes hands but is
//! never created or destroyed. Each place has its own market and price.

use crate::config::Config;
use crate::economy::Workforce;
use crate::grid::{clip, div_or, sum_by, Grid};
use crate::population::Population;

/// Sum a per-row value for each household.
pub fn by_household(population: &Population, n_households: usize, value: impl Fn(usize) -> f64) -> Vec<f64> {
    sum_by(n_households, &population.household, value)
}

/// Sum a per-household value for each place.
pub fn by_village(values: &[f64], location: &[u32], n_locations: usize) -> Vec<f64> {
    sum_by(n_locations, location, |i| values[i])
}

/// Families with plenty help families who can't afford their food.
///
/// Savings above `sharing_threshold_months` of a family's own food cost are
/// given at `sharing_rate` a month into a village pot, which covers up to
/// `gift_share` of what families are short of for this month's food and
/// firewood, in proportion to the shortfall (the rest they must borrow or
/// raise by selling). Changes `money`; returns the amount shared per place.
pub fn share_with_neighbours(
    money: &mut [f64],
    cost: &[f64],
    location: &[u32],
    n_locations: usize,
    config: &Config,
) -> Vec<f64> {
    let cfg = &config.money;
    let surplus: Vec<f64> =
        money.iter().zip(cost).map(|(&m, &c)| (m - cfg.sharing_threshold_months * c).max(0.0)).collect();
    let shortfall: Vec<f64> = money.iter().zip(cost).map(|(&m, &c)| cfg.gift_share * (c - m).max(0.0)).collect();
    let offered = sum_by(n_locations, location, |i| cfg.sharing_rate * surplus[i]);
    let wanted = by_village(&shortfall, location, n_locations);
    let pot: Vec<f64> = offered.iter().zip(&wanted).map(|(&o, &w)| o.min(w)).collect();
    let give: Vec<f64> = (0..n_locations).map(|v| div_or(pot[v], offered[v], 0.0)).collect();
    let get: Vec<f64> = (0..n_locations).map(|v| div_or(pot[v], wanted[v], 0.0)).collect();
    for (i, m) in money.iter_mut().enumerate() {
        let loc = location[i] as usize;
        *m -= cfg.sharing_rate * surplus[i] * give[loc];
        *m += shortfall[i] * get[loc];
    }
    pot
}

/// One market's outcome.
#[derive(Clone, Debug)]
pub struct Sale {
    pub bought: Vec<f64>, // per buyer
    pub spent: Vec<f64>,  // per buyer
    pub sold: Vec<f64>,   // per place
    pub demand: Vec<f64>, // per place, what buyers asked for at the going price
}

/// Buyers buy up to what they `want` and can afford; if too little is on
/// offer, everyone gets the same share of what they asked for.
///
/// `price` and `offer` are per place, `location` per buyer. Takes the
/// payment out of `money`.
pub fn buy(money: &mut [f64], want: &[f64], price: &[f64], offer: &[f64], location: &[u32]) -> Sale {
    let n = offer.len();
    let asked: Vec<f64> = (0..money.len())
        .map(|i| {
            let p = price[location[i] as usize];
            want[i].min(div_or(money[i], p, 0.0)).max(0.0)
        })
        .collect();
    let demand = by_village(&asked, location, n);
    let filled: Vec<f64> = (0..n).map(|v| 1.0f64.min(div_or(offer[v], demand[v], 1.0))).collect();
    let mut bought = Vec::with_capacity(money.len());
    let mut spent = Vec::with_capacity(money.len());
    for i in 0..money.len() {
        let loc = location[i] as usize;
        let b = asked[i] * filled[loc];
        let s = (b * price[loc]).min(money[i]);
        money[i] -= s;
        bought.push(b);
        spent.push(s);
    }
    let sold = by_village(&bought, location, n);
    Sale { bought, spent, sold, demand }
}

/// Mark-ups rise when buyers want more than is on offer and fall when goods go unsold.
pub fn adjust_markup(markup: &Grid, demand: &Grid, supply: &Grid, config: &Config) -> Grid {
    let cfg = &config.trade;
    let mut out = markup.clone();
    for i in 0..markup.data.len() {
        let (d, s) = (demand.data[i], supply.data[i]);
        let gap = if s <= 0.0 && d > 0.0 { 1.0 } else { div_or(d - s, s, 0.0) }; // (nothing on offer at all: 1)
        let change = clip(cfg.markup_speed * gap, -cfg.max_markup_change, cfg.max_markup_change);
        out.data[i] = clip(markup.data[i] * (1.0 + change), cfg.markup_range.0, cfg.markup_range.1);
    }
    out
}

/// Stock to hold going into each calendar month, in months of average
/// need, so that making the average need every month lasts through the
/// coming season: the largest running shortfall of need over that average,
/// from that month on. Index 0 is January.
pub fn seasonal_buffer(profile: &[f64]) -> Vec<f64> {
    let k = profile.len();
    let mean = profile.iter().sum::<f64>() / k as f64;
    if mean <= 0.0 {
        return vec![0.0; k];
    }
    (0..k)
        .map(|month| {
            let mut running = 0.0;
            let mut most = f64::NEG_INFINITY;
            for j in 0..k {
                running += profile[(month + j) % k] / mean - 1.0;
                most = most.max(running);
            }
            most.max(0.0)
        })
        .collect()
}

/// Move the grain mark-up toward the level set by how short the year's supply is.
pub fn grain_markup(markup: &[f64], cover: &[f64], config: &Config) -> Vec<f64> {
    let food = &config.food;
    let (low, high) = config.trade.markup_range;
    markup
        .iter()
        .zip(cover)
        .map(|(&m, &c)| {
            let target = clip((food.normal_cover / c.max(1e-6)).powf(food.price_elasticity), low, high);
            m + food.price_adjustment * (target - m)
        })
        .collect()
}

/// Each business pays out part of its cash to its workers, by skill and health.
///
/// It first sets aside `keep` (places x businesses), e.g. next month's
/// supplies. `cash` has the wages taken out. Returns (income per household,
/// wages paid per place and business).
pub fn pay_wages(
    cash: &mut Grid,
    population: &Population,
    n_households: usize,
    config: &Config,
    keep: Option<&Grid>,
    force: &Workforce,
) -> (Vec<f64>, Grid) {
    let total = force.by_business(|i| force.effort[i]);
    let mut payout = Grid::zeros(cash.rows, cash.cols);
    let mut rate = vec![0.0; cash.data.len() + 1]; // (nothing for those outside a business)
    for i in 0..cash.data.len() {
        let spare = match keep {
            Some(keep) => (cash.data[i] - keep.data[i]).max(0.0),
            None => cash.data[i],
        };
        let p = if total.data[i] > 0.0 { config.money.wage_payout * spare } else { 0.0 };
        payout.data[i] = p;
        rate[i] = div_or(p, total.data[i], 0.0);
        cash.data[i] -= p;
    }
    let income = by_household(population, n_households, |i| force.effort[i] * rate[force.bucket[i] as usize]);
    (income, payout)
}
