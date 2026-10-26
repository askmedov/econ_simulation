//! Reading the golden fixtures exported from Python (`scripts/export_fixtures.py`).

#![allow(dead_code)]

use std::path::PathBuf;

use econ_core::config::Config;
use econ_core::council::Councils;
use econ_core::geography::{Geography, Routes};
use econ_core::grid::{Grid, Grid3};
use econ_core::households::Households;
use econ_core::lords::Lords;
use econ_core::migration::Memory;
use econ_core::population::Population;
use econ_core::town::Town;
use econ_core::towns::Carts;
use serde_json::{Map, Value};

pub fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("fixtures")
}

/// The states fixtures are taken from.
pub const STATES: [&str; 5] = ["village_harvest", "village_famine", "hamlet", "city_june", "city_later"];

pub struct Fixture {
    pub name: String,
    pub config: Config,
    pub state: Value,
    pub cases: Map<String, Value>,
}

pub fn load(name: &str) -> Fixture {
    let path = fixtures_dir().join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut data: Value = serde_json::from_str(&text).expect("fixture JSON");
    let config: Config = serde_json::from_value(data["config"].take()).expect("fixture config");
    let state = data["state"].take();
    let cases = data["cases"].as_object().cloned().unwrap_or_default();
    Fixture { name: name.to_string(), config, state, cases }
}

impl Fixture {
    /// The cases of one function (empty if it has none here).
    pub fn of(&self, function: &str) -> Vec<(Value, Value)> {
        match self.cases.get(function) {
            Some(Value::Array(cases)) => cases.iter().map(|c| (c["args"].clone(), c["out"].clone())).collect(),
            _ => Vec::new(),
        }
    }

    pub fn population(&self, v: &Value) -> Population {
        if v.as_str() == Some("$population") {
            population(&self.state["population"])
        } else {
            population(v)
        }
    }

    pub fn households(&self, v: &Value) -> Households {
        if v.as_str() == Some("$households") {
            households(&self.state["households"])
        } else {
            households(v)
        }
    }
}

pub fn num(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap(),
        Value::Bool(b) => *b as i64 as f64,
        Value::String(s) => match s.as_str() {
            "inf" => f64::INFINITY,
            "-inf" => f64::NEG_INFINITY,
            "nan" => f64::NAN,
            other => panic!("not a number: {other}"),
        },
        other => panic!("not a number: {other}"),
    }
}

pub fn vecf(v: &Value) -> Vec<f64> {
    match v {
        Value::Array(items) => items.iter().map(num).collect(),
        other => vec![num(other)],
    }
}

pub fn vecb(v: &Value) -> Vec<bool> {
    v.as_array().expect("a list").iter().map(|x| x.as_bool().unwrap_or_else(|| num(x) != 0.0)).collect()
}

pub fn vecu(v: &Value) -> Vec<u32> {
    vecf(v).iter().map(|&x| x as u32).collect()
}

pub fn veci(v: &Value) -> Vec<i64> {
    vecf(v).iter().map(|&x| x as i64).collect()
}

pub fn grid(v: &Value) -> Grid {
    let rows: Vec<Vec<f64>> = v.as_array().expect("rows").iter().map(vecf).collect();
    let cols = rows.first().map_or(0, |r| r.len());
    Grid::from_rows(&rows, cols)
}

pub fn grid3(v: &Value) -> Grid3 {
    let rows = v.as_array().expect("rows");
    let r = rows.len();
    let c = rows.first().and_then(|x| x.as_array()).map_or(0, |x| x.len());
    let d = rows
        .first()
        .and_then(|x| x.as_array())
        .and_then(|x| x.first())
        .and_then(|x| x.as_array())
        .map_or(0, |x| x.len());
    let mut g = Grid3::zeros(r, c, d);
    for (i, row) in rows.iter().enumerate() {
        for (j, cell) in row.as_array().unwrap().iter().enumerate() {
            for (k, x) in cell.as_array().unwrap().iter().enumerate() {
                *g.at(i, j, k) = num(x);
            }
        }
    }
    g
}

pub fn population(v: &Value) -> Population {
    Population {
        count: vecu(&v["count"]),
        age_months: vecf(&v["age_months"]).iter().map(|&x| x as i32).collect(),
        female: vecb(&v["female"]),
        health: vecf(&v["health"]),
        skill: vecf(&v["skill"]),
        location: vecu(&v["location"]),
        household: vecu(&v["household"]),
        job: vecf(&v["job"]).iter().map(|&x| x as i16).collect(),
        married: vecb(&v["married"]),
        couple: vecf(&v["couple"]).iter().map(|&x| x as i32).collect(),
    }
}

pub fn households(v: &Value) -> Households {
    Households {
        location: vecu(&v["location"]),
        money: vecf(&v["money"]),
        land: vecf(&v["land"]),
        grain: vecf(&v["grain"]),
        animals: vecf(&v["animals"]),
        debt: vecf(&v["debt"]),
        lent: vecf(&v["lent"]),
        kin: vecf(&v["kin"]).iter().map(|&x| x as i32).collect(),
        minds: None, // (Python's families follow the rules)
    }
}

pub fn lords(v: &Value) -> Lords {
    Lords {
        land: vecf(&v["land"]),
        barn: vecf(&v["barn"]),
        purse: vecf(&v["purse"]),
        state_purse: vecf(&v["state_purse"]),
        garrison_pay: None, // (Python's state pays no garrison)
    }
}

