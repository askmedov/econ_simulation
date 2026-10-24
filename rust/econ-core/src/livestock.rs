//! Livestock: plough animals and herds held by families.
//!
//! Animals (in livestock units: an ox or a cow; a few sheep or pigs count as
//! one) plough and manure the fields: the farms grow more the closer the
//! place comes to `per_plot` animals per plot, and the owners get that part
//! of the harvest. Herds grow in good years, are thinned each November to
//! what the place can feed through winter, die off in droughts and hard
//! winters, and are sold when families can't afford their food, for less
//! the more are sold at once.

use crate::config::Config;
use crate::credit::distress_market;
use crate::grid::{clip, div_or};
use crate::households::Households;
use crate::market::by_village;
use crate::rng::Rng;

/// Farm output multiplier per place from its plough animals.
pub fn farm_factor(animals: &[f64], land: &[f64], config: &Config) -> Vec<f64> {
    let cfg = &config.livestock;
    animals
        .iter()
        .zip(land)
        .map(|(&a, &l)| 1.0 + cfg.farm_boost * clip(div_or(a, cfg.per_plot * l, 1.0), 0.0, 1.0))
        .collect()
}

/// Share of the harvest due to the animals (what they add over none).
pub fn owners_part(factor: &[f64]) -> Vec<f64> {
    factor.iter().map(|f| (f - 1.0) / f).collect()
}

/// Give landholding families a full set of animals at the start, roughly by
/// the land they hold (more where there is more pasture: a hill village is
/// already stocked with sheep and cattle).
pub fn place_herds(
    households: &mut Households,
    village_land: &[f64],
    config: &Config,
    rng: &mut Rng,
    pasture: Option<&[f64]>,
) {
    let n = village_land.len();
    let weight: Vec<f64> = households.land.iter().map(|&l| l * rng.lognormal(0.0, 0.5)).collect();
    let total = by_village(&weight, &households.location, n);
    for h in 0..households.len() {
        let v = households.location[h] as usize;
        let herd = config.livestock.per_plot * village_land[v] * pasture.map_or(1.0, |p| p[v].max(1.0));
        households.animals[h] = div_or(weight[h] * herd, total[v], 0.0);
    }
}

/// A month of births and deaths in every herd. Bad weather for the farms
/// (`farm_weather` below 1) and hard winters (`heating_mult` above 1) kill
/// animals. Returns animals lost per place (net of births, if negative).
pub fn grow_and_die(
    households: &mut Households,
    farm_weather: &[f64],
    heating_mult: &[f64],
    config: &Config,
) -> Vec<f64> {
    let cfg = &config.livestock;
    let n = farm_weather.len();
    let before = by_village(&households.animals, &households.location, n);
    let change: Vec<f64> = (0..n)
        .map(|v| {
            let losses = cfg.drought_losses * (1.0 - farm_weather[v]).max(0.0)
                + cfg.winter_losses * (heating_mult[v] - 1.0).max(0.0);
            (1.0 + cfg.growth).powf(1.0 / 12.0) - 1.0 - losses / 12.0
        })
        .collect();
    for h in 0..households.len() {
        households.animals[h] *= (1.0 + change[households.location[h] as usize]).max(0.0);
    }
    let after = by_village(&households.animals, &households.location, n);
    (0..n).map(|v| before[v] - after[v]).collect()
}

/// Thin each place's herds to what it can feed through winter (more with
/// hill pasture or river meadows: `pasture` times the usual), every owner
/// in proportion; the meat goes into their grain stores. Returns (animals
/// slaughtered, rations of meat) per place.
pub fn winter_cull(
    households: &mut Households,
    village_land: &[f64],
    config: &Config,
    pasture: Option<&[f64]>,
) -> (Vec<f64>, Vec<f64>) {
    let cfg = &config.livestock;
    let n = village_land.len();
    let herd = by_village(&households.animals, &households.location, n);
    let cull: Vec<f64> = (0..n)
        .map(|v| {
            let room = cfg.winter_capacity_per_plot * village_land[v] * pasture.map_or(1.0, |p| p[v]);
            div_or((herd[v] - room).max(0.0), herd[v], 0.0)
        })
        .collect();
    let mut culled = vec![0.0; n];
    for h in 0..households.len() {
        let v = households.location[h] as usize;
        let slaughtered = households.animals[h] * cull[v];
        households.animals[h] -= slaughtered;
        households.grain[h] += slaughtered * cfg.meat_rations;
        culled[v] += slaughtered;
    }
    let meat = culled.iter().map(|c| c * cfg.meat_rations).collect();
    (culled, meat)
}

/// Families with less than a month's food in store and too few coins to buy
/// it slaughter animals for up to two months of food; the meat goes into
/// their stores. `need` is per household, `food_price` per place. Returns
/// (animals slaughtered, rations of meat) per place.
pub fn slaughter_in_hunger(
    households: &mut Households,
    need: &[f64],
    food_price: &[f64],
    config: &Config,
) -> (Vec<f64>, Vec<f64>) {
    let cfg = &config.livestock;
    let n = food_price.len();
    let mut slaughtered = vec![0.0; n];
    for h in 0..households.len() {
        let v = households.location[h] as usize;
        let afford = div_or(households.money[h], food_price[v], 0.0);
        let grain = households.grain[h];
        if grain < need[h] && grain + afford < need[h] {
            let killed = households.animals[h].min(2.0 * need[h] / cfg.meat_rations);
            households.animals[h] -= killed;
            households.grain[h] += killed * cfg.meat_rations;
            slaughtered[v] += killed;
        }
    }
    let meat = slaughtered.iter().map(|s| s * cfg.meat_rations).collect();
    (slaughtered, meat)
}

/// Animals sold in distress, per place.
#[derive(Clone, Debug)]
pub struct AnimalSales {
    pub sold: Vec<f64>,  // animals sold
    pub price: Vec<f64>, // coins per animal
}

/// Families `short` of coins for this month's food sell animals to families
/// with coins to `spare`.
///
/// Each seller offers enough animals to raise its shortfall at the animal's
/// `worth` (per place), up to all it has. Buyers put up to `buyers_spend`
/// of their spare coins into animals. With more animals on offer than
/// buyers can pay for at their worth, the price falls to what the buyers'
/// coins will pay (not below `lowest_price` of the worth, in which case only
/// that many sell). The families with the most coins to spare buy them.
pub fn distress_sales(
    households: &mut Households,
    short: &[f64],
    spare: &[f64],
    worth: &[f64],
    config: &Config,
) -> AnimalSales {
    let cfg = &config.livestock;
    let loc = households.location.clone();
    let (sold, price, selling, got) =
        distress_market(&households.animals, short, spare, worth, cfg.buyers_spend, cfg.lowest_price, &loc);
    for h in 0..households.len() {
        households.animals[h] += got[h] - selling[h];
        households.money[h] += (selling[h] - got[h]) * price[loc[h] as usize];
    }
    AnimalSales { sold, price }
}

/// A year's wool from every herd, sold for `worth` coins an animal (per
/// place). Returns coins received per household.
pub fn shear(households: &Households, worth: &[f64]) -> Vec<f64> {
    (0..households.len()).map(|h| households.animals[h] * worth[households.location[h] as usize]).collect()
}
