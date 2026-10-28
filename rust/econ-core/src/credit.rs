//! Credit and distress: loans, repayment, foreclosure and land sales.
//!
//! In a bad year families short of coins for their food borrow from
//! families with coins to spare, at high interest. Loans are pooled per
//! place: each borrower owes the place's lenders, and each lender holds a
//! claim on the pool, so every coin repaid (or every animal and plot
//! seized) is shared out among the lenders by their claims. A borrower whose
//! debt outgrows what their animals and land are worth loses them to the
//! lenders; families still short sell land outright, and when many must
//! sell at once its price collapses.
//!
//! Debts and claims are in coins: `households.debt` (owed) and
//! `households.lent` (claims). In every place they add up to the same total.

use crate::config::Config;
use crate::finance::Lenders;
use crate::grid::{clip, div_or};
use crate::households::Households;
use crate::market::by_village;

/// A starting point in debt: in each place the biggest fifth of the
/// landholding families (at least one) are the lenders, and every other
/// landholding family owes them `months` of the place's customary `wage`,
/// their claims in proportion to their land.
pub fn start_in_debt(households: &mut Households, wage: &[f64], months: f64) {
    let n = wage.len();
    let mut landed: Vec<Vec<usize>> = vec![Vec::new(); n];
    for h in 0..households.len() {
        if households.land[h] > 0.0 {
            landed[households.location[h] as usize].push(h);
        }
    }
    for (v, families) in landed.iter_mut().enumerate() {
        families.sort_by(|&a, &b| households.land[b].total_cmp(&households.land[a]));
        let lenders = families.len().div_ceil(5);
        if families.len() < 2 {
            continue;
        }
        let (lending, owing) = families.split_at(lenders);
        let owed = months * wage[v] * owing.len() as f64;
        let land: f64 = lending.iter().map(|&h| households.land[h]).sum();
        for &h in owing {
            households.debt[h] += months * wage[v];
        }
        for &h in lending {
            households.lent[h] += owed * households.land[h] / land;
        }
    }
}

/// Each household's share of its place's total, by `weights`.
pub fn pro_rata(weights: &[f64], totals: &[f64], location: &[u32]) -> Vec<f64> {
    let village = by_village(weights, location, totals.len());
    (0..weights.len())
        .map(|h| {
            let v = location[h] as usize;
            div_or(weights[h], village[v], 0.0) * totals[v]
        })
        .collect()
}

/// Share out each place's `totals` to its households, the largest
/// `capacity` first, each taking up to its capacity: the richest buyers and
/// the biggest creditors get the land and animals that change hands.
pub fn biggest_first(capacity: &[f64], totals: &[f64], location: &[u32]) -> Vec<f64> {
    let mut got = vec![0.0; capacity.len()];
    if !totals.iter().any(|&t| t > 0.0) {
        return got;
    }
    let mut able: Vec<Vec<usize>> = vec![Vec::new(); totals.len()];
    for h in 0..capacity.len() {
        if capacity[h] > 0.0 && totals[location[h] as usize] > 0.0 {
            able[location[h] as usize].push(h);
        }
    }
    for (village, order) in able.iter_mut().enumerate() {
        if order.is_empty() {
            continue;
        }
        // the largest first, ties in order
        order.sort_by(|&a, &b| capacity[b].partial_cmp(&capacity[a]).unwrap_or(std::cmp::Ordering::Equal));
        let mut running = 0.0;
        for &h in order.iter() {
            running += capacity[h];
            let before = running - capacity[h];
            got[h] = clip(totals[village] - before, 0.0, capacity[h]);
        }
    }
    got
}

/// Coins a plot fetches: `years_purchase` years of its rent.
pub fn land_price(rent_per_plot: &[f64], config: &Config) -> Vec<f64> {
    rent_per_plot.iter().map(|r| config.credit.years_purchase * r).collect()
}

/// What a family's land and animals are worth, in coins.
pub fn collateral(households: &Households, plot_price: &[f64], animal_price: &[f64]) -> Vec<f64> {
    (0..households.len())
        .map(|h| {
            let v = households.location[h] as usize;
            households.land[h] * plot_price[v] + households.animals[h] * animal_price[v]
        })
        .collect()
}

