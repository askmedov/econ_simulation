//! Market towns: their grain dealers, what merchants carry between each
//! village and its town, and the border with the world beyond.
//!
//! On a map, market towns are places of their own, with people but no
//! farmland: they work in crafts and buy their grain. Each town's grain
//! dealers keep a store and coins. They buy grain from the villages that
//! trade with the town when it is cheap enough there to be worth carting,
//! sell it in the town, and send it back out to villages in a dearth. What
//! grain costs in a town is what it costs to bring in (the villages' price
//! plus carriage), times how short its store runs against what its people eat.
//!
//! Merchants carry cloth, tools and firewood the same way, wherever the gap
//! in price beats the cost of carting. Goods brought to a village are sold
//! for the town's business that made them; what is not sold goes back. Only
//! a border town trades with the world beyond, at the outside price.

use crate::config::Config;
use crate::geography::Geography;
use crate::grid::{clip, div_or, Grid};

/// A village whose carriage costs more than this share of the price still
/// reckons its grain worth at least the rest (see `world::MIN_PRICE_LEVEL`).
pub const MIN_GETS: f64 = 0.3;
/// Weight of each month in what a town expects to come in, and what a
/// village expects to send (about the last half year).
pub const MEMORY: f64 = 1.0 / 6.0;

/// numpy's `nan_to_num`: not-a-number as 0, infinities as the largest
/// numbers (or `posinf` for the positive one).
#[inline]
fn nan_to_num(x: f64, posinf: f64) -> f64 {
    if x.is_nan() {
        0.0
    } else if x == f64::INFINITY {
        posinf
    } else if x == f64::NEG_INFINITY {
        f64::MIN
    } else {
        x
    }
}

/// How much of a full load merchants carry for a price gap of `gap` (a
/// share of the price beyond the carting): a full load once it beats the
/// carting by `ramp`.
#[inline]
fn load_share(gap: f64, ramp: f64) -> f64 {
    clip(gap / ramp, 0.0, 1.0)
}

/// Share of the town price a place's grain fetches, after carting it there.
pub fn gets(geo: &Geography) -> Vec<f64> {
    geo.transport.iter().map(|t| clip(1.0 - t, MIN_GETS, 1.0)).collect()
}

/// Move a town's grain mark-up toward the level set by how its dealers'
/// store compares with the usual (`dealers_months` of its need).
pub fn store_markup(markup: &[f64], store: &[f64], need: &[f64], config: &Config) -> Vec<f64> {
    let (low, high) = config.trade.markup_range;
    (0..markup.len())
        .map(|i| {
            let usual = config.town.dealers_months * need[i];
            let short = if usual > 0.0 { usual / store[i].max(1e-9) } else { 1.0 };
            let target = clip(short.powf(config.town.store_elasticity), low, high);
            markup[i] + config.food.price_adjustment * (target - markup[i])
        })
        .collect()
}

/// What grain costs each town to bring from its cheapest neighbouring town
/// (its price last month, plus the route); infinite if it has none.
pub fn from_neighbours(previous: &[f64], geo: &Geography) -> Vec<f64> {
    let r = &geo.routes;
    let mut cheapest = vec![f64::INFINITY; previous.len()];
    for k in 0..r.len() {
        cheapest[r.b[k]] = cheapest[r.b[k]].min(previous[r.a[k]] * (1.0 + r.cost[k]));
    }
    for k in 0..r.len() {
        cheapest[r.a[k]] = cheapest[r.a[k]].min(previous[r.b[k]] * (1.0 + r.cost[k]));
    }
    cheapest
}

/// What grain costs to bring into each town from the villages that trade
/// with it (their price, plus carriage), averaged by `weights` (their need);
/// a village's own price elsewhere.
pub fn delivered_price(village_price: &[f64], geo: &Geography, weights: &[f64]) -> Vec<f64> {
    let mut price = village_price.to_vec();
    let fetches = gets(geo);
    let n = village_price.len();
    for town in (0..n).filter(|&t| geo.town[t]) {
        let mut mine: Vec<usize> = (0..n).filter(|&i| !geo.town[i] && geo.market[i] == town as i64).collect();
        if mine.is_empty() {
            mine = (0..n).filter(|&i| !geo.town[i]).collect();
        }
        if mine.is_empty() {
            continue;
        }
        let (mut total, mut weight) = (0.0, 0.0);
        for &i in &mine {
            let w = weights[i].max(1e-9);
            total += village_price[i] / fetches[i] * w;
            weight += w;
        }
        price[town] = total / weight;
    }
    price
}

