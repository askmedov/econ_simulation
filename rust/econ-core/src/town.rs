//! The town: a regional grain market, and where coins come from and go.
//!
//! Grain has a regional price in the town, which moves with the seasons,
//! with the region's harvests and at random. Merchants buy a village's grain
//! when it is cheaper than the town price less carting, and bring grain to
//! sell when the village price is above the town price plus carting.
//!
//! Coins are metal: nobody prints them. They come in when villages sell to
//! the world beyond and leave when they buy from it, pay taxes in coin or
//! sell to their lord; and each year a little is lost or buried. The town's
//! purse counts the coins it has paid out (negative) or taken in, so that
//! every coin is accounted for.

use crate::config::Config;
use crate::rng::Rng;

#[derive(Clone, Debug, PartialEq)]
pub struct Town {
    pub level: f64,      // coins per ration of grain in the town in a normal year, at the start
    pub swing: f64,      // the region's random price swing (log), slowly reverting to 0
    pub purse: f64,      // coins the town has taken in from the villages, net (negative: paid out)
    pub coins_lost: f64, // coins lost, worn away or buried, since the start
}

impl Town {
    pub fn at(level: f64) -> Town {
        Town { level, swing: 0.0, purse: 0.0, coins_lost: 0.0 }
    }
}

/// The town's grain price this month, per place (the region's harvest
/// follows each place's weather in part).
pub fn price(town: &Town, month_of_year: i64, farm_weather: &[f64], config: &Config) -> Vec<f64> {
    let cfg = &config.town;
    let season =
        1.0 + cfg.seasonal * (2.0 * std::f64::consts::PI * (month_of_year - cfg.dearest_month) as f64 / 12.0).cos();
    farm_weather
        .iter()
        .map(|&w| {
            let dearth = 1.0 + cfg.regional_share * (1.0 / w.max(0.2) - 1.0).max(0.0);
            town.level * season * town.swing.exp() * dearth
        })
        .collect()
}

/// A month of the region's random price swing.
pub fn drift(town: &mut Town, config: &Config, rng: &mut Rng) {
    let cfg = &config.town;
    town.swing = cfg.swing_memory * town.swing + rng.normal(0.0, cfg.swing_sd);
}

/// What merchants will carry this month.
#[derive(Clone, Debug)]
pub struct Merchants {
    pub exports: Vec<f64>, // rations merchants would buy, per place (0 if none)
    pub imports: Vec<f64>, // rations merchants offer, per place (0 if none)
}

/// Grain out when a village is cheap, grain in when it is dear. Carting it
/// costs `transport` of the price (per place).
pub fn merchants(
    village_price: &[f64],
    town_price: &[f64],
    need: &[f64],
    config: &Config,
    transport: &[f64],
) -> Merchants {
    let cfg = &config.town;
    let n = need.len();
    if !cfg.enabled {
        return Merchants { exports: vec![0.0; n], imports: vec![0.0; n] };
    }
    let mut exports = vec![0.0; n];
    let mut imports = vec![0.0; n];
    for v in 0..n {
        let cart = cfg.capacity * need[v];
        // (a village no road reaches never trades: the comparisons fail)
        if village_price[v] < town_price[v] * (1.0 - transport[v]) {
            exports[v] = cart;
        }
        if village_price[v] > town_price[v] * (1.0 + transport[v]) {
            imports[v] = cart;
        }
    }
    Merchants { exports, imports }
}

/// A month's share of the coins lost, worn away or buried for good.
pub fn lose_coins(money: &mut [f64], town: &mut Town, config: &Config) -> f64 {
    let rate = 1.0 - (1.0 - config.town.coins_lost_a_year).powf(1.0 / 12.0);
    let mut lost = 0.0;
    for m in money.iter_mut() {
        let l = *m * rate;
        *m -= l;
        lost += l;
    }
    town.coins_lost += lost;
    lost
}