/// A month's interest on every debt (and claim, the town lenders' too), at
/// each place's yearly `rates`. Returns interest per household.
pub fn accrue_interest(households: &mut Households, rates: &[f64], town: Option<&mut Lenders>) -> Vec<f64> {
    let monthly: Vec<f64> = rates.iter().map(|r| (1.0 + r).powf(1.0 / 12.0) - 1.0).collect();
    let loc = &households.location;
    let interest: Vec<f64> = households.debt.iter().zip(loc).map(|(d, &v)| d * monthly[v as usize]).collect();
    for (d, i) in households.debt.iter_mut().zip(&interest) {
        *d += i;
    }
    for (l, &v) in households.lent.iter_mut().zip(loc) {
        *l *= 1.0 + monthly[v as usize];
    }
    if let Some(lenders) = town {
        for (c, m) in lenders.claims.iter_mut().zip(&monthly) {
            *c *= 1.0 + m;
        }
    }
    interest
}

/// Families `short` of coins borrow up to their credit limit (`loan_to_value`
/// of their land and animals' `worth`, plus `personal` credit, less what
/// they owe) from families with coins to `spare`, who lend up to
/// `lend_share` of it. Returns coins lent per place.
pub fn borrow(
    households: &mut Households,
    short: &[f64],
    spare: &[f64],
    worth: &[f64],
    personal: &[f64],
    n: usize,
    config: &Config,
) -> Vec<f64> {
    borrow_from(households, short, spare, worth, personal, n, config, None, None).lent
}

/// A month's borrowing, per place: coins lent, borrowing asked, coins the
/// families offered, and of the coins lent, those the town's lenders lent.
#[derive(Clone, Debug)]
pub struct Borrowing {
    pub lent: Vec<f64>,
    pub asked: Vec<f64>,
    pub offered: Vec<f64>,
    pub by_town: Vec<f64>,
}

/// As `borrow`, with families `willing` to lend more (or less) of their
/// spare coins at each place's rate, and the `town` lenders: neighbours
/// lend first, the town's lenders what they can't.
#[allow(clippy::too_many_arguments)]
pub fn borrow_from(
    households: &mut Households,
    short: &[f64],
    spare: &[f64],
    worth: &[f64],
    personal: &[f64],
    n: usize,
    config: &Config,
    willing: Option<&[f64]>,
    town: Option<&mut Lenders>,
) -> Borrowing {
    let cfg = &config.credit;
    let n_hh = households.len();
    let asked: Vec<f64> = (0..n_hh)
        .map(|h| {
            let room = (cfg.loan_to_value * worth[h] + personal[h] - households.debt[h]).max(0.0);
            short[h].max(0.0).min(room)
        })
        .collect();
    let decide = households.decide();
    let loc = &households.location;
    let offered: Vec<f64> = (0..n_hh)
        .map(|h| {
            if asked[h] > 0.0 {
                return 0.0;
            }
            match willing {
                Some(willing) => {
                    (decide.lend_share(h, cfg.lend_share) * willing[loc[h] as usize]).min(1.0) * spare[h].max(0.0)
                }
                None => decide.lend_share(h, cfg.lend_share) * spare[h].max(0.0),
            }
        })
        .collect();
    let wanted = by_village(&asked, loc, n);
    let available = by_village(&offered, loc, n);
    let from_town = match &town {
        Some(lenders) => lenders.offers(&(0..n).map(|v| wanted[v] - available[v]).collect::<Vec<f64>>(), config),
        None => vec![0.0; n],
    };
    let lent: Vec<f64> = (0..n).map(|v| wanted[v].min(available[v] + from_town[v])).collect();
    // (neighbours first: without town lenders, all of it)
    let local: Vec<f64> = (0..n).map(|v| lent[v].min(available[v])).collect();
    let by_town: Vec<f64> = (0..n).map(|v| lent[v] - local[v]).collect();
    let got = pro_rata(&asked, &lent, loc);
    let gave = pro_rata(&offered, &local, loc);
    for h in 0..n_hh {
        households.money[h] += got[h] - gave[h];
        households.debt[h] += got[h];
        households.lent[h] += gave[h];
    }
    if let Some(lenders) = town {
        lenders.lend(&by_town);
    }
    Borrowing { lent, asked: wanted, offered: available, by_town }
}

