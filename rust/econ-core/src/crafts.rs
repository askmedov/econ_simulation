//! Crafts beyond the village's four (Python's `CraftsConfig`): iron dug and
//! smelted where a place has it, which the smiths need for their tools, and
//! the region's crafts sold beyond it at the border towns.
//!
//! A trade that needs a resource (`BusinessSpec.resource`) works only where
//! a place has it (`VillageConfig.resources`). What it makes is sold across
//! the region by merchants (ironmongers) from the makers' stock: in each
//! place at the makers' price plus the way there (along the cheapest
//! roads and routes, with the carting and the merchants' margin at each
//! leg), so a scarcity shows where the good is made and its price reaches
//! everywhere. The carting and the margin go to the merchants of the
//! buyer's market town. A resource no place has is had by every place, and
//! so is one off a map with towns: there is nowhere to bring it from.

use crate::config::Config;
use crate::geography::Geography;
use crate::grid::{div_or, Grid};
use crate::market::{by_village, Sale};

/// `config` with the crafts added, if they are enabled (and not already in).
pub fn with_crafts(config: &Config) -> Config {
    let cc = &config.crafts;
    let mut c = config.clone();
    if !cc.enabled || cc.businesses.iter().all(|b| c.businesses.iter().any(|x| x.name == b.name)) {
        return c;
    }
    for product in &cc.products {
        if !c.products.iter().any(|p| p.name == product.name) {
            c.products.push(product.clone());
        }
    }
    for business in &cc.businesses {
        if !c.businesses.iter().any(|b| b.name == business.name) {
            c.businesses.push(business.clone());
        }
    }
    let tool = c.products[c.product_for("tool")].name.clone();
    for business in c.businesses.iter_mut().filter(|b| b.product == tool) {
        business.inputs = cc.smithing_inputs.clone();
    }
    c
}

/// The wood each trade burns as charcoal, per unit it makes, cut from the
/// place's woods: `crafts.charcoal` for the crafts' own trades (with the
/// crafts enabled), none for the rest.
pub fn charcoal(config: &Config) -> Vec<f64> {
    let cc = &config.crafts;
    config
        .businesses
        .iter()
        .map(|b| if cc.enabled && cc.businesses.iter().any(|x| x.name == b.name) { cc.charcoal } else { 0.0 })
        .collect()
}

/// Where each trade's resource is lacking (places x businesses): only on a
/// map with towns, and only for a resource some place has.
pub fn lacking(config: &Config, geo: &Geography) -> Vec<Vec<bool>> {
    let n = config.villages.len();
    let somewhere: Vec<bool> = config
        .businesses
        .iter()
        .map(|b| b.resource.as_ref().is_some_and(|r| config.villages.iter().any(|v| v.resources.contains(r))))
        .collect();
    (0..n)
        .map(|v| {
            config
                .businesses
                .iter()
                .enumerate()
                .map(|(b, spec)| {
                    geo.has_towns()
                        && somewhere[b]
                        && spec.resource.as_ref().is_some_and(|r| !config.villages[v].resources.contains(r))
                })
                .collect()
        })
        .collect()
}

/// Whether each product is made by a trade that needs a resource.
pub fn from_a_resource(config: &Config) -> Vec<bool> {
    let mut made = vec![false; config.products.len()];
    for business in config.businesses.iter().filter(|b| b.resource.is_some()) {
        made[config.product_index(&business.product)] = true;
    }
    made
}

/// Whether any trade lacks its resource anywhere.
pub fn any_lacking(lacking: &[Vec<bool>]) -> bool {
    lacking.iter().any(|row| row.iter().any(|&l| l))
}

/// Where each place lacking a good gets it from: the next place on the
/// cheapest way from where it is made, along a village's road to its market
/// town and the routes between towns (found once, from the map), and what
/// that leg costs (a share of the price: the carting, at most 0.9, and the
/// merchants' margin). A fixed line, so prices pass down it and never
/// round in a circle.
#[derive(Clone, Debug, PartialEq)]
pub struct SupplyLine {
    pub from: Vec<Option<usize>>,
    pub leg: Vec<f64>,
    /// The whole way's cost from the nearest maker (1 at a maker; infinite
    /// where it can't come from).
    pub way: Vec<f64>,
    /// The places that make it.
    pub makers: Vec<usize>,
    /// Places in order of their distance from where the good is made.
    pub order: Vec<usize>,
}