/// What merchants carry this month. Grain (per place): what the town's
/// dealers send `grain_in` to each village (and `grain_held` back from each
/// town's own market for it), and what they would buy `grain_out` of a
/// village if it is left unsold. Other goods go on consignment, each move
/// from place `src` to place `dst` (`goods`, moves x products, the food
/// column empty): offered at the price where they arrive and sold for the
/// business that made them; what is not sold goes back.
#[derive(Clone, Debug)]
pub struct Carts {
    pub grain_in: Vec<f64>,
    pub grain_held: Vec<f64>,
    pub grain_out: Vec<f64>,
    pub src: Vec<usize>,
    pub dst: Vec<usize>,
    pub goods: Grid,
}

impl Carts {
    /// Goods arriving at each place (places x products).
    pub fn incoming(&self, n: usize) -> Grid {
        self.gather(n, &self.dst)
    }

    /// Goods leaving each place (places x products).
    pub fn outgoing(&self, n: usize) -> Grid {
        self.gather(n, &self.src)
    }

    fn gather(&self, n: usize, ends: &[usize]) -> Grid {
        let mut out = Grid::zeros(n, self.goods.cols);
        for (m, &place) in ends.iter().enumerate() {
            for p in 0..self.goods.cols {
                *out.at(place, p) += self.goods.get(m, p);
            }
        }
        out
    }
}