pub fn councils(v: &Value) -> Councils {
    Councils {
        formed: vecb(&v["formed"]),
        months_ready: veci(&v["months_ready"]),
        treasury: vecf(&v["treasury"]),
        reserve: vecf(&v["reserve"]),
    }
}

pub fn memory(v: &Value) -> Memory {
    Memory {
        hunger: vecf(&v["hunger"]),
        danger: vecf(&v["danger"]),
        leaving: vecf(&v["leaving"]),
        danger_now: vecf(&v["danger_now"]),
    }
}

pub fn town(v: &Value) -> Town {
    Town {
        level: num(&v["level"]),
        swing: num(&v["swing"]),
        purse: num(&v["purse"]),
        coins_lost: num(&v["coins_lost"]),
    }
}

pub fn carts(v: &Value, products: usize) -> Carts {
    let goods = v["goods"].as_array().unwrap();
    let rows: Vec<Vec<f64>> = goods.iter().map(vecf).collect();
    Carts {
        grain_in: vecf(&v["grain_in"]),
        grain_held: vecf(&v["grain_held"]),
        grain_out: vecf(&v["grain_out"]),
        src: vecf(&v["src"]).iter().map(|&x| x as usize).collect(),
        dst: vecf(&v["dst"]).iter().map(|&x| x as usize).collect(),
        goods: Grid::from_rows(&rows, products),
    }
}

pub fn geography(v: &Value) -> Geography {
    let r = &v["routes"];
    Geography {
        x: vecf(&v["x"]),
        y: vecf(&v["y"]),
        terrain: vecf(&v["terrain"]).iter().map(|&x| x as usize).collect(),
        transport: vecf(&v["transport"]),
        market: veci(&v["market"]),
        pasture: vecf(&v["pasture"]),
        woods: vecf(&v["woods"]),
        forage: vecf(&v["forage"]),
        on_map: v["on_map"].as_bool().unwrap(),
        town: vecb(&v["town"]),
        border: vecb(&v["border"]),
        routes: Routes {
            a: vecf(&r["a"]).iter().map(|&x| x as usize).collect(),
            b: vecf(&r["b"]).iter().map(|&x| x as usize).collect(),
            cost: vecf(&r["cost"]),
            water: vecf(&r["water"]),
        },
        arable: vecf(&v["arable"]),
        to_border: vecf(&v["to_border"]),
    }
}

/// Equal to a relative 1e-9 (or within 1e-12 of 0), infinities alike.
pub fn same(a: f64, b: f64) -> bool {
    if a.is_infinite() || b.is_infinite() {
        return a == b;
    }
    if a.is_nan() || b.is_nan() {
        return a.is_nan() && b.is_nan();
    }
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()) + 1e-12
}

#[track_caller]
pub fn check(rust: &[f64], python: &Value, what: &str) {
    let python = vecf(python);
    assert_eq!(rust.len(), python.len(), "{what}: lengths differ");
    for (i, (&a, &b)) in rust.iter().zip(&python).enumerate() {
        assert!(same(a, b), "{what}[{i}]: Rust {a} against Python {b}");
    }
}

#[track_caller]
pub fn check_one(rust: f64, python: &Value, what: &str) {
    let b = num(python);
    assert!(same(rust, b), "{what}: Rust {rust} against Python {b}");
}

#[track_caller]
pub fn check_grid(rust: &Grid, python: &Value, what: &str) {
    let p = grid(python);
    assert_eq!((rust.rows, rust.cols), (p.rows, p.cols), "{what}: shapes differ");
    check(&rust.data, &Value::from(p.data.iter().map(|&x| Value::from(x)).collect::<Vec<_>>()), what);
}

#[track_caller]
pub fn check_bools(rust: &[bool], python: &Value, what: &str) {
    assert_eq!(rust, vecb(python).as_slice(), "{what}");
}

#[track_caller]
pub fn check_households(rust: &Households, python: &Value, what: &str) {
    check(&rust.money, &python["money"], &format!("{what}.money"));
    check(&rust.land, &python["land"], &format!("{what}.land"));
    check(&rust.grain, &python["grain"], &format!("{what}.grain"));
    check(&rust.animals, &python["animals"], &format!("{what}.animals"));
    check(&rust.debt, &python["debt"], &format!("{what}.debt"));
    check(&rust.lent, &python["lent"], &format!("{what}.lent"));
    assert_eq!(rust.kin, vecf(&python["kin"]).iter().map(|&x| x as i32).collect::<Vec<_>>(), "{what}.kin");
}

#[track_caller]
pub fn check_lords(rust: &Lords, python: &Value, what: &str) {
    check(&rust.barn, &python["barn"], &format!("{what}.barn"));
    check(&rust.purse, &python["purse"], &format!("{what}.purse"));
    check(&rust.state_purse, &python["state_purse"], &format!("{what}.state_purse"));
}

#[track_caller]
pub fn check_councils(rust: &Councils, python: &Value, what: &str) {
    check_bools(&rust.formed, &python["formed"], &format!("{what}.formed"));
    check(&rust.treasury, &python["treasury"], &format!("{what}.treasury"));
    check(&rust.reserve, &python["reserve"], &format!("{what}.reserve"));
    let months: Vec<f64> = rust.months_ready.iter().map(|&m| m as f64).collect();
    check(&months, &python["months_ready"], &format!("{what}.months_ready"));
}
