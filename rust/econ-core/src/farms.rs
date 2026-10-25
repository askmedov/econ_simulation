//! Peasant farms: the harvest is shared out in kind and families live off
//! their own grain store, selling what they can spare and buying what they lack.
//!
//! The place's fields are farmed as one (`economy::capacity`), but the grain
//! belongs to the people who grew it and the families who hold the land:
//! workers get the labour share by how much they worked, landholders the
//! rest by the plots they hold.

use crate::config::Config;
use crate::economy::{household_effort, Workforce};
use crate::grid::{clip, div_or, Grid};
use crate::households::Households;
use crate::market::by_village;
use crate::population::Population;
use crate::rules::season_factors;

/// Each household's share of its place's harvest (after deductions), and
/// the lord's share per place.
///
/// `animals_part` (per place) goes to the owners of plough animals by their
/// animals; of the rest, the labour share goes to families by their farm
/// work (less the labour days owed to the lord, `labour_service`) and the
/// land share to the holders of the land by their plots, the lord's demesne
/// (`lord_land`) included. `extra_effort` (per household) is work in the
/// fields by people outside farming (harvest help).
#[allow(clippy::too_many_arguments)]
pub fn income_shares(
    population: &Population,
    households: &Households,
    village_land: &[f64],
    config: &Config,
    animals_part: Option<&[f64]>,
    lord_land: Option<&[f64]>,
    extra_effort: Option<&[f64]>,
    force: &Workforce,
    visiting: Option<&[f64]>,
) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let n = village_land.len();
    let n_hh = households.len();
    let loc = &households.location;
    let a = config.food.labor_share;
    let zeros = vec![0.0; n];
    let lord = lord_land.unwrap_or(&zeros);
    let mut effort = household_effort(population, config.farming(), n_hh, force);
    if let Some(extra) = extra_effort {
        for (e, x) in effort.iter_mut().zip(extra) {
            *e += x;
        }
    }
    let mut total_effort = by_village(&effort, loc, n);
    // Hands from the town work the fields too, and claim their share.
    if let Some(visiting) = visiting {
        for v in 0..n {
            total_effort[v] += visiting[v];
        }
    }
    let held = by_village(&households.land, loc, n);
    let all_land: Vec<f64> = (0..n).map(|v| held[v] + lord[v]).collect();
    // Where nobody holds land (or nobody farms), the other share goes to the workers (or holders).
    let mut land_part = vec![0.0; n];
    let mut work_part = vec![0.0; n];
    let mut service = vec![0.0; n];
    let mut to_lord = vec![0.0; n];
    for v in 0..n {
        let lp = if all_land[v] > 0.0 { 1.0 - a } else { 0.0 };
        let wp = if total_effort[v] > 0.0 { a } else { 0.0 };
        let total = lp + wp;
        land_part[v] = div_or(lp, total, 0.0);
        work_part[v] = div_or(wp, total, 0.0);
        service[v] = if lord[v] > 0.0 { config.lord.labour_service } else { 0.0 };
        to_lord[v] = land_part[v] * div_or(lord[v], all_land[v], 0.0) + work_part[v] * service[v];
    }
    let mut visitors: Vec<f64> = (0..n)
        .map(|v| visiting.map_or(0.0, |x| work_part[v] * (1.0 - service[v]) * div_or(x[v], total_effort[v], 0.0)))
        .collect();
    let mut shares: Vec<f64> = (0..n_hh)
        .map(|h| {
            let v = loc[h] as usize;
            let work = div_or(effort[h], total_effort[v], 0.0);
            let land = div_or(households.land[h], all_land[v], 0.0);
            work_part[v] * (1.0 - service[v]) * work + land_part[v] * land
        })
        .collect();
    if let Some(animals_part) = animals_part {
        let herd = by_village(&households.animals, loc, n);
        let part: Vec<f64> = (0..n).map(|v| if herd[v] > 0.0 { animals_part[v] } else { 0.0 }).collect();
        for h in 0..n_hh {
            let v = loc[h] as usize;
            let owned = div_or(households.animals[h], herd[v], 0.0);
            shares[h] = part[v] * owned + (1.0 - part[v]) * shares[h];
        }
        for v in 0..n {
            to_lord[v] *= 1.0 - part[v];
            visitors[v] *= 1.0 - part[v];
        }
    }
    (shares, to_lord, visitors)
}