/// Borrowers pay `repay_share` of this month's income toward their debts,
/// plus any coins they have to `spare` (beyond what they keep); lenders get
/// it by their claims (the `town` lenders theirs). Returns coins repaid
/// per place.
pub fn repay(
    households: &mut Households,
    income: &[f64],
    n: usize,
    config: &Config,
    spare: Option<&[f64]>,
    town: Option<&mut Lenders>,
) -> Vec<f64> {
    let n_hh = households.len();
    let paying: Vec<f64> = (0..n_hh)
        .map(|h| {
            let offered = config.credit.repay_share * income[h].max(0.0) + spare.map_or(0.0, |s| s[h].max(0.0));
            households.debt[h].min(offered.min(households.money[h])).max(0.0)
        })
        .collect();
    let loc = households.location.clone();
    let repaid = by_village(&paying, &loc, n);
    for h in 0..n_hh {
        households.money[h] -= paying[h];
        households.debt[h] -= paying[h];
    }
    let received = match town {
        Some(lenders) => {
            let theirs = lenders.their_part(&repaid, &by_village(&households.lent, &loc, n));
            lenders.receive(&theirs);
            let ours: Vec<f64> = (0..n).map(|v| repaid[v] - theirs[v]).collect();
            pro_rata(&households.lent, &ours, &loc)
        }
        None => pro_rata(&households.lent, &repaid, &loc),
    };
    for h in 0..n_hh {
        households.money[h] += received[h];
        households.lent[h] -= received[h];
    }
    repaid
}

/// Debtors pay in grain too: `grain_repay_share` of whatever grain they hold
/// beyond `grain_kept_months` of their `need` (mostly after harvest), valued
/// at the place's price; the lenders get the grain by their claims. (The
/// `town` lenders want coins: debtors pay their neighbours' share of their
/// debts in grain.) Returns rations paid per place.
pub fn repay_in_grain(
    households: &mut Households,
    need: &[f64],
    food_price: &[f64],
    config: &Config,
    town: Option<&Lenders>,
) -> Vec<f64> {
    let cfg = &config.credit;
    let n = food_price.len();
    let n_hh = households.len();
    let loc = households.location.clone();
    let local = town.map(|lenders| lenders.local_share(&by_village(&households.lent, &loc, n)));
    let paying: Vec<f64> = (0..n_hh)
        .map(|h| {
            let price = food_price[loc[h] as usize];
            let beyond = (households.grain[h] - cfg.grain_kept_months * need[h]).max(0.0);
            let owed = match &local {
                Some(share) => div_or(households.debt[h] * share[loc[h] as usize], price, 0.0),
                None => div_or(households.debt[h], price, 0.0),
            };
            if households.debt[h] > 0.0 {
                (cfg.grain_repay_share * beyond).min(owed)
            } else {
                0.0
            }
        })
        .collect();
    for h in 0..n_hh {
        households.grain[h] -= paying[h];
        households.debt[h] -= paying[h] * food_price[loc[h] as usize];
    }
    let rations = by_village(&paying, &loc, n);
    let value: Vec<f64> = (0..n).map(|v| rations[v] * food_price[v]).collect();
    let received = pro_rata(&households.lent, &value, &loc);
    for h in 0..n_hh {
        households.lent[h] -= received[h];
        households.grain[h] += div_or(received[h], food_price[loc[h] as usize], 0.0);
        households.debt[h] = households.debt[h].max(0.0);
    }
    rations
}

/// What foreclosures took, per place.
#[derive(Clone, Debug)]
pub struct Foreclosures {
    pub animals: Vec<f64>, // taken
    pub land: Vec<f64>,    // plots taken
    pub value: Vec<f64>,   // coins of debt they settled
}

