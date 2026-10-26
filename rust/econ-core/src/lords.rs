//! The lord and the state: what leaves the village.
//!
//! A lord holds part of the village's land (his demesne): its share of the
//! harvest goes to his barn, and tenants owe him labour days. Part of the
//! barn is carted away to his hall each month; his steward sells some of the
//! rest in the village, and the coins go into his purse, outside the village
//! (though he spends some of them on the village's cloth, or in his market
//! town). The state takes a tax in coin each year after the harvest; families
//! without the coins have grain seized. Both purses are outside the villages.
//! On a map with a stronghold, the state spends most of its tax there, on
//! the pay of its garrison and officials: what keeps a city.

use crate::config::Config;
use crate::grid::div_or;
use crate::households::Households;
use crate::market::by_village;

#[derive(Clone, Debug, PartialEq)]
pub struct Lords {
    pub land: Vec<f64>,        // demesne plots, per place
    pub barn: Vec<f64>,        // rations in the lord's barn, per place
    pub purse: Vec<f64>,       // coins the lord has taken out of the place
    pub state_purse: Vec<f64>, // coins the state has taken out of the place
    /// The state's monthly pay to its garrison and officials in the
    /// stronghold (set from each year's tax, and in the first month from
    /// what the tax would bring).
    pub garrison_pay: Option<f64>,
}

impl Lords {
    pub fn none(n: usize) -> Lords {
        Lords {
            land: vec![0.0; n],
            barn: vec![0.0; n],
            purse: vec![0.0; n],
            state_purse: vec![0.0; n],
            garrison_pay: None,
        }
    }
}

/// Plots the lord holds in each place at the start.
pub fn take_demesne(village_land: &[f64], config: &Config) -> Vec<f64> {
    let share = if config.lord.enabled { config.lord.demesne } else { 0.0 };
    village_land.iter().map(|l| l * share).collect()
}

pub fn into_barn(lords: &mut Lords, grain: &[f64]) {
    for (b, g) in lords.barn.iter_mut().zip(grain) {
        *b += g;
    }
}

/// A share of the barn goes to the lord's hall each month. Returns rations taken away.
pub fn cart_away(lords: &mut Lords, config: &Config) -> Vec<f64> {
    lords
        .barn
        .iter_mut()
        .map(|b| {
            let away = *b * config.lord.carted_away;
            *b -= away;
            away
        })
        .collect()
}

/// Grain the lord's steward offers on the village market this month.
pub fn offer(lords: &Lords, config: &Config) -> Vec<f64> {
    lords.barn.iter().map(|b| b * config.lord.sells).collect()
}

pub fn sold(lords: &mut Lords, rations: &[f64], price: &[f64]) {
    for v in 0..rations.len() {
        lords.barn[v] -= rations[v];
        lords.purse[v] += rations[v] * price[v];
    }
}

/// Coins the lord's household spends this month on cloth: in the village,
/// or (`in_town`) in its market town (what is not paid to servants).
pub fn local_spending(lords: &Lords, config: &Config, in_town: bool) -> Vec<f64> {
    let share =
        if in_town { config.lord.spends_in_town * (1.0 - config.lord.servants) } else { config.lord.spends_locally };
    lords.purse.iter().map(|p| p * share).collect()
}

/// The lord's household pays townsfolk of his market town for their service
/// this month (`servants` of what he spends in town), shared by the town's
/// families by their members. Takes it from his purse; returns coins
/// received per household.
pub fn pay_servants(
    lords: &mut Lords,
    market: &[i64],
    households_location: &[u32],
    members: &[f64],
    config: &Config,
) -> Vec<f64> {
    let n = lords.purse.len();
    let people = by_village(members, households_location, n);
    let mut to_town = vec![0.0; n];
    for v in 0..n {
        let m = market[v];
        if m >= 0 && people[m as usize] > 0.0 {
            let wages = lords.purse[v] * config.lord.spends_in_town * config.lord.servants;
            lords.purse[v] -= wages;
            to_town[m as usize] += wages;
        }
    }
    (0..members.len())
        .map(|h| {
            let v = households_location[h] as usize;
            div_or(to_town[v] * members[h], people[v], 0.0)
        })
        .collect()
}

/// The grain carted from each place's barn is sold to the dealers of its
/// market town at the town's price, as far as their coins go (beyond what
/// they have `promised` to villages this month); the rest goes back into
/// the barn. Changes the dealers' store and coins and the lord's purse;
/// returns rations sold per place.
pub fn sell_in_town(
    lords: &mut Lords,
    carted: &[f64],
    market: &[i64],
    dealers_grain: &mut [f64],
    dealers_cash: &mut [f64],
    town_price: &[f64],
    promised: Option<&[f64]>,
) -> Vec<f64> {
    let n = carted.len();
    let to: Vec<usize> = market.iter().map(|&m| m.max(0) as usize).collect();
    let mut asked = vec![0.0; n];
    for v in 0..n {
        if market[v] >= 0 {
            asked[to[v]] += carted[v] * town_price[to[v]];
        }
    }
    let afford: Vec<f64> = (0..n)
        .map(|t| {
            let free = (dealers_cash[t] - promised.map_or(0.0, |p| p[t])).max(0.0);
            1.0f64.min(div_or(free, asked[t], 1.0))
        })
        .collect();
    let mut sold = vec![0.0; n];
    for v in 0..n {
        if market[v] >= 0 {
            sold[v] = carted[v] * afford[to[v]];
            let paid = sold[v] * town_price[to[v]];
            dealers_grain[to[v]] += sold[v];
            dealers_cash[to[v]] -= paid;
            lords.purse[v] += paid;
        }
        lords.barn[v] += carted[v] - sold[v];
    }
    sold
}

