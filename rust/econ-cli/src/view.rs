//! A page to watch a run (`econ-sim view`): the region's map month by
//! month, coloured by a measure of each place, with the region's measures
//! over time and its chronicle alongside, and the paired run with the
//! forced events beside the one without.
//!
//! The runs are the ones `econ-sim` makes from the same options (the same
//! seed and settings, so the same records, month for month); the page only
//! reads them. Each month it also reads, from the world, what the engine
//! doesn't keep per place: the prices of every good, pay, coins and debts,
//! grain in store, the woods and the workers of the resource trades.
//! Everything goes into one HTML file with the data in it, to open in any
//! browser, offline too (the page's code is in `econ-cli/viewer/`).

use std::path::Path;

use econ_core::chronicle::{self, Chronicle};
use econ_core::config::{Config, ScheduledEvent};
use econ_core::geography;
use econ_core::households;
use econ_core::metrics::{Cell, MonthRecord};
use econ_core::rules;
use econ_core::starts;
use econ_core::Simulation;
use serde_json::{json, Value};

use crate::options::RunArgs;
use crate::report;

const PAGE: &str = include_str!("../viewer/page.html");
const STYLE: &str = include_str!("../viewer/viewer.css");
const SCRIPT: &str = include_str!("../viewer/viewer.js");

/// Months a page shows unless --months says otherwise: ten years, the
/// longest a run need be.
pub const MONTHS: i64 = 120;

/// Above this many places the map shows each market town with its
/// countryside added in, rather than every village (ten million people
/// make 25,584 places: too many to draw and too much data for one page).
pub const MOST_PLACES: usize = 5_000;

/// How the page is to show a measure on the map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Show {
    Change, // against the start (people)
    Rate,   // a count, per 1,000 people a month
    Count,  // a count as it is (workers)
    Level,  // a price or a sum, as it is
    Share,  // a share, 0 to 1
}

impl Show {
    fn name(self) -> &'static str {
        match self {
            Show::Change => "change",
            Show::Rate => "rate",
            Show::Count => "count",
            Show::Level => "level",
            Show::Share => "share",
        }
    }
}

/// How finely the page keeps a measure: to 1/scale (a whole number of
/// people, a tenth of a percent), or to `RELATIVE` of its value (a price,
/// coins), which is as fine as a colour or a chart can show and takes far
/// fewer bytes. A relative measure is kept as a whole number of steps of
/// ln(1 + value / unit), so that 0 is kept exactly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kept {
    Scaled(f64),
    Relative { unit: f64 },
}

/// The step of a measure kept relative: 0.4% of its value.
pub const RELATIVE: f64 = 0.004;

impl Kept {
    /// `value` as a whole number of steps.
    pub fn whole(self, value: f64) -> i64 {
        match self {
            Kept::Scaled(scale) => (value * scale).round() as i64,
            Kept::Relative { unit } => ((value.max(0.0) / unit).ln_1p() / RELATIVE.ln_1p()).round() as i64,
        }
    }

    /// The value of `whole` steps (as the page reads it, in viewer.js).
    #[cfg(test)]
    pub fn value(self, whole: i64) -> f64 {
        match self {
            Kept::Scaled(scale) => whole as f64 / scale,
            Kept::Relative { unit } => unit * (whole as f64 * RELATIVE.ln_1p()).exp_m1(),
        }
    }

    fn data(self) -> Value {
        match self {
            Kept::Scaled(scale) => json!({"scale": scale}),
            Kept::Relative { unit } => json!({"unit": unit, "step": RELATIVE.ln_1p()}),
        }
    }
}

/// What a group of places has of a measure: the sum of a count, or the mean
/// of a level weighted by people, families or the woods they had.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weight {
    Sum,
    People,
    Families,
    Woods,
}

/// A measure the page shows for each place.
#[derive(Clone, Debug)]
pub struct Measure {
    pub key: String,
    pub label: String,
    pub group: &'static str,
    pub unit: &'static str,
    pub about: String,
    /// How finely the page keeps it.
    pub kept: Kept,
    pub weight: Weight,
    pub show: Show,
    /// Which way is worse: "low" (fewer people, less food) or "high"
    /// (dearer, more deaths).
    pub worse: &'static str,
}

#[allow(clippy::too_many_arguments)]
fn measure(
    key: &str,
    label: &str,
    group: &'static str,
    unit: &'static str,
    about: &str,
    kept: Kept,
    weight: Weight,
    show: Show,
    worse: &'static str,
) -> Measure {
    Measure { key: key.into(), label: label.into(), group, unit, about: about.into(), kept, weight, show, worse }
}

/// Prices and coins are kept to `RELATIVE` of their value, down to a
/// thousandth of a coin.
const COINS: Kept = Kept::Relative { unit: 0.001 };
/// Shares to a tenth of a percent.
const SHARE: Kept = Kept::Scaled(1000.0);
/// Counts as whole numbers.
const WHOLE: Kept = Kept::Scaled(1.0);