/// Move the farms' grain, beyond what they keep to sell for tools, into
/// families' stores by their `shares` and to the lord by his. Changes
/// `farm_grain`; returns (grain received per household, grain for the lord
/// per place).
pub fn share_harvest(
    households: &mut Households,
    farm_grain: &mut [f64],
    keep_for_tools: &[f64],
    shares: &[f64],
    to_lord: Option<&[f64]>,
    visitors: Option<&[f64]>,
) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let n = farm_grain.len();
    let zeros = vec![0.0; n];
    let lord = to_lord.unwrap_or(&zeros);
    let claimed = by_village(shares, &households.location, n);
    let mut per_claim = vec![0.0; n];
    for v in 0..n {
        let mut claims = claimed[v] + lord[v];
        if let Some(visitors) = visitors {
            claims += visitors[v];
        }
        // Places where nobody has a claim keep their grain in the farm store.
        let to_share = if claims > 0.0 { (farm_grain[v] - keep_for_tools[v]).max(0.0) } else { 0.0 };
        per_claim[v] = div_or(to_share, claims, 0.0);
        farm_grain[v] -= to_share;
    }
    let received: Vec<f64> =
        shares.iter().zip(&households.location).map(|(&s, &l)| s * per_claim[l as usize]).collect();
    for (g, r) in households.grain.iter_mut().zip(&received) {
        *g += r;
    }
    let to_visitors = visitors.map_or_else(|| vec![0.0; n], |x| (0..n).map(|v| x[v] * per_claim[v]).collect());
    (received, (0..n).map(|v| lord[v] * per_claim[v]).collect(), to_visitors)
}

/// How each family uses its grain this month.
#[derive(Clone, Debug)]
pub struct FoodPlan {
    pub own: Vec<f64>,   // rations each family eats from its own store this month
    pub spare: Vec<f64>, // rations it is willing to sell
    pub want: Vec<f64>,  // rations it would like to buy
}

/// How each family uses its grain this month.
///
/// It eats the steadiest ration its store plus its expected share of the
/// coming harvests allows (`rules::plan_ration`, per family), and wants to
/// buy the rest of its need. It sells only what is beyond the store it
/// needs to eat fully until its harvests come in, plus `keep_months` of
/// need as a margin, which it lets go when grain is dear (divided by the
/// grain `markup`, per place) or when it is in debt. `after_deductions` (per
/// place) is the share of the harvest left after the council's levy.
pub fn plan_family_food(
    households: &Households,
    need: &[f64],
    village_outlook: &Grid,
    shares: &[f64],
    after_deductions: &[f64],
    config: &Config,
    markup: Option<&[f64]>,
) -> FoodPlan {
    let n_hh = households.len();
    let months = village_outlook.cols;
    let mut own = Vec::with_capacity(n_hh);
    let mut spare = Vec::with_capacity(n_hh);
    let mut want = Vec::with_capacity(n_hh);
    let kept_a_month = 1.0 - config.food.spoilage;
    let decide = households.decide();
    for h in 0..n_hh {
        let v = households.location[h] as usize;
        let mine = shares[h] * after_deductions[v];
        let grain = households.grain[h];
        let need_h = need[h];
        // plan_ration_one (at most a full ration) and stock_to_keep_one, in
        // one pass over the coming months: the same steps, side by side.
        let (ration, to_keep) = if config.food.planning_months <= 0 {
            (1.0, need_h)
        } else {
            let mut available = grain;
            let mut required = need_h;
            let mut affordable = if required > 0.0 { available / required } else { 1.0 };
            let (mut income, mut kept, mut smallest) = (0.0, 1.0, need_h);
            for m in 0..months {
                let coming = village_outlook.get(v, m) * mine;
                available = available * kept_a_month + coming;
                required = required * kept_a_month + need_h;
                affordable = affordable.min(if required > 0.0 { available / required } else { 1.0 });
                income = income * kept_a_month + coming;
                kept *= kept_a_month;
                smallest = smallest.max((required - income) / kept);
            }
            (clip(affordable, 0.0, 1.0), smallest.max(0.0))
        };
        let eat = (ration * need_h).min(grain);
        let dear = markup.map_or(1.0, |m| m[v].max(1.0));
        // (debtors sell it to pay)
        let margin = if households.debt[h] > 0.0 { 0.0 } else { decide.grain_margin(h, config.land.keep_months) };
        let keep = to_keep + margin * need_h / dear;
        own.push(eat);
        spare.push((grain - keep.max(eat)).max(0.0));
        want.push((need[h] - eat).max(0.0));
    }
    FoodPlan { own, spare, want }
}