/// Debts beyond `foreclose_at` of what a borrower's animals and land are
/// worth are settled by taking them: animals first, then land, at their
/// price, as far as they cover the debt (what they don't cover stays owed).
/// The biggest creditors get them, and the debt is cleared from their claims.
/// (The `town` lenders take no land or animals: only the neighbours' share
/// of a debt is settled so.)
pub fn foreclose(
    households: &mut Households,
    plot_price: &[f64],
    animal_price: &[f64],
    config: &Config,
    town: Option<&Lenders>,
) -> Foreclosures {
    let cfg = &config.credit;
    let n = plot_price.len();
    let n_hh = households.len();
    let loc = households.location.clone();
    let worth = collateral(households, plot_price, animal_price);
    let local = town.map(|lenders| lenders.local_share(&by_village(&households.lent, &loc, n)));
    let mut take = vec![0.0; n_hh];
    let mut animals = vec![0.0; n_hh];
    let mut plots = vec![0.0; n_hh];
    for h in 0..n_hh {
        let v = loc[h] as usize;
        if households.debt[h] > cfg.foreclose_at * worth[h] {
            take[h] = match &local {
                Some(share) => worth[h].min(households.debt[h]) * share[v],
                None => worth[h].min(households.debt[h]),
            };
        }
        let animal_value = households.animals[h] * animal_price[v];
        let from_animals = take[h].min(animal_value);
        let from_land = take[h] - from_animals;
        animals[h] = div_or(from_animals, animal_price[v], 0.0).min(households.animals[h]);
        plots[h] = div_or(from_land, plot_price[v], 0.0).min(households.land[h]);
        households.animals[h] -= animals[h];
        households.land[h] -= plots[h];
        households.debt[h] -= take[h];
    }
    let settled = by_village(&take, &loc, n);
    let taken_animals = by_village(&animals, &loc, n);
    let taken_land = by_village(&plots, &loc, n);
    // The biggest creditors take them, each up to its claim.
    let value = biggest_first(&households.lent, &settled, &loc);
    for h in 0..n_hh {
        let v = loc[h] as usize;
        let share = div_or(value[h], settled[v], 0.0);
        households.lent[h] -= value[h];
        households.animals[h] += share * taken_animals[v];
        households.land[h] += share * taken_land[v];
    }
    Foreclosures { animals: taken_animals, land: taken_land, value: settled }
}

/// Debt beyond `default_at` times a family's credit `limit` (per household)
/// can never be repaid: the excess is lost to the lenders, by their claims
/// (the `town` lenders' too). Returns coins written off per place.
pub fn default(
    households: &mut Households,
    limit: &[f64],
    n: usize,
    config: &Config,
    town: Option<&mut Lenders>,
) -> Vec<f64> {
    let loc = households.location.clone();
    let excess: Vec<f64> =
        (0..households.len()).map(|h| (households.debt[h] - config.credit.default_at * limit[h]).max(0.0)).collect();
    for (d, e) in households.debt.iter_mut().zip(&excess) {
        *d -= e;
    }
    let lost = by_village(&excess, &loc, n);
    let cut = match town {
        Some(lenders) => {
            let theirs = lenders.their_part(&lost, &by_village(&households.lent, &loc, n));
            lenders.lose(&theirs);
            pro_rata(&households.lent, &(0..n).map(|v| lost[v] - theirs[v]).collect::<Vec<f64>>(), &loc)
        }
        None => pro_rata(&households.lent, &lost, &loc),
    };
    for (l, c) in households.lent.iter_mut().zip(&cut) {
        *l = (*l - c).max(0.0);
    }
    lost
}

/// Land sold, per place.
#[derive(Clone, Debug)]
pub struct LandSales {
    pub plots: Vec<f64>, // sold
    pub price: Vec<f64>, // coins per plot
}

/// Families still `short` of coins for their food sell land to families
/// with coins to `spare` (who put up to `buyers_spend` of it into land).
/// With more land on offer than buyers can pay for at its `worth`, the
/// price falls (not below `lowest_price` of it).
pub fn sell_land(
    households: &mut Households,
    short: &[f64],
    spare: &[f64],
    worth: &[f64],
    config: &Config,
) -> LandSales {
    let cfg = &config.credit;
    let loc = households.location.clone();
    let (plots, price, selling, bought) =
        distress_market(&households.land, short, spare, worth, cfg.buyers_spend, cfg.lowest_price, &loc);
    for h in 0..households.len() {
        let p = price[loc[h] as usize];
        households.land[h] += bought[h] - selling[h];
        households.money[h] += selling[h] * p - bought[h] * p;
    }
    LandSales { plots, price }
}