/// What a good is called on the page.
fn good_name(product: &str) -> String {
    match product {
        "clothing" => "Cloth".into(),
        "pottery" => "Pots".into(),
        other => {
            let mut name = other.replace('_', " ");
            name[..1].make_ascii_uppercase();
            name
        }
    }
}

/// What the workers of a trade are called on the page.
fn workers_name(business: &str) -> String {
    match business {
        "ironworks" => "Ironworkers".into(),
        "leadworks" => "Leadworkers".into(),
        "saltworks" => "Salt boilers".into(),
        "potting" => "Potters".into(),
        "smithing" => "Smiths".into(),
        other => format!("Workers in {}", other.replace('_', " ")),
    }
}

/// The measures of each place a run of `config` (with its crafts added, as
/// the simulation has it) can show, in the order the page lists them.
pub fn measures(config: &Config) -> Vec<Measure> {
    let mut out = vec![
        measure(
            "ration",
            "Food eaten",
            "Food",
            "of need",
            "The share of the food they need that people ate this month: under 97% they go short, under 80% it is famine.",
            SHARE,
            Weight::People,
            Show::Share,
            "low",
        ),
        measure(
            "store",
            "Grain in store",
            "Food",
            "rations a head",
            "Grain held by families, farms and grain dealers, in rations a head (a ration feeds a grown-up for a month).",
            Kept::Relative { unit: 0.01 },
            Weight::People,
            Show::Level,
            "low",
        ),
        measure(
            "deaths",
            "Deaths",
            "Food",
            "a month per 1,000",
            "People who died this month, per 1,000 people.",
            WHOLE,
            Weight::Sum,
            Show::Rate,
            "high",
        ),
        measure(
            "population",
            "People",
            "People",
            "against the start",
            "People living here, against how many lived here at the start.",
            WHOLE,
            Weight::Sum,
            Show::Change,
            "low",
        ),
        measure(
            "left",
            "Leaving",
            "People",
            "a month per 1,000",
            "People who left this month, for a town, another village or beyond the region, per 1,000 people.",
            WHOLE,
            Weight::Sum,
            Show::Rate,
            "high",
        ),
        measure(
            "arrived",
            "Arriving",
            "People",
            "a month per 1,000",
            "People who came this month, from another place or from beyond the region, per 1,000 people.",
            WHOLE,
            Weight::Sum,
            Show::Rate,
            "low",
        ),
        measure(
            "births",
            "Births",
            "People",
            "a month per 1,000",
            "Children born this month, per 1,000 people.",
            WHOLE,
            Weight::Sum,
            Show::Rate,
            "low",
        ),
        measure(
            "food_price",
            "Grain",
            "Prices",
            "coins a ration",
            "What a ration of grain costs here this month.",
            COINS,
            Weight::People,
            Show::Level,
            "high",
        ),
    ];
    // Then the other goods, the ones every family buys first.
    let order = ["tool", "staple", "comfort", "heating", "material"];
    let mut goods: Vec<(usize, &str)> = config
        .products
        .iter()
        .filter(|p| p.use_ != "food")
        .map(|p| (order.iter().position(|&u| u == p.use_).unwrap_or(order.len()), p.name.as_str()))
        .collect();
    goods.sort_by_key(|&(rank, _)| rank);
    for (_, name) in goods {
        let label = good_name(name);
        out.push(Measure {
            key: format!("price_{name}"),
            about: format!("What a unit of {} costs here this month.", label.to_lowercase()),
            label,
            group: "Prices",
            unit: "coins a unit",
            kept: COINS,
            weight: Weight::People,
            show: Show::Level,
            worse: "high",
        });
    }
    out.extend([
        measure(
            "pay",
            "Pay",
            "Money",
            "coins a month",
            "What a worker earns here in a month (a running average over the trades, by their workers).",
            COINS,
            Weight::People,
            Show::Level,
            "low",
        ),
        measure(
            "coins",
            "Coins",
            "Money",
            "coins a head",
            "Coins families hold, a head.",
            COINS,
            Weight::People,
            Show::Level,
            "low",
        ),
        Measure {
            key: "debt".into(),
            label: "Debt".into(),
            group: "Money",
            unit: "coins a family",
            about: format!(
                "Coins families owe the place's lenders, a family; debts grow by {:.0}% a year.",
                100.0 * config.credit.interest
            ),
            kept: COINS,
            weight: Weight::Families,
            show: Show::Level,
            worse: "high",
        },
        measure(
            "landless",
            "Landless",
            "Money",
            "of people",
            "The share of people in families that hold no land (all of a town's).",
            SHARE,
            Weight::People,
            Show::Share,
            "high",
        ),
    ]);
    if config.products.iter().any(|p| p.use_ == "staple") {
        out.push(measure(
            "staples_bought",
            "Salt got",
            "Trades and resources",
            "of need",
            "The share of the salt they need that families bought this month.",
            SHARE,
            Weight::People,
            Show::Share,
            "low",
        ));
    }
    out.push(measure(
        "woods",
        "Woods",
        "Trades and resources",
        "of the start",
        "Wood standing in the place's woods, against what they held at the start.",
        SHARE,
        Weight::Woods,
        Show::Share,
        "low",
    ));
    for business in config.businesses.iter().filter(|b| b.resource.is_some()) {
        let resource = business.resource.as_deref().unwrap_or_default();
        out.push(Measure {
            key: format!("jobs_{}", business.name),
            label: workers_name(&business.name),
            group: "Trades and resources",
            unit: "workers",
            about: format!(
                "People working in {} here: only where there is {resource}.",
                business.name.replace('_', " ")
            ),
            kept: WHOLE,
            weight: Weight::Sum,
            show: Show::Count,
            worse: "low",
        });
    }
    out
}