/// Who sold the grain that sold in each place.
#[derive(Clone, Debug)]
pub struct GrainSellers {
    pub families: Vec<f64>, // sold per household
    pub farms: Vec<f64>,    // sold from the farm store, per place
    pub lord: Vec<f64>,     // sold by the lord's steward, per place
    pub imports: Vec<f64>,  // sold by merchants from the town, per place
}

/// Split each place's grain sales between the farm store (sold first: it is
/// grain kept back to pay for tools), merchants' grain from the town (next),
/// and the families and the lord's steward who offered grain, in proportion
/// to what each offered.
pub fn sellers_share(
    sold: &[f64],
    offered_by_families: &[f64],
    farm_offer: &[f64],
    location: &[u32],
    lord_offer: Option<&[f64]>,
    imports: Option<&[f64]>,
) -> GrainSellers {
    let n = farm_offer.len();
    let zeros = vec![0.0; n];
    let lord = lord_offer.unwrap_or(&zeros);
    let brought = imports.unwrap_or(&zeros);
    let by_farms: Vec<f64> = (0..n).map(|v| sold[v].min(farm_offer[v])).collect();
    let by_imports: Vec<f64> = (0..n).map(|v| (sold[v] - by_farms[v]).min(brought[v])).collect();
    let families_offer = by_village(offered_by_families, location, n);
    let filled: Vec<f64> = (0..n)
        .map(|v| div_or(sold[v] - by_farms[v] - by_imports[v], families_offer[v] + lord[v], 0.0).min(1.0))
        .collect();
    GrainSellers {
        families: offered_by_families.iter().zip(location).map(|(&o, &l)| o * filled[l as usize]).collect(),
        farms: by_farms,
        lord: (0..n).map(|v| lord[v] * filled[v]).collect(),
        imports: by_imports,
    }
}

/// Seed to sow the plots farmed for a year.
pub fn seed_needed(farmed: &[f64], config: &Config) -> Vec<f64> {
    farmed.iter().map(|f| config.food.seed_per_plot * f).collect()
}

pub fn sowing_month(config: &Config) -> i64 {
    *config.food.seed_months.last().expect("seed months")
}

/// Share of the year's seed meant to be picked from this calendar month's
/// harvest (by its share of the seed months' harvests).
pub fn seed_plan(month_of_year: i64, config: &Config) -> f64 {
    let months = &config.food.seed_months;
    if !months.contains(&month_of_year) {
        return 0.0;
    }
    let season = season_factors(config);
    let total: f64 = months.iter().map(|&m| season[(m - 1) as usize]).sum();
    season[(month_of_year - 1) as usize] / total
}

fn seed_done_before(month_of_year: i64, config: &Config) -> f64 {
    config.food.seed_months.iter().filter(|&&m| m < month_of_year).map(|&m| seed_plan(m, config)).sum()
}

/// In the seed months, pick next year's seed from this month's `harvest`
/// before it is shared out: this month's part of what is `needed`, plus
/// anything a past seed month fell short, but never more than
/// `max_seed_share` of the harvest. Changes both; returns grain kept.
pub fn keep_seed(
    farm_grain: &mut [f64],
    seed: &mut [f64],
    needed: &[f64],
    harvest: &[f64],
    month_of_year: i64,
    config: &Config,
) -> Vec<f64> {
    let kept = seed_due(farm_grain, seed, needed, harvest, month_of_year, config);
    for v in 0..kept.len() {
        farm_grain[v] -= kept[v];
        seed[v] += kept[v];
    }
    kept
}