/// Merchants' loads for this month, between each village and its market
/// town, wherever a good is dearer at one end than at the other by more than
/// the carting (`transport` x the product's `carriage`), a full load once
/// the gap beats it by `full_load_margin`. Into a village: up to
/// `MapConfig.capacity` of what it `wanted` of a good last month, from the
/// town's `stock` (shared out among its villages if short; for grain, the
/// dealers'). Out of a village to its town: as much again, plus its share
/// (by `weights`) of what the town wanted, from what it has beyond a month
/// of its own orders; grain only as far as the dealers' coins go.
#[allow(clippy::too_many_arguments)]
pub fn carts(
    prices: &Grid,
    geo: &Geography,
    wanted: &Grid,
    stock: &Grid,
    dealers_cash: &[f64],
    weights: &[f64],
    food: usize,
    config: &Config,
) -> Carts {
    let (n, k) = (prices.rows, prices.cols);
    let villages: Vec<usize> =
        (0..n).filter(|&i| !geo.town[i] && geo.market[i] >= 0 && geo.transport[i].is_finite()).collect();
    if villages.is_empty() {
        return Carts {
            grain_in: vec![0.0; n],
            grain_held: vec![0.0; n],
            grain_out: vec![0.0; n],
            src: Vec::new(),
            dst: Vec::new(),
            goods: Grid::zeros(0, k),
        };
    }
    let m = villages.len();
    let town: Vec<usize> = villages.iter().map(|&v| geo.market[v] as usize).collect();
    let carriage: Vec<f64> = config.products.iter().map(|p| p.carriage).collect();
    let ramp = config.map.full_load_margin;
    let mut bring = Grid::zeros(m, k);
    let mut send = Grid::zeros(m, k);
    let mut full = Grid::zeros(m, k);
    let mut full_in = Grid::zeros(m, k);
    for (j, &v) in villages.iter().enumerate() {
        for p in 0..k {
            let cost = (geo.transport[v] * carriage[p]).min(0.9);
            let (here, there) = (prices.get(v, p), prices.get(town[j], p));
            bring.set(j, p, load_share(nan_to_num(here / (there * (1.0 + cost)) - 1.0, f64::MAX), ramp));
            let s = load_share(nan_to_num(there * (1.0 - cost) / here - 1.0, 1.0), ramp);
            send.set(j, p, if here > 0.0 { s } else { 0.0 });
            let f = config.map.capacity * wanted.get(v, p).max(0.0);
            full.set(j, p, f);
            full_in.set(j, p, if p == food { config.map.relief_capacity * wanted.get(v, food).max(0.0) } else { f });
        }
    }
    // Into villages: each town shares out what it has among those asking
    // (grain up to `relief_capacity` of their need).
    let asked = bring.zip_map(&full_in, |b, f| b * f);
    let mut asked_of = Grid::zeros(n, k);
    for j in 0..m {
        for p in 0..k {
            *asked_of.at(town[j], p) += asked.get(j, p);
        }
    }
    let can = Grid {
        rows: n,
        cols: k,
        data: (0..n * k).map(|i| 1.0f64.min(div_or(stock.data[i], asked_of.data[i], 1.0))).collect(),
    };
    let mut into = Grid::zeros(m, k);
    for j in 0..m {
        for p in 0..k {
            into.set(j, p, asked.get(j, p) * can.get(town[j], p));
        }
    }
    // Out of villages: their share of the town's wants too.
    let w: Vec<f64> = villages.iter().map(|&v| weights[v].max(0.0)).collect();
    let mut around = vec![0.0; n];
    for j in 0..m {
        around[town[j]] += w[j];
    }
    let mut out = Grid::zeros(m, k);
    for (j, &v) in villages.iter().enumerate() {
        let share = div_or(w[j], around[town[j]], 0.0);
        for p in 0..k {
            let town_wants = config.map.capacity * wanted.get(town[j], p).max(0.0) * share;
            let beyond = (stock.get(v, p) - wanted.get(v, p)).max(0.0);
            out.set(j, p, (send.get(j, p) * (full.get(j, p) + town_wants)).min(beyond));
        }
    }
    // Grain out: as far as each town's dealers can pay.
    let grain_out: Vec<f64> = (0..m).map(|j| send.get(j, food) * full.get(j, food)).collect();
    let mut offered_to = vec![0.0; n];
    for (j, &v) in villages.iter().enumerate() {
        offered_to[town[j]] += grain_out[j] * prices.get(v, food);
    }
    let afford: Vec<f64> = (0..n).map(|t| 1.0f64.min(div_or(dealers_cash[t], offered_to[t], 1.0))).collect();
    let mut grain_in = vec![0.0; n];
    let mut grain_held = vec![0.0; n];
    let mut grain_sold = vec![0.0; n];
    for (j, &v) in villages.iter().enumerate() {
        grain_in[v] = into.get(j, food);
        grain_held[town[j]] += into.get(j, food);
        grain_sold[v] = grain_out[j] * afford[town[j]];
        into.set(j, food, 0.0);
        out.set(j, food, 0.0);
    }
    let mut src: Vec<usize> = town.clone();
    src.extend_from_slice(&villages);
    let mut dst: Vec<usize> = villages.clone();
    dst.extend_from_slice(&town);
    let mut goods = into.data.clone();
    goods.extend_from_slice(&out.data);
    // Between neighbouring towns, from what each has left beyond a month of
    // its own orders.
    let r = &geo.routes;
    if !r.is_empty() {
        let mut left = stock.clone();
        for j in 0..m {
            for p in 0..k {
                *left.at(town[j], p) -= into.get(j, p);
            }
        }
        let pairs_src: Vec<usize> = r.a.iter().chain(&r.b).cloned().collect();
        let pairs_dst: Vec<usize> = r.b.iter().chain(&r.a).cloned().collect();
        let route_cost: Vec<f64> = r.cost.iter().chain(&r.cost).cloned().collect();
        let water = water_factor(geo, config);
        let water: Vec<f64> = water.iter().chain(&water).cloned().collect();
        let legs = pairs_src.len();
        let mut load = Grid::zeros(legs, k);
        for q in 0..legs {
            for p in 0..k {
                if p == food {
                    continue; // (the dealers trade grain: see `between_towns`)
                }
                let cost = (route_cost[q] * carriage[p]).min(0.9);
                let gap = nan_to_num(
                    prices.get(pairs_dst[q], p) / (prices.get(pairs_src[q], p) * (1.0 + cost)) - 1.0,
                    f64::MAX,
                );
                load.set(
                    q,
                    p,
                    load_share(gap, ramp) * config.map.town_capacity * water[q] * wanted.get(pairs_dst[q], p).max(0.0),
                );
            }
        }
        let mut asked = Grid::zeros(n, k);
        for q in 0..legs {
            for p in 0..k {
                *asked.at(pairs_src[q], p) += load.get(q, p);
            }
        }
        for q in 0..legs {
            for p in 0..k {
                let s = pairs_src[q];
                let spare = (left.get(s, p) - wanted.get(s, p)).max(0.0);
                let scale = 1.0f64.min(div_or(spare, asked.get(s, p), 1.0));
                *load.at(q, p) *= scale;
            }
        }
        src.extend_from_slice(&pairs_src);
        dst.extend_from_slice(&pairs_dst);
        goods.extend_from_slice(&load.data);
    }
    let rows = src.len();
    Carts { grain_in, grain_held, grain_out: grain_sold, src, dst, goods: Grid { rows, cols: k, data: goods } }
}