/// One arm of the page: a run, and its measures per place month by month.
pub struct Watched {
    pub records: Vec<MonthRecord>,
    pub chronicle: Chronicle,
    /// Per measure (in the order of `measures`): month by month, per place.
    pub values: Vec<Vec<Vec<f64>>>,
    /// People and families per place, month by month, to weigh by.
    pub people: Vec<Vec<f64>>,
    pub families: Vec<Vec<f64>>,
    /// What each place's woods held at the start.
    pub woods: Vec<f64>,
}

/// Run `config` month by month as `econ-sim` does, and read each place's
/// measures after every month.
pub fn watch(config: Config) -> Result<Watched, String> {
    let mut sim = Simulation::new(config)?;
    let measures = measures(&sim.config);
    let months = sim.config.months.max(0) as usize;
    let mut values: Vec<Vec<Vec<f64>>> = vec![Vec::with_capacity(months); measures.len()];
    let (mut people, mut families) = (Vec::with_capacity(months), Vec::with_capacity(months));
    for _ in 0..months {
        sim.step();
        let month = read_places(&sim, &measures);
        for (k, v) in month.values.into_iter().enumerate() {
            if let Some(v) = v {
                values[k].push(v);
            }
        }
        people.push(month.people);
        families.push(month.families);
    }
    // The measures the simulation keeps itself.
    for (name, kept) in sim.place_series() {
        if let Some(k) = measures.iter().position(|m| m.key == name) {
            values[k] = kept;
        }
    }
    let woods = sim.world.woods_normal.clone();
    Ok(Watched { records: sim.records, chronicle: sim.chronicle, values, people, families, woods })
}

/// What a month left in each place: a value per place for each measure the
/// simulation doesn't keep (None for those it keeps), and people and
/// families per place.
struct PlacesNow {
    values: Vec<Option<Vec<f64>>>,
    people: Vec<f64>,
    families: Vec<f64>,
}

fn read_places(sim: &Simulation, measures: &[Measure]) -> PlacesNow {
    let world = &sim.world;
    let config = &sim.config;
    let n = world.n_locations();
    let nb = config.businesses.len();
    let pop = &world.population;
    let hh = &world.households;
    let people = rules::people_by_location(pop, n);
    let size = households::sizes(pop, hh.len());
    let (mut families, mut landless, mut coins, mut debt) = (vec![0.0; n], vec![0.0; n], vec![0.0; n], vec![0.0; n]);
    for (h, &members) in size.iter().enumerate() {
        let v = hh.location[h] as usize;
        coins[v] += hh.money[h];
        debt[v] += hh.debt[h];
        if members > 0.0 {
            families[v] += 1.0;
            if hh.land[h] <= 0.0 {
                landless[v] += members;
            }
        }
    }
    let mut workers = vec![0.0; n * nb];
    for i in 0..pop.len() {
        if pop.job[i] >= 0 && (pop.job[i] as usize) < nb {
            workers[pop.location[i] as usize * nb + pop.job[i] as usize] += pop.count[i] as f64;
        }
    }
    let per = |a: f64, b: f64| if b > 0.0 { a / b } else { 0.0 };
    let granary = world.granary();
    let kept: Vec<&str> = econ_core::simulation::PLACE_SERIES.to_vec();
    let values = measures
        .iter()
        .map(|m| {
            if kept.contains(&m.key.as_str()) {
                return None;
            }
            let value: Vec<f64> = match m.key.as_str() {
                "store" => (0..n).map(|v| per(granary[v], people[v])).collect(),
                "pay" => (0..n)
                    .map(|v| {
                        let here = &workers[v * nb..(v + 1) * nb];
                        let paid: f64 = (0..nb).map(|b| world.pay.get(v, b) * here[b]).sum();
                        per(paid, here.iter().sum())
                    })
                    .collect(),
                "coins" => (0..n).map(|v| per(coins[v], people[v])).collect(),
                "debt" => (0..n).map(|v| per(debt[v], families[v])).collect(),
                "landless" => (0..n).map(|v| per(landless[v], people[v])).collect(),
                "woods" => (0..n)
                    .map(|v| if world.woods_normal[v] > 0.0 { world.woods[v] / world.woods_normal[v] } else { 1.0 })
                    .collect(),
                key => {
                    if let Some(product) = key.strip_prefix("price_") {
                        let p = config.product_index(product);
                        (0..n).map(|v| world.prices.get(v, p)).collect()
                    } else if let Some(business) = key.strip_prefix("jobs_") {
                        let b = config.businesses.iter().position(|s| s.name == business).expect("a trade of the run");
                        (0..n).map(|v| workers[v * nb + b]).collect()
                    } else {
                        unreachable!("no way to read {key} per place")
                    }
                }
            };
            Some(value.into_iter().map(|x| if x.is_finite() { x } else { 0.0 }).collect())
        })
        .collect();
    PlacesNow { values, people, families }
}