/// The supply line of each product made only where a resource is (None for
/// the rest, and for every product if no trade lacks its resource).
pub fn supply_lines(config: &Config, geo: &Geography, lacking: &[Vec<bool>]) -> Vec<Option<SupplyLine>> {
    let n = config.villages.len();
    let product_of = crate::economy::product_of(config);
    let margin = config.map.full_load_margin;
    let mut lines = vec![None; config.products.len()];
    for (b, &p) in product_of.iter().enumerate() {
        if !lacking.iter().any(|row| row[b]) {
            continue;
        }
        // The legs, each way: a village and its market town, and the routes.
        let carriage = config.products[p].carriage;
        let mut legs: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
        let mut join = |a: usize, b: usize, way: f64| {
            let leg = (1.0 + (way * carriage).min(0.9)) * (1.0 + margin);
            legs[a].push((b, leg));
            legs[b].push((a, leg));
        };
        for v in 0..n {
            let m = geo.market[v];
            if !geo.town[v] && m >= 0 && geo.transport[v].is_finite() {
                join(v, m as usize, geo.transport[v]);
            }
        }
        let r = &geo.routes;
        for k in 0..r.len() {
            join(r.a[k], r.b[k], r.cost[k]);
        }
        // From every place that makes it at once (costs multiply along the way).
        let mut cost = vec![f64::INFINITY; n];
        let mut from = vec![None; n];
        let mut leg = vec![0.0; n];
        let mut heap = std::collections::BinaryHeap::new();
        for v in (0..n).filter(|&v| !lacking[v][b]) {
            cost[v] = 1.0;
            heap.push(std::cmp::Reverse((Ordered(1.0), v)));
        }
        let mut order = Vec::with_capacity(n);
        let mut done = vec![false; n];
        while let Some(std::cmp::Reverse((Ordered(c), v))) = heap.pop() {
            if done[v] {
                continue;
            }
            done[v] = true;
            order.push(v);
            for &(u, l) in &legs[v] {
                if !done[u] && lacking[u][b] && c * l < cost[u] {
                    cost[u] = c * l;
                    from[u] = Some(v);
                    leg[u] = l;
                    heap.push(std::cmp::Reverse((Ordered(c * l), u)));
                }
            }
        }
        let makers = (0..n).filter(|&v| !lacking[v][b]).collect();
        lines[p] = Some(SupplyLine { from, leg, way: cost, makers, order });
    }
    lines
}