/// After a market in `product` where each place had `offer` (its own
/// `stock` plus goods carted in), `sold` and took `spent` coins, with buyers
/// having `asked` for as much as `demand`: each unit on offer was as likely
/// to sell as any other, so each business (the place's own, and each that
/// sent goods) is paid and counts the demand for its share, and unsold goods
/// go back. Changes `stock`, `cash` (that product's businesses) and `demand`
/// (all per place).
#[allow(clippy::too_many_arguments)]
pub fn settle(
    cart: &Carts,
    product: usize,
    stock: &mut [f64],
    cash: &mut [f64],
    demand: &mut [f64],
    offer: &[f64],
    sold: &[f64],
    spent: &[f64],
    asked: &[f64],
) {
    let n = stock.len();
    let goods: Vec<f64> = (0..cart.src.len()).map(|q| cart.goods.get(q, product)).collect();
    let part: Vec<f64> = (0..goods.len()).map(|q| div_or(goods[q], offer[cart.dst[q]], 0.0)).collect();
    for i in 0..n {
        let local = clip(div_or(stock[i], offer[i], 1.0), 0.0, 1.0);
        stock[i] -= (sold[i] * local).min(stock[i]);
        cash[i] += spent[i] * local;
        demand[i] = asked[i] * local;
    }
    for q in 0..goods.len() {
        let (s, d) = (cart.src[q], cart.dst[q]);
        stock[s] += goods[q] - sold[d] * part[q];
        cash[s] += spent[d] * part[q];
        demand[s] += asked[d] * part[q];
    }
}

/// Grain a border town's dealers buy from the world beyond (positive) or
/// sell to it (negative) this month, at the `outside` price plus or minus
/// carting: up to `outside_capacity` of the town's monthly `need`, as their
/// coins allow, keeping a month of the town's need in store. Changes
/// `dealers_grain` and `dealers_cash`; returns rations per place.
pub fn border_trade(
    dealers_grain: &mut [f64],
    dealers_cash: &mut [f64],
    town_price: &[f64],
    outside: &[f64],
    need: &[f64],
    geo: &Geography,
    config: &Config,
) -> Vec<f64> {
    let cfg = &config.town;
    let n = need.len();
    let mut moved = vec![0.0; n];
    if !cfg.enabled {
        return moved;
    }
    let ramp = config.map.full_load_margin;
    for i in 0..n {
        let border = geo.border[i] && geo.town[i];
        let buy_at = outside[i] * (1.0 + cfg.outside_transport);
        let sell_at = outside[i] * (1.0 - cfg.outside_transport);
        let buy = if border && buy_at > 0.0 {
            load_share(nan_to_num(town_price[i] / buy_at - 1.0, f64::MAX), ramp)
        } else {
            0.0
        };
        let sell = if border && town_price[i] > 0.0 {
            load_share(nan_to_num(sell_at / town_price[i] - 1.0, f64::MAX), ramp)
        } else {
            0.0
        };
        let load = cfg.outside_capacity * need[i];
        let bought = (buy * load).min(div_or(dealers_cash[i], buy_at, 0.0));
        let sold = (sell * load).min((dealers_grain[i] - cfg.keep_months * need[i]).max(0.0));
        dealers_grain[i] += bought - sold;
        dealers_cash[i] += sold * sell_at - bought * buy_at;
        moved[i] = bought - sold;
    }
    moved
}