/// The places as the page shows them: every place, or (in a big region)
/// each market town with the villages that market there.
pub struct Shown {
    /// The shown place of each place of the run.
    pub of: Vec<usize>,
    /// The place of the run each shown place is drawn as (a town, or a
    /// village of its own).
    pub head: Vec<usize>,
    pub by_town: bool,
}

impl Shown {
    pub fn every_place(n: usize) -> Shown {
        Shown { of: (0..n).collect(), head: (0..n).collect(), by_town: false }
    }

    /// Each town with its countryside (each village's market town); a
    /// village no town reaches is shown on its own.
    pub fn by_town(market: &[i64]) -> Shown {
        let n = market.len();
        let mut of = vec![usize::MAX; n];
        let mut head = Vec::new();
        for (v, &m) in market.iter().enumerate() {
            if m == v as i64 || m < 0 {
                of[v] = head.len();
                head.push(v);
            }
        }
        for (v, &m) in market.iter().enumerate() {
            if of[v] == usize::MAX {
                of[v] = of[m as usize];
            }
        }
        Shown { of, head, by_town: true }
    }

    pub fn len(&self) -> usize {
        self.head.len()
    }

    /// A month's values per place, gathered to the shown places: summed, or
    /// averaged with `weights`.
    fn gather(&self, values: &[f64], weights: Option<&[f64]>) -> Vec<f64> {
        let mut total = vec![0.0; self.len()];
        let mut weight = vec![0.0; self.len()];
        for (v, &x) in values.iter().enumerate() {
            let w = weights.map_or(1.0, |w| w[v]);
            total[self.of[v]] += x * w;
            weight[self.of[v]] += w;
        }
        match weights {
            None => total,
            Some(_) => total.iter().zip(&weight).map(|(t, w)| if *w > 0.0 { t / w } else { 0.0 }).collect(),
        }
    }
}

/// One arm's measures for the shown places: per measure, month by month.
fn shown_values(arm: &Watched, measures: &[Measure], shown: &Shown) -> Vec<Vec<Vec<f64>>> {
    measures
        .iter()
        .zip(&arm.values)
        .map(|(m, months)| {
            months
                .iter()
                .enumerate()
                .map(|(t, values)| {
                    if !shown.by_town {
                        return values.clone();
                    }
                    match m.weight {
                        Weight::Sum => shown.gather(values, None),
                        Weight::People => shown.gather(values, Some(&arm.people[t])),
                        Weight::Families => shown.gather(values, Some(&arm.families[t])),
                        Weight::Woods => shown.gather(values, Some(&arm.woods)),
                    }
                })
                .collect()
        })
        .collect()
}

/// Pack the arms' measures (arm, measure, month, place) small: each value
/// kept to its measure's precision as a whole number, the second arm as
/// its gap from the first (nothing before the forced events), each place's
/// months as the change from the month before, each change in as few bytes
/// as it needs, and the whole squeezed with deflate (zlib), which the page
/// unpacks with the browser's own DecompressionStream.
pub fn pack(arms: &[Vec<Vec<Vec<f64>>>], measures: &[Measure]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (a, arm) in arms.iter().enumerate() {
        for (k, months) in arm.iter().enumerate() {
            let kept = measures[k].kept;
            let places = months.first().map_or(0, |m| m.len());
            for p in 0..places {
                let mut before = 0i64;
                for (t, month) in months.iter().enumerate() {
                    let mut q = kept.whole(month[p]);
                    if a > 0 {
                        q -= kept.whole(arms[0][k][t][p]);
                    }
                    write_varint(&mut bytes, zigzag(q - before));
                    before = q;
                }
            }
        }
    }
    miniz_oxide::deflate::compress_to_vec_zlib(&bytes, 9)
}

fn zigzag(x: i64) -> u64 {
    ((x << 1) ^ (x >> 63)) as u64
}

fn write_varint(out: &mut Vec<u8>, mut x: u64) {
    while x >= 0x80 {
        out.push((x as u8 & 0x7f) | 0x80);
        x >>= 7;
    }
    out.push(x as u8);
}