/// Seed to be picked from this month's harvest (see `keep_seed`), without
/// picking it.
pub fn seed_due(
    farm_grain: &[f64],
    seed: &[f64],
    needed: &[f64],
    harvest: &[f64],
    month_of_year: i64,
    config: &Config,
) -> Vec<f64> {
    let share = seed_plan(month_of_year, config);
    if share <= 0.0 {
        return vec![0.0; seed.len()];
    }
    let done = seed_done_before(month_of_year, config);
    (0..seed.len())
        .map(|v| {
            let wanted = (needed[v] * (done + share) - seed[v]).max(0.0);
            wanted.min(config.food.max_seed_share * harvest[v]).min(farm_grain[v])
        })
        .collect()
}

/// Share of the year's seed picked by the start of this calendar month (for
/// a run that starts in the middle of the seed months; none once it is sown).
pub fn seed_gathered(month_of_year: i64, config: &Config) -> f64 {
    if month_of_year > sowing_month(config) {
        return 0.0;
    }
    seed_done_before(month_of_year, config)
}

/// Seed expected to be taken from each coming month's harvest (next month
/// first), places x months.
pub fn seed_outlook(needed: &[f64], month_of_year: i64, months: usize, config: &Config) -> Grid {
    let plan: Vec<f64> = (0..months).map(|m| seed_plan((month_of_year + m as i64) % 12 + 1, config)).collect();
    let mut out = Grid::zeros(needed.len(), months);
    for (r, &need) in needed.iter().enumerate() {
        for (m, &p) in plan.iter().enumerate() {
            out.set(r, m, need * p);
        }
    }
    out
}

/// Sow the seed store; if it is short, landholding families make up the
/// rest from their own grain, by the land they hold, keeping at least two
/// months of their `need`. Returns (share of the needed seed sown, grain
/// families gave) per place.
pub fn sow(households: &mut Households, seed: &mut [f64], needed: &[f64], need: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let n = seed.len();
    let loc = &households.location;
    let can_give: Vec<f64> = (0..households.len())
        .map(|h| if households.land[h] > 0.0 { (households.grain[h] - 2.0 * need[h]).max(0.0) } else { 0.0 })
        .collect();
    let giving_land: Vec<f64> =
        (0..households.len()).map(|h| if can_give[h] > 0.0 { households.land[h] } else { 0.0 }).collect();
    let held = by_village(&giving_land, loc, n);
    // By land held, but nobody gives more than they can.
    let given: Vec<f64> = (0..households.len())
        .map(|h| {
            let v = loc[h] as usize;
            let short = (needed[v] - seed[v]).max(0.0);
            (div_or(giving_land[h], held[v], 0.0) * short).min(can_give[h])
        })
        .collect();
    for (g, x) in households.grain.iter_mut().zip(&given) {
        *g -= x;
    }
    let from_families = by_village(&given, loc, n);
    let sown: Vec<f64> = (0..n).map(|v| clip(div_or(seed[v] + from_families[v], needed[v], 1.0), 0.0, 1.0)).collect();
    seed.iter_mut().for_each(|s| *s = 0.0);
    (sown, from_families)
}

/// Multiplier on each coming month's harvest (next month first) from the
/// seed sown: this year's sowing up to and including the next sowing month
/// (fewer plots sown, worked harder: sown ** (1 - labour share)), a full
/// sowing after it (places x months).
pub fn sown_outlook(sown: &[f64], month_of_year: i64, months: usize, config: &Config) -> Grid {
    let until_sowing = (sowing_month(config) - month_of_year).rem_euclid(12) as usize;
    let a = config.food.labor_share;
    let mut out = Grid::new(sown.len(), months, 1.0);
    for (r, &s) in sown.iter().enumerate() {
        for m in 0..months.min(until_sowing) {
            out.set(r, m, s.powf(1.0 - a));
        }
    }
    out
}
