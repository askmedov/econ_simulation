//! Starting points: the state a region starts from, as changes to its
//! settings (Python's `econ_sim/starts.py`, exported to
//! rust/configs/starts.json).
//!
//! A change sets a parameter to a value or scales it by a factor. A
//! parameter is named by its path in the config ("lord.demesne"). A path
//! through a list changes the item with that name ("events.plague.chance",
//! "villages.City.population"), or else every item ("villages.population":
//! every place). Scaling keeps whole numbers whole, and a zero a zero.

use serde::Deserialize;
use serde_json::Value;

use crate::config::Config;

pub const STARTS_JSON: &str = include_str!("../../configs/starts.json");

/// A starting point: its name, what it is, and its changes.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Start {
    pub name: String,
    pub about: String,
    pub set: Vec<(String, Value)>,
    pub scale: Vec<(String, f64)>,
}

/// Every starting point, in Python's order.
pub fn starts() -> Vec<Start> {
    serde_json::from_str(STARTS_JSON).expect("rust/configs/starts.json matches the starting points")
}

pub fn find(name: &str) -> Result<Start, String> {
    starts()
        .into_iter()
        .find(|s| s.name == name)
        .ok_or_else(|| format!("no starting point named {name:?} (econ-sim starts --list shows them)"))
}

/// `config` from the starting point `start`.
pub fn apply(config: &Config, start: &Start) -> Result<Config, String> {
    change(config, &start.set, &start.scale).map_err(|e| format!("starting point {}: {e}", start.name))
}

/// `config` with each parameter in `set` set to its value and each in
/// `scale` multiplied by its factor.
pub fn change(config: &Config, set: &[(String, Value)], scale: &[(String, f64)]) -> Result<Config, String> {
    let mut data = serde_json::to_value(config).map_err(|e| e.to_string())?;
    for (path, value) in set {
        each(&mut data, &steps(path), path, &mut |v| {
            *v = value.clone();
            Ok(())
        })?;
    }
    for (path, factor) in scale {
        each(&mut data, &steps(path), path, &mut |v| scaled(v, *factor, path))?;
    }
    Config::from_json(&data.to_string()).map_err(|e| e.0)
}

/// A change written on the command line, `PATH=VALUE`: the value as JSON
/// (a number, true or false, a list), or else as text.
pub fn parse_set(text: &str) -> Result<(String, Value), String> {
    let (path, value) = text.split_once('=').ok_or_else(|| format!("{text:?}: write PATH=VALUE"))?;
    let value = serde_json::from_str(value).unwrap_or_else(|_| Value::String(value.to_string()));
    Ok((path.to_string(), value))
}

/// `PATH=FACTOR`.
pub fn parse_scale(text: &str) -> Result<(String, f64), String> {
    let (path, factor) = text.split_once('=').ok_or_else(|| format!("{text:?}: write PATH=FACTOR"))?;
    let factor: f64 = factor.parse().map_err(|_| format!("{text:?}: the factor must be a number"))?;
    Ok((path.to_string(), factor))
}

fn steps(path: &str) -> Vec<&str> {
    path.split('.').collect()
}

/// Apply `f` to every value the path leads to; an error if it leads nowhere.
fn each(
    value: &mut Value,
    steps: &[&str],
    path: &str,
    f: &mut dyn FnMut(&mut Value) -> Result<(), String>,
) -> Result<(), String> {
    let Some((&step, rest)) = steps.split_first() else {
        return f(value);
    };
    match value {
        Value::Object(fields) => match fields.get_mut(step) {
            Some(inner) => each(inner, rest, path, f),
            None => Err(format!("no parameter {path:?} ({step:?} is not among {})", names(fields))),
        },
        Value::Array(items) => {
            let named = |item: &Value| item.get("name").and_then(Value::as_str) == Some(step);
            if items.iter().any(named) {
                items.iter_mut().filter(|item| named(item)).try_for_each(|item| each(item, rest, path, f))
            } else if items.is_empty() {
                Err(format!("no parameter {path:?}: the list is empty"))
            } else {
                items.iter_mut().try_for_each(|item| each(item, steps, path, f))
            }
        }
        _ => Err(format!("no parameter {path:?}: {step:?} goes past a value")),
    }
}