/// A cost that can be ordered (never NaN here).
#[derive(Clone, Copy, PartialEq)]
struct Ordered(f64);
impl Eq for Ordered {}
impl Ord for Ordered {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}
impl PartialOrd for Ordered {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// The makers' price of a good: their prices, weighted by what each has in
/// stock (by none, if none has any).
pub fn makers_price(prices: &Grid, stock: &Grid, line: &SupplyLine, p: usize) -> f64 {
    let held: f64 = line.makers.iter().map(|&u| stock.get(u, p).max(0.0)).sum();
    let weight = |u: usize| if held > 0.0 { stock.get(u, p).max(0.0) } else { 1.0 };
    let total: f64 = line.makers.iter().map(|&u| weight(u)).sum();
    line.makers.iter().map(|&u| prices.get(u, p) * weight(u)).sum::<f64>() / total.max(1e-12)
}

/// In each place lacking a good, its price (before any mark-up: it has
/// none of its own) is the makers' price, from `previous` prices and the
/// makers' `stock`, times the way. Changes `prices`.
pub fn price_brought_in(prices: &mut Grid, previous: &Grid, stock: &Grid, lines: &[Option<SupplyLine>]) {
    for (p, line) in lines.iter().enumerate() {
        let Some(line) = line else { continue };
        let at_makers = makers_price(previous, stock, line, p);
        for v in 0..prices.rows {
            if line.from[v].is_some() {
                prices.set(v, p, at_makers * line.way[v]);
            }
        }
    }
}

/// What the regional market in a good made only where a resource is came to.
pub struct Regional {
    pub sale: Sale,
    pub sold_by: Vec<f64>,   // units each maker sold, per place
    pub paid_to: Vec<f64>,   // coins each maker took, per place
    pub merchants: Vec<f64>, // coins to each town's merchants: the carting and their margin
}

/// The regional market in a good made only where a resource is: merchants
/// sell it everywhere from the makers' stock, at each place's `prices`
/// (the makers' price plus the way). Buyers (`money`, `want`, `place`)
/// buy as in `market::buy`, each getting the same share of what they asked
/// for if the makers' stock runs short. Each maker sells its share by its
/// stock; it takes what its own place paid, and the makers' price for
/// what was sold elsewhere; the rest (the carting and the margin) goes to
/// the merchants of the buyer's market town. The orders go to the makers
/// by what each `can` make this month (at least a tenth of the makers'
/// mean, so an idle maker can start again), not by their stock: a maker
/// that ran out still has its customers. Takes the payments out of `money`.
#[allow(clippy::too_many_arguments)]
pub fn regional_market(
    money: &mut [f64],
    want: &[f64],
    place: &[u32],
    prices: &[f64],
    stock: &[f64],
    can: &[f64],
    line: &SupplyLine,
    at_makers: f64,
    geo: &Geography,
) -> Regional {
    let n = prices.len();
    let asked: Vec<f64> =
        (0..money.len()).map(|i| want[i].min(div_or(money[i], prices[place[i] as usize], 0.0)).max(0.0)).collect();
    let all_asked: f64 = asked.iter().sum();
    let held: f64 = line.makers.iter().map(|&u| stock[u].max(0.0)).sum();
    let filled = 1.0f64.min(div_or(held, all_asked, 1.0));
    let mut bought = Vec::with_capacity(money.len());
    let mut spent = Vec::with_capacity(money.len());
    for i in 0..money.len() {
        let b = asked[i] * filled;
        let s = (b * prices[place[i] as usize]).min(money[i]);
        money[i] -= s;
        bought.push(b);
        spent.push(s);
    }
    let sold = by_village(&bought, place, n);
    let paid = by_village(&spent, place, n);
    // (what each place's buyers asked for; a maker's, its share of all asked)
    let mut demand = by_village(&asked, place, n);
    // What the makers take, and the merchants' part.
    let is_maker = |v: usize| line.from[v].is_none() && line.way[v] == 1.0;
    let mut to_makers = 0.0;
    let mut merchants = vec![0.0; n];
    for v in 0..n {
        if sold[v] <= 0.0 {
            continue;
        }
        if is_maker(v) {
            to_makers += paid[v];
        } else {
            let theirs = (sold[v] * at_makers).min(paid[v]);
            to_makers += theirs;
            let town = if geo.town[v] { v } else { geo.market[v].max(0) as usize };
            merchants[town] += paid[v] - theirs;
        }
    }
    let total_sold: f64 = sold.iter().sum();
    let mean_can = line.makers.iter().map(|&u| can[u].max(0.0)).sum::<f64>() / line.makers.len().max(1) as f64;
    let can_at = |u: usize| can[u].max(0.1 * mean_can).max(1e-12);
    let all_can: f64 = line.makers.iter().map(|&u| can_at(u)).sum();
    let mut sold_by = vec![0.0; n];
    let mut paid_to = vec![0.0; n];
    for &u in &line.makers {
        let share = if held > 0.0 { stock[u].max(0.0) / held } else { 1.0 / line.makers.len() as f64 };
        sold_by[u] = total_sold * share;
        paid_to[u] = to_makers * share;
        demand[u] = all_asked * can_at(u) / all_can;
    }
    for (v, d) in demand.iter_mut().enumerate() {
        if line.from[v].is_none() && !is_maker(v) {
            *d = 0.0; // (nowhere to bring it from)
        }
    }
    Regional { sale: Sale { bought, spent, sold, demand }, sold_by, paid_to, merchants }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{default_config, standard_region, Standard};

    #[test]
    fn the_crafts_add_the_ironworks_and_the_smiths_take_iron() {
        let c = with_crafts(&standard_region(Standard::HundredThousand));
        assert!(c.businesses.iter().any(|b| b.name == "ironworks" && b.resource.as_deref() == Some("iron")));
        assert!(c.products.iter().any(|p| p.name == "iron" && p.use_ == "material"));
        let smithing = c.businesses.iter().find(|b| b.name == "smithing").unwrap();
        assert!(smithing.inputs.iter().any(|(p, _)| p == "iron"));
        assert_eq!(with_crafts(&c), c, "adding them twice adds nothing");
        let mut off = default_config();
        off.crafts.enabled = false;
        assert_eq!(with_crafts(&off), off);
    }

    #[test]
    fn only_the_places_without_iron_lack_it_and_a_village_off_the_map_has_its_own() {
        let c = with_crafts(&standard_region(Standard::HundredThousand));
        let geo = crate::geography::build(&c).unwrap();
        let lack = lacking(&c, &geo);
        let iron = c.businesses.iter().position(|b| b.name == "ironworks").unwrap();
        for (v, place) in c.villages.iter().enumerate() {
            assert_eq!(lack[v][iron], !place.resources.contains(&"iron".to_string()));
        }
        let village = with_crafts(&default_config());
        let geo = crate::geography::build(&village).unwrap();
        assert!(!any_lacking(&lacking(&village, &geo)));
    }

    fn iron_line(name: &str) -> (Config, Geography, SupplyLine) {
        let path = crate::config::configs_dir().join("regions").join(format!("{name}.json"));
        let c = with_crafts(&Config::from_file(path).unwrap());
        let geo = crate::geography::build(&c).unwrap();
        let lines = supply_lines(&c, &geo, &lacking(&c, &geo));
        let line = lines[c.product_index("iron")].clone().expect("iron is brought in");
        assert!(lines.iter().filter(|l| l.is_some()).count() == 1, "only iron is made where a resource is");
        (c, geo, line)
    }

    #[test]
    fn each_place_gets_iron_by_the_cheapest_way_from_a_maker_and_the_costs_multiply_along_it() {
        let (c, _, line) = iron_line("city3");
        let n = c.villages.len();
        let mut seen = vec![false; n];
        for &v in &line.order {
            match line.from[v] {
                None => assert!(line.makers.contains(&v) && line.way[v] == 1.0),
                Some(u) => {
                    assert!(seen[u], "{v} comes after {u}, where it gets its iron");
                    assert!(line.leg[v] > 1.0);
                    assert!((line.way[v] - line.way[u] * line.leg[v]).abs() < 1e-12);
                }
            }
            seen[v] = true;
        }
        assert_eq!(line.order.len(), n, "iron reaches every place");
    }

    #[test]
    fn the_regional_market_shares_a_short_stock_evenly_and_every_coin_paid_goes_to_a_maker_or_a_merchant() {
        let (c, geo, line) = iron_line("city3");
        let n = c.villages.len();
        let place: Vec<u32> = (0..n as u32).collect();
        let prices: Vec<f64> = line.way.iter().map(|w| 2.0 * w).collect();
        let want = vec![10.0; n];
        let mut stock = vec![0.0; n];
        let mut can = vec![0.0; n];
        for (k, &u) in line.makers.iter().enumerate() {
            stock[u] = 1.0 + k as f64;
            can[u] = 5.0;
        }
        let held: f64 = stock.iter().sum();
        let mut money = vec![1e6; n];
        let r = regional_market(&mut money, &want, &place, &prices, &stock, &can, &line, 2.0, &geo);
        for i in 0..n {
            assert!((r.sale.bought[i] - 10.0 * held / (10.0 * n as f64)).abs() < 1e-9, "the same share for each");
            assert!((1e6 - money[i] - r.sale.spent[i]).abs() < 1e-6);
        }
        let paid: f64 = r.sale.spent.iter().sum();
        let taken: f64 = r.paid_to.iter().sum::<f64>() + r.merchants.iter().sum::<f64>();
        assert!((paid - taken).abs() < 1e-9 * paid);
        assert!((r.sold_by.iter().sum::<f64>() - held).abs() < 1e-9, "the makers sold all they had");
        for &u in &line.makers {
            assert!((r.sold_by[u] - stock[u]).abs() < 1e-9, "each by its stock");
        }
        assert!(r.merchants.iter().enumerate().all(|(v, &m)| m == 0.0 || geo.town[v]), "the merchants are in towns");
    }
}