/// The inverse of `pack` without the deflate, for the tests: values per
/// (arm, measure, month, place).
#[cfg(test)]
#[allow(clippy::needless_range_loop)]
fn unpack(raw: &[u8], arms: usize, measures: &[Measure], months: usize, places: usize) -> Vec<Vec<Vec<Vec<f64>>>> {
    let mut at = 0;
    let mut next = || {
        let (mut x, mut shift) = (0u64, 0);
        loop {
            let b = raw[at];
            at += 1;
            x |= ((b & 0x7f) as u64) << shift;
            shift += 7;
            if b < 0x80 {
                break;
            }
        }
        (x >> 1) as i64 ^ -((x & 1) as i64)
    };
    let mut q = vec![vec![vec![vec![0i64; places]; months]; measures.len()]; arms];
    for a in 0..arms {
        for k in 0..measures.len() {
            for p in 0..places {
                let mut value = 0;
                for t in 0..months {
                    value += next();
                    q[a][k][t][p] = value;
                }
            }
        }
    }
    (0..arms)
        .map(|a| {
            (0..measures.len())
                .map(|k| {
                    (0..months)
                        .map(|t| {
                            (0..places)
                                .map(|p| {
                                    let whole = if a > 0 { q[a][k][t][p] + q[0][k][t][p] } else { q[a][k][t][p] };
                                    measures[k].kept.value(whole)
                                })
                                .collect()
                        })
                        .collect()
                })
                .collect()
        })
        .collect()
}