/// A charitable lord opens his barn to families still hungry in a famine
/// (`gap` per household). Returns rations given per household.
pub fn charity(lords: &mut Lords, gap: &[f64], location: &[u32], famine: &[bool], config: &Config) -> Vec<f64> {
    let n = lords.barn.len();
    if !config.lord.charity_in_famine {
        return vec![0.0; gap.len()];
    }
    let wanted = by_village(gap, location, n);
    let mut share = vec![0.0; n];
    for v in 0..n {
        let given = if famine[v] { wanted[v].min(lords.barn[v]) } else { 0.0 };
        share[v] = div_or(given, wanted[v], 0.0);
        lords.barn[v] -= given;
    }
    gap.iter().zip(location).map(|(&g, &l)| g * share[l as usize]).collect()
}

/// The state's take, per place.
#[derive(Clone, Debug)]
pub struct TaxTake {
    pub coins: Vec<f64>,
    pub grain: Vec<f64>, // rations seized from families without the coins
}

/// The state's yearly tax: `hearth_tax_months` of the customary wage on
/// every living household and `land_tax` of a year's rent on every plot
/// families hold. Paid in coins, but not out of the coins a family must
/// `keep` for its food and firewood: for the rest the collector seizes grain
/// at the market price; what they still can't pay is let go. In a famine it
/// is remitted if `remit_in_famine`. Households with nobody left (`living`
/// false) pay nothing.
#[allow(clippy::too_many_arguments)]
pub fn collect_state_tax(
    households: &mut Households,
    lords: &mut Lords,
    wage: &[f64],
    rent_per_plot: &[f64],
    food_price: &[f64],
    famine: &[bool],
    config: &Config,
    living: Option<&[bool]>,
    keep: Option<&[f64]>,
) -> TaxTake {
    let cfg = &config.state;
    let n = wage.len();
    let mut coins = vec![0.0; n];
    let mut grain = vec![0.0; n];
    if !cfg.enabled {
        return TaxTake { coins, grain };
    }
    for h in 0..households.len() {
        let v = households.location[h] as usize;
        let mut due = cfg.hearth_tax_months * wage[v] + cfg.land_tax * households.land[h] * rent_per_plot[v];
        if living.is_some_and(|l| !l[h]) || (cfg.remit_in_famine && famine[v]) {
            due = 0.0;
        }
        let can_pay = households.money[h] - keep.map_or(0.0, |k| k[h]);
        let paid = due.min(can_pay.max(0.0));
        households.money[h] -= paid;
        let short = due - paid;
        let seized = div_or(short, food_price[v], 0.0).min(households.grain[h]);
        households.grain[h] -= seized;
        coins[v] += paid;
        grain[v] += seized;
    }
    for v in 0..n {
        lords.state_purse[v] += coins[v];
    }
    TaxTake { coins, grain }
}

/// What the state's tax would bring this year if every family paid in
/// coin (before any remission in a famine).
pub fn tax_due(
    households: &Households,
    wage: &[f64],
    rent_per_plot: &[f64],
    config: &Config,
    living: Option<&[bool]>,
) -> f64 {
    let cfg = &config.state;
    (0..households.len())
        .filter(|&h| living.is_none_or(|l| l[h]))
        .map(|h| {
            let v = households.location[h] as usize;
            cfg.hearth_tax_months * wage[v] + cfg.land_tax * households.land[h] * rent_per_plot[v]
        })
        .sum()
}

/// The state pays its garrison and officials in the `stronghold` this
/// month, from its purse: shared by the stronghold's families by their
/// members. Returns coins received per household.
pub fn pay_garrison(lords: &mut Lords, stronghold: usize, households_location: &[u32], members: &[f64]) -> Vec<f64> {
    let pay = lords.garrison_pay.unwrap_or(0.0);
    let people: f64 =
        (0..members.len()).filter(|&h| households_location[h] as usize == stronghold).map(|h| members[h]).sum();
    if people <= 0.0 {
        return vec![0.0; members.len()];
    }
    lords.state_purse[stronghold] -= pay;
    (0..members.len())
        .map(|h| if households_location[h] as usize == stronghold { pay * members[h] / people } else { 0.0 })
        .collect()
}

/// Soldiers or raiders take `share` (per place) of all grain and animals.
/// Returns (rations taken from families and farms, rations taken from the
/// lord's barn) per place.
pub fn requisition(
    households: &mut Households,
    lords: &mut Lords,
    farm_grain: &mut [f64],
    share: &[f64],
) -> (Vec<f64>, Vec<f64>) {
    let n = share.len();
    let mut taken = vec![0.0; n];
    for h in 0..households.len() {
        let v = households.location[h] as usize;
        let from_family = households.grain[h] * share[v];
        households.grain[h] -= from_family;
        households.animals[h] *= 1.0 - share[v];
        taken[v] += from_family;
    }
    let mut from_barn = vec![0.0; n];
    for v in 0..n {
        from_barn[v] = lords.barn[v] * share[v];
        lords.barn[v] -= from_barn[v];
        let from_farms = farm_grain[v] * share[v];
        farm_grain[v] -= from_farms;
        taken[v] += from_farms;
    }
    (taken, from_barn)
}