fn names(fields: &serde_json::Map<String, Value>) -> String {
    fields.keys().map(String::as_str).collect::<Vec<_>>().join(", ")
}

fn scaled(value: &mut Value, factor: f64, path: &str) -> Result<(), String> {
    match value {
        Value::Number(x) => {
            let whole = x.is_i64() || x.is_u64();
            let y = x.as_f64().unwrap_or(0.0) * factor;
            *value = if whole { Value::from(y.round() as i64) } else { Value::from(y) };
            Ok(())
        }
        Value::Array(items) => items.iter_mut().try_for_each(|item| scaled(item, factor, path)),
        _ => Err(format!("{path:?} is not a number, so it can't be scaled")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{default_config, standard_region, Standard};

    #[test]
    fn a_path_through_a_list_changes_every_item_or_the_one_named() {
        let region = standard_region(Standard::HundredThousand);
        let half = change(&region, &[], &[("villages.population".into(), 0.5)]).unwrap();
        for (a, b) in region.villages.iter().zip(&half.villages) {
            assert_eq!(b.population, (a.population as f64 * 0.5).round() as i64);
        }
        let city = change(&region, &[("villages.City.population".into(), Value::from(5000))], &[]).unwrap();
        let changed: Vec<_> = region.villages.iter().zip(&city.villages).filter(|(a, b)| a != b).collect();
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].1.population, 5000);
        let plague = change(&region, &[("events.plague.chance".into(), Value::from(0.0))], &[]).unwrap();
        assert_eq!(plague.events.iter().find(|e| e.name == "plague").unwrap().chance, 0.0);
    }

    #[test]
    fn scaling_keeps_whole_numbers_whole_and_a_towns_empty_store_empty() {
        let region = standard_region(Standard::HundredThousand);
        let lean = change(&region, &[], &[("villages.initial_food_months".into(), 0.2)]).unwrap();
        for (a, b) in region.villages.iter().zip(&lean.villages) {
            assert!((b.initial_food_months - 0.2 * a.initial_food_months).abs() < 1e-12);
        }
        let weak = change(&region, &[], &[("health.initial".into(), 0.5)]).unwrap();
        assert_eq!(weak.health.initial, (40.0, 50.0));
    }

    #[test]
    fn a_change_to_a_parameter_that_does_not_exist_is_refused() {
        let c = default_config();
        assert!(change(&c, &[("lord.demesnee".into(), Value::from(0.3))], &[]).is_err());
        assert!(change(&c, &[("lord.demesne.share".into(), Value::from(0.3))], &[]).is_err());
        assert!(change(&c, &[("lord.demesne".into(), Value::from("much"))], &[]).is_err());
        assert!(change(&c, &[], &[("lord.enabled".into(), 2.0)]).is_err());
    }

    #[test]
    fn every_starting_point_applies_to_the_standard_regions_and_the_village() {
        let all = starts();
        assert!(all.len() >= 10 && all[0].name == "standard");
        for config in [standard_region(Standard::Million), standard_region(Standard::HundredThousand), default_config()]
        {
            for start in &all {
                apply(&config, start).unwrap_or_else(|e| panic!("{e}"));
            }
        }
        assert_eq!(apply(&default_config(), &all[0]).unwrap(), default_config());
    }

    #[test]
    fn a_command_line_change_reads_numbers_switches_lists_and_text() {
        assert_eq!(parse_set("lord.demesne=0.4").unwrap(), ("lord.demesne".into(), Value::from(0.4)));
        assert_eq!(parse_set("lord.enabled=false").unwrap().1, Value::Bool(false));
        assert_eq!(parse_set("health.initial=[50,90]").unwrap().1, serde_json::json!([50, 90]));
        assert_eq!(parse_set("villages.City.kind=town").unwrap().1, Value::from("town"));
        assert!(parse_set("lord.demesne").is_err());
        assert_eq!(parse_scale("villages.population=0.8").unwrap().1, 0.8);
        assert!(parse_scale("villages.population=lots").is_err());
    }
}