/// How much more each route between towns carries than a road (by its share of water).
pub fn water_factor(geo: &Geography, config: &Config) -> Vec<f64> {
    geo.routes.water.iter().map(|w| 1.0 + (config.map.water_capacity - 1.0) * w).collect()
}

/// Grain the dealers of neighbouring towns trade this month: from a town to
/// one dearer by more than the cost of the route between them, up to
/// `town_capacity` of the monthly need of the dearer town's market (the town
/// and the villages that trade with it; more by river), a full load once the
/// gap beats the cost by `full_load_margin`, from what the cheaper town's
/// dealers hold beyond `keep_months` of their own town's need, and as far as
/// the buyers' coins go at the seller's price. Changes the stores and coins;
/// returns grain arriving per place (negative: sent).
pub fn between_towns(
    dealers_grain: &mut [f64],
    dealers_cash: &mut [f64],
    price: &[f64],
    need: &[f64],
    geo: &Geography,
    config: &Config,
) -> Vec<f64> {
    let r = &geo.routes;
    let n = need.len();
    let mut moved = vec![0.0; n];
    if r.is_empty() {
        return moved;
    }
    let mut area = vec![0.0; n];
    for i in 0..n {
        if geo.market[i] >= 0 {
            area[geo.market[i] as usize] += need[i];
        }
    }
    let src: Vec<usize> = r.a.iter().chain(&r.b).cloned().collect();
    let dst: Vec<usize> = r.b.iter().chain(&r.a).cloned().collect();
    let cost: Vec<f64> = r.cost.iter().chain(&r.cost).cloned().collect();
    let water = water_factor(geo, config);
    let water: Vec<f64> = water.iter().chain(&water).cloned().collect();
    let ramp = config.map.full_load_margin;
    let legs = src.len();
    let mut load: Vec<f64> = (0..legs)
        .map(|q| {
            let gap = nan_to_num(price[dst[q]] / (price[src[q]] * (1.0 + cost[q])) - 1.0, f64::MAX);
            load_share(gap, ramp) * config.map.town_capacity * water[q] * area[dst[q]]
        })
        .collect();
    let spare: Vec<f64> = (0..n).map(|i| (dealers_grain[i] - config.town.keep_months * need[i]).max(0.0)).collect();
    let mut asked = vec![0.0; n];
    for q in 0..legs {
        asked[src[q]] += load[q];
    }
    for q in 0..legs {
        load[q] *= 1.0f64.min(div_or(spare[src[q]], asked[src[q]], 1.0));
    }
    let mut value: Vec<f64> = (0..legs).map(|q| load[q] * price[src[q]]).collect();
    let mut owed = vec![0.0; n];
    for q in 0..legs {
        owed[dst[q]] += value[q];
    }
    for q in 0..legs {
        let afford = 1.0f64.min(div_or(dealers_cash[dst[q]], owed[dst[q]], 1.0));
        load[q] *= afford;
        value[q] *= afford;
    }
    for q in 0..legs {
        dealers_grain[src[q]] -= load[q];
        dealers_grain[dst[q]] += load[q];
        dealers_cash[dst[q]] -= value[q];
        dealers_cash[src[q]] += value[q];
        moved[dst[q]] += load[q];
        moved[src[q]] -= load[q];
    }
    moved
}

/// Coins the world beyond takes in (net) for the grain of `border_trade`.
pub fn outside_coins(moved: &[f64], outside: &[f64], config: &Config) -> f64 {
    let ot = config.town.outside_transport;
    moved.iter().zip(outside).map(|(&m, &o)| m.max(0.0) * o * (1.0 + ot) - (-m).max(0.0) * o * (1.0 - ot)).sum()
}

/// Coins the grain dealers pay out this month (merchants' and carters'
/// earnings): `dealers_payout` of what they hold beyond `dealers_months` of
/// the town's need at its price. Changes `dealers_cash`; returns coins per place.
pub fn dealers_payout(dealers_cash: &mut [f64], need: &[f64], price: &[f64], config: &Config) -> Vec<f64> {
    let cfg = &config.town;
    (0..need.len())
        .map(|i| {
            let working = cfg.dealers_months * need[i] * price[i];
            let paid = cfg.dealers_payout * (dealers_cash[i] - working).max(0.0);
            dealers_cash[i] -= paid;
            paid
        })
        .collect()
}