/// Bytes as base64, to carry them in the page.
pub fn base64(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(DIGITS[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// `value` to `digits` significant digits (the region's measures need no
/// more on a chart).
fn significant(value: f64, digits: i32) -> f64 {
    if value == 0.0 || !value.is_finite() {
        return 0.0;
    }
    let scale = 10f64.powi(digits - 1 - value.abs().log10().floor() as i32);
    (value * scale).round() / scale
}

/// The region's records as columns (name: month by month).
fn columns(records: &[MonthRecord]) -> Value {
    let mut out = serde_json::Map::new();
    for record in records {
        for (name, cell) in record.flat() {
            let value = match cell {
                Cell::Int(v) => json!(v),
                Cell::Float(v) => json!(significant(v, 6)),
                Cell::Text(t) => json!(t),
            };
            match out.get_mut(&name) {
                Some(Value::Array(column)) => column.push(value),
                _ => {
                    out.insert(name, json!([value]));
                }
            }
        }
    }
    Value::Object(out)
}

/// An arm's chronicle: every entry (all four ranks), with its places as
/// shown places, and each year's line of totals and the year in brief.
fn chronicle_data(arm: &Watched, names: &[String], shown: &Shown) -> Value {
    let index: std::collections::HashMap<&str, usize> =
        names.iter().enumerate().map(|(i, name)| (name.as_str(), i)).collect();
    let entries: Vec<Value> = arm
        .chronicle
        .entries
        .iter()
        .map(|e| {
            let places: Vec<Value> = e
                .places
                .iter()
                .map(|d| json!([index.get(d.place.as_str()).map_or(-1, |&i| shown.of[i] as i64), d.place, d.what]))
                .collect();
            json!({"m": e.month_number, "r": e.rank, "k": e.kind, "w": e.what, "n": e.people.round(), "p": places, "more": e.more_places})
        })
        .collect();
    let mut years: Vec<i64> = arm.records.iter().map(|r| r.year).collect();
    years.dedup();
    let years: Vec<Value> = years
        .iter()
        .map(|&y| json!({"year": y, "line": chronicle::year_line(&arm.records, y), "brief": arm.chronicle.in_brief(y)}))
        .collect();
    json!({"entries": entries, "years": years})
}

/// What the page needs: the run, its map, its arms' measures (packed), the
/// region's records and the chronicles.
pub struct Page {
    pub title: String,
    pub data: Value,
}

/// The page's data from finished arms (the first without the forced events,
/// the second with them, if any).
pub fn page_data(
    config: &Config,
    forced: &[ScheduledEvent],
    arms: &[Watched],
    shown: &Shown,
    about: Value,
) -> Result<Value, String> {
    let with_crafts = econ_core::crafts::with_crafts(config);
    let measures = measures(&with_crafts);
    let geo = geography::build(config)?;
    let names: Vec<String> = config.villages.iter().map(|v| v.name.clone()).collect();
    let n = shown.len();
    // The shown places: each drawn where its head is, with what its places have.
    let mut people = vec![0i64; n];
    let mut villages = vec![0usize; n];
    let mut resources: Vec<Vec<String>> = vec![Vec::new(); n];
    for (v, village) in config.villages.iter().enumerate() {
        let s = shown.of[v];
        people[s] += village.population;
        villages[s] += (village.kind == "village") as usize;
        for r in &village.resources {
            if !resources[s].contains(r) {
                resources[s].push(r.clone());
            }
        }
    }
    let head = |f: &dyn Fn(usize) -> Value| -> Vec<Value> { shown.head.iter().map(|&v| f(v)).collect() };
    let market: Vec<i64> = shown
        .head
        .iter()
        .map(|&v| if geo.market[v] < 0 { -1 } else { shown.of[geo.market[v] as usize] as i64 })
        .collect();
    let links = |pairs: &[(usize, usize)]| -> Vec<(usize, usize)> {
        let mut out: Vec<(usize, usize)> = pairs
            .iter()
            .map(|&(a, b)| (shown.of[a].min(shown.of[b]), shown.of[a].max(shown.of[b])))
            .filter(|(a, b)| a != b)
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    };
    let roads = links(&config.map.roads);
    let rivers = links(&config.map.rivers);
    let watched: Vec<Vec<Vec<Vec<f64>>>> = arms.iter().map(|arm| shown_values(arm, &measures, shown)).collect();
    let packed = pack(&watched, &measures);
    let months = config.months;
    let series: Vec<Value> = measures
        .iter()
        .map(|m| {
            json!({"key": m.key, "label": m.label, "group": m.group, "unit": m.unit, "about": m.about,
                   "kept": m.kept.data(), "show": m.show.name(), "worse": m.worse, "sum": m.weight == Weight::Sum})
        })
        .collect();
    let events: Vec<Value> =
        forced.iter().map(|e| json!({"event": e.event, "month": e.month, "label": report::forced_label(e)})).collect();
    let arm_labels: Vec<Value> = if arms.len() > 1 {
        let what = forced_names(forced);
        vec![
            json!({"key": "without", "label": format!("Without {what}")}),
            json!({"key": "with", "label": format!("With {what}")}),
        ]
    } else {
        vec![json!({"key": "run", "label": "The run"})]
    };
    Ok(json!({
        "about": about,
        "months": months,
        "start_month": config.start_month,
        "arms": arm_labels,
        "events": events,
        "places": {
            "name": head(&|v| json!(names[v])),
            "kind": head(&|v| json!(config.villages[v].kind)),
            "terrain": head(&|v| json!(config.villages[v].terrain)),
            "x": head(&|v| json!(significant(geo.x[v], 5))),
            "y": head(&|v| json!(significant(geo.y[v], 5))),
            "border": head(&|v| json!(config.villages[v].border)),
            "stronghold": head(&|v| json!(config.villages[v].stronghold)),
            "people": people,
            "villages": villages,
            "resources": resources,
            "market": market,
        },
        "by_town": shown.by_town,
        "roads": roads,
        "rivers": rivers,
        "series": series,
        "packed": base64(&packed),
        "records": arms.iter().map(|arm| columns(&arm.records)).collect::<Vec<_>>(),
        "chronicle": arms.iter().map(|arm| chronicle_data(arm, &names, shown)).collect::<Vec<_>>(),
        "ranks": econ_core::chronicle::RANKS,
        "thresholds": {"famine": config.chronicle.famine_ration, "short": config.chronicle.short_ration},
        "calendar": {
            "harvest": config.work.harvest_months,
            "sowing": config.food.seed_months,
            "shearing": config.livestock.shearing_month,
            "tax": if config.state.enabled { json!(config.state.collection_month) } else { Value::Null },
        },
    }))
}

/// The page itself: the template with its style, script and data put in.
pub fn page_html(page: &Page) -> String {
    let escaped = page.title.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
    // (inside a script element, a "<" in the data could end it: JSON writes it as <)
    let data = page.data.to_string().replace('<', "\\u003c");
    PAGE.replace("{{TITLE}}", &escaped)
        .replace("/*{{STYLE}}*/", STYLE)
        .replace("/*{{SCRIPT}}*/", SCRIPT)
        .replacen("{{DATA}}", &data, 1)
}

/// The forced events by name: "the drought", "the drought and the plague".
fn forced_names(forced: &[ScheduledEvent]) -> String {
    let mut names: Vec<String> = Vec::new();
    for e in forced {
        let name = format!("the {}", e.event.replace('_', " "));
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names.join(" and ")
}

/// The title a page gets unless --title gives one: what was forced, and from where.
fn default_title(args: &RunArgs, forced: &[ScheduledEvent]) -> String {
    let what = if forced.is_empty() {
        "The region".to_string()
    } else {
        let mut what = report::event_label(forced);
        what[..1].make_ascii_uppercase();
        what
    };
    match &args.start {
        Some(start) => format!("{what}, from {}", start.replace('-', " ")),
        None => what,
    }
}

/// `econ-sim view`: run the options' region (and, with forced events, the
/// paired run) and write the page to `page`.
pub fn run(args: &RunArgs, page_path: Option<&str>, title: Option<&str>, by_town: bool) -> Result<(), String> {
    let mut config = args.config()?;
    if args.months.is_none() {
        config.months = MONTHS;
    }
    if !config.map.enabled {
        return Err("the viewer shows a region on a map: use --size, --region or a region's --config".into());
    }
    if args.runs != 1 {
        return Err("the viewer shows one run and its pair: choose the seed with --seed".into());
    }
    let forced = args.forced()?;
    let mut with_events = config.clone();
    with_events.scheduled_events.extend_from_slice(&forced);
    let mut check = with_events.clone();
    check.months = 0;
    Simulation::new(check)?;

    let geo = geography::build(&config)?;
    let shown = if by_town || config.villages.len() > MOST_PLACES {
        Shown::by_town(&geo.market)
    } else {
        Shown::every_place(config.villages.len())
    };
    println!("{}", geography::describe(&config, &geo));
    println!(
        "{} months; seed {}{}{}",
        config.months,
        config.seed,
        if forced.is_empty() { String::new() } else { format!("; {}", report::event_label(&forced)) },
        if shown.by_town {
            format!("; shown as {} market towns with their countryside", shown.len())
        } else {
            String::new()
        }
    );
    let started = std::time::Instant::now();
    let arms = if forced.is_empty() {
        vec![watch(config.clone())?]
    } else {
        let (without, with) = rayon::join(|| watch(config.clone()), || watch(with_events));
        vec![without?, with?]
    };

    let people: i64 = config.villages.iter().map(|v| v.population).sum();
    let start_about = match &args.start {
        Some(name) => starts::find(name).map(|s| s.about).unwrap_or_default(),
        None => String::new(),
    };
    let mut command: Vec<String> = std::env::args().collect();
    if let Some(first) = command.first_mut() {
        *first = "econ-sim".into();
    }
    let about = json!({
        "people": people,
        "places": config.villages.len(),
        "towns": config.villages.iter().filter(|v| v.kind == "town").count(),
        "seed": config.seed,
        "start": args.start,
        "start_about": start_about,
        "changes": args.sets.iter().chain(&args.scales).collect::<Vec<_>>(),
        "random_events": config.random_events,
        "minds": config.minds.enabled,
        "command": command.join(" "),
        "map": geography::describe(&config, &geo),
    });
    let page = Page {
        title: title.map_or_else(|| default_title(args, &forced), str::to_string),
        data: page_data(&config, &forced, &arms, &shown, about)?,
    };
    let html = page_html(&page);
    let path = match page_path {
        Some(path) => Path::new(path).to_path_buf(),
        None => Path::new(&args.out).join("viewer.html"),
    };
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| format!("writing to {}: {e}", parent.display()))?;
    }
    std::fs::write(&path, &html).map_err(|e| format!("writing {}: {e}", path.display()))?;
    println!(
        "Ran in {:.0} s. Wrote {} ({:.1} MB): open it in a browser",
        started.elapsed().as_secs_f64(),
        path.display(),
        html.len() as f64 / 1e6
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use econ_core::config::standard_region;
    use econ_core::config::Standard;

    fn small_region(months: i64) -> Config {
        let mut config = Config::from_file(econ_core::config::configs_dir().join("regions").join("region20.json"))
            .expect("the region of 20 villages");
        config.months = months;
        config
    }

    #[test]
    fn packed_numbers_come_back_to_their_precision() {
        let measures = vec![
            measure("a", "A", "G", "", "", WHOLE, Weight::Sum, Show::Count, "low"),
            measure("b", "B", "G", "", "", SHARE, Weight::People, Show::Share, "low"),
            measure("c", "C", "G", "", "", COINS, Weight::People, Show::Level, "high"),
        ];
        // two arms; three measures, three months, two places each
        let base = vec![
            vec![vec![10.0, 200.0], vec![12.0, 190.0], vec![9.0, 0.0]],
            vec![vec![0.5, 1.0], vec![0.98765, 0.0], vec![-0.25, 2.5]],
            vec![vec![1.0, 0.0], vec![2.75, 0.31], vec![4000.0, 0.0004]],
        ];
        let mut with = base.clone();
        with[0][2][1] = 7.0;
        with[1][1][0] = 0.5;
        with[2][1][1] = 0.62;
        let arms = vec![base, with];
        let packed = pack(&arms, &measures);
        let raw = miniz_oxide::inflate::decompress_to_vec_zlib(&packed).unwrap();
        let back = unpack(&raw, 2, &measures, 3, 2);
        // counts exactly, shares to a tenth of a percent
        assert_eq!(back[0][0], arms[0][0]);
        assert_eq!(back[1][0], arms[1][0]);
        assert_eq!(back[0][1][1][0], 0.988);
        assert_eq!(back[1][1][1][0], 0.5);
        assert_eq!(back[1][1][2], vec![-0.25, 2.5]);
        // prices within a fifth of a percent, and nothing kept as nothing
        for a in 0..2 {
            for t in 0..3 {
                for p in 0..2 {
                    let (was, is) = (arms[a][2][t][p], back[a][2][t][p]);
                    assert!((is - was).abs() <= 0.0021 * was + 1e-5, "{was} came back as {is}");
                }
            }
        }
        assert_eq!(back[0][2][0][1], 0.0);
    }

    #[test]
    fn base64_is_written_as_browsers_read_it() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0xfb, 0xff]), "+/8=");
    }

    #[test]
    fn the_viewer_runs_give_the_records_econ_sim_gives() {
        let config = small_region(18);
        let mut with = config.clone();
        with.scheduled_events.push(crate::options::parse_event("drought@4").unwrap());
        let pairs = econ_core::scenarios::compare(&config, &with.scheduled_events, 1).unwrap();
        assert_eq!(watch(config).unwrap().records, pairs.baseline[0].records);
        assert_eq!(watch(with).unwrap().records, pairs.scenario[0].records);
    }

    #[test]
    fn every_place_has_every_measure_every_month_and_they_add_up_to_the_records() {
        let arm = watch(small_region(15)).unwrap();
        let config = econ_core::crafts::with_crafts(&small_region(15));
        let measures = measures(&config);
        let n = config.villages.len();
        assert_eq!(arm.values.len(), measures.len());
        for (m, months) in measures.iter().zip(&arm.values) {
            assert_eq!(months.len(), 15, "{}", m.key);
            assert!(months.iter().all(|month| month.len() == n && month.iter().all(|x| x.is_finite())), "{}", m.key);
        }
        let total = |key: &str, t: usize| -> f64 {
            let k = measures.iter().position(|m| m.key == key).unwrap();
            arm.values[k][t].iter().sum()
        };
        let mean = |key: &str, t: usize| -> f64 {
            let k = measures.iter().position(|m| m.key == key).unwrap();
            arm.values[k][t].iter().zip(&arm.people[t]).map(|(x, p)| x * p).sum::<f64>()
        };
        for (t, record) in arm.records.iter().enumerate() {
            assert_eq!(total("population", t), record.population as f64);
            assert_eq!(arm.people[t].iter().sum::<f64>(), record.population as f64);
            assert_eq!(total("deaths", t), record.deaths as f64);
            assert_eq!(total("births", t), record.births as f64);
            assert_eq!(total("left", t), record.emigrants as f64);
            assert_eq!(total("arrived", t), record.immigrants as f64);
            assert_eq!(arm.families[t].iter().sum::<f64>(), record.households as f64);
            for (trade, workers) in &record.jobs {
                if measures.iter().any(|m| m.key == format!("jobs_{trade}")) {
                    assert_eq!(total(&format!("jobs_{trade}"), t), *workers as f64, "{trade}");
                }
            }
            assert!((mean("coins", t) - record.savings).abs() < 1e-6 * record.savings.max(1.0));
            let debt = (0..n).map(|v| measures_value(&arm, &measures, "debt", t, v) * arm.families[t][v]).sum::<f64>();
            assert!((debt - record.debt).abs() < 1e-6 * record.debt.max(1.0), "{debt} {}", record.debt);
            let landless = mean("landless", t);
            assert!((landless - record.landless as f64).abs() < 1.0, "{landless} {}", record.landless);
            // (the region's prices are a plain mean over places)
            let tools = measures.iter().position(|m| m.key == "price_tools").unwrap();
            let plain = arm.values[tools][t].iter().sum::<f64>() / n as f64;
            let recorded = record.prices.iter().find(|(p, _)| p == "tools").unwrap().1;
            assert!((plain - recorded).abs() < 1e-9, "{plain} {recorded}");
        }
    }

    fn measures_value(arm: &Watched, measures: &[Measure], key: &str, t: usize, v: usize) -> f64 {
        arm.values[measures.iter().position(|m| m.key == key).unwrap()][t][v]
    }

    #[test]
    fn a_big_region_is_shown_as_its_towns_with_their_countryside() {
        let config = standard_region(Standard::HundredThousand);
        let geo = geography::build(&config).unwrap();
        let shown = Shown::by_town(&geo.market);
        let towns = config.villages.iter().filter(|v| v.kind == "town").count();
        assert_eq!(shown.len(), towns);
        assert!(shown.head.iter().all(|&v| config.villages[v].kind == "town"));
        // a count adds up, a mean is weighed by people
        let people: Vec<f64> = config.villages.iter().map(|v| v.population as f64).collect();
        let summed = shown.gather(&people, None);
        assert_eq!(summed.iter().sum::<f64>(), people.iter().sum::<f64>());
        let ones = shown.gather(&vec![1.0; people.len()], Some(&people));
        assert!(ones.iter().all(|&x| (x - 1.0).abs() < 1e-12));
    }

    #[test]
    fn the_page_carries_its_data_and_nothing_that_ends_its_script_early() {
        let config = small_region(6);
        let forced = vec![crate::options::parse_event("drought@4").unwrap()];
        let mut with = config.clone();
        with.scheduled_events.extend_from_slice(&forced);
        let arms = vec![watch(config.clone()).unwrap(), watch(with).unwrap()];
        let shown = Shown::every_place(config.villages.len());
        let data = page_data(&config, &forced, &arms, &shown, json!({"note": "</script><b>"})).unwrap();
        assert_eq!(data["places"]["name"].as_array().unwrap().len(), config.villages.len());
        assert_eq!(data["records"][1]["population"].as_array().unwrap().len(), 6);
        assert_eq!(data["arms"][1]["label"], "With the drought");
        let html = page_html(&Page { title: "A <drought>".into(), data });
        assert!(html.contains("<title>A &lt;drought&gt;</title>"));
        assert_eq!(html.matches("</script>").count(), PAGE.matches("</script>").count());
        assert!(!html.contains("{{"), "every part of the template is filled in");
    }
}