/// A market of families who must sell (land or animals: `held`) to raise
/// what they are `short` of, and families with coins to `spare`, who put up
/// to `buyers_spend` of it in; with more on offer than the buyers can pay
/// for at its `worth`, the price falls, to no less than `lowest_price` of
/// it. Returns (sold per place, price per place, sold per household, bought
/// per household): the richest buyers take it, each as far as its coins go.
pub fn distress_market(
    held: &[f64],
    short: &[f64],
    spare: &[f64],
    worth: &[f64],
    buyers_spend: f64,
    lowest_price: f64,
    location: &[u32],
) -> (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>) {
    let n = worth.len();
    let m = held.len();
    let offer: Vec<f64> =
        (0..m).map(|h| held[h].min(div_or(short[h], worth[location[h] as usize], 0.0)).max(0.0)).collect();
    let budget: Vec<f64> =
        (0..m).map(|h| if offer[h] <= 0.0 { buyers_spend * spare[h].max(0.0) } else { 0.0 }).collect();
    let offered = by_village(&offer, location, n);
    let coins = by_village(&budget, location, n);
    let price: Vec<f64> =
        (0..n).map(|v| worth[v].min(div_or(coins[v], offered[v], worth[v])).max(lowest_price * worth[v])).collect();
    let sold: Vec<f64> = (0..n).map(|v| offered[v].min(div_or(coins[v], price[v], 0.0))).collect();
    let filled: Vec<f64> = (0..n).map(|v| div_or(sold[v], offered[v], 0.0)).collect();
    let selling: Vec<f64> = (0..m).map(|h| offer[h] * filled[location[h] as usize]).collect();
    let can_buy: Vec<f64> = (0..m).map(|h| div_or(budget[h], price[location[h] as usize], 0.0)).collect();
    let bought = biggest_first(&can_buy, &sold, location);
    (sold, price, selling, bought)
}

/// A ruler's decree cancels `share` (per place) of all debts and the claims
/// on them (the `town` lenders' too). Returns coins of debt cancelled per place.
pub fn cancel_debts(households: &mut Households, share: &[f64], town: Option<&mut Lenders>) -> Vec<f64> {
    let n = share.len();
    let mut cancelled = vec![0.0; n];
    for h in 0..households.len() {
        let v = households.location[h] as usize;
        let c = households.debt[h] * share[v];
        households.debt[h] -= c;
        households.lent[h] *= 1.0 - share[v];
        cancelled[v] += c;
    }
    if let Some(lenders) = town {
        let lost: Vec<f64> = lenders.claims.iter().zip(share).map(|(c, s)| c * s).collect();
        lenders.lose(&lost);
    }
    cancelled
}

/// Debts of families with nobody left are lost to their lenders (the
/// `town` lenders too, by their claims); their own claims are cancelled
/// (their borrowers are let off).
pub fn write_off(households: &mut Households, gone: &[bool], n: usize, town: Option<&mut Lenders>) {
    let loc = households.location.clone();
    let n_hh = households.len();
    let lost = crate::grid::sum_by(n, &loc, |h| if gone[h] { households.debt[h] } else { 0.0 });
    for h in 0..n_hh {
        if gone[h] {
            households.debt[h] = 0.0;
        }
    }
    let living_claims: Vec<f64> = (0..n_hh).map(|h| if gone[h] { 0.0 } else { households.lent[h] }).collect();
    let cut = match town {
        Some(lenders) => {
            let theirs = lenders.their_part(&lost, &by_village(&living_claims, &loc, n));
            lenders.lose(&theirs);
            pro_rata(&living_claims, &(0..n).map(|v| lost[v] - theirs[v]).collect::<Vec<f64>>(), &loc)
        }
        None => pro_rata(&living_claims, &lost, &loc),
    };
    for h in 0..n_hh {
        if !gone[h] {
            households.lent[h] -= cut[h];
        }
    }
    let let_off = crate::grid::sum_by(n, &loc, |h| if gone[h] { households.lent[h] } else { 0.0 });
    for h in 0..n_hh {
        if gone[h] {
            households.lent[h] = 0.0;
        }
    }
    let relief = pro_rata(&households.debt, &let_off, &loc);
    for h in 0..n_hh {
        households.debt[h] = (households.debt[h] - relief[h]).max(0.0);
        households.lent[h] = households.lent[h].max(0.0);
    }
}
