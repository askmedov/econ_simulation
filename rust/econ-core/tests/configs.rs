//! Every config exported from Python reads into the Rust parameters (an
//! unknown or missing field fails), and the default is Python's `Config()`.

use econ_core::config::{configs_dir, default_config, Config};
use serde_json::Value;

#[test]
fn the_default_config_is_the_default_village() {
    let config = default_config();
    assert_eq!(config.villages.len(), 1);
    assert_eq!(config.villages[0].population, 1000);
    assert_eq!(config.businesses[config.farming()].name, "farming");
    assert!(!config.map.enabled);
}

#[test]
fn every_exported_region_reads() {
    for entry in std::fs::read_dir(configs_dir().join("regions")).unwrap() {
        let path = entry.unwrap().path();
        let config = Config::from_file(&path).unwrap_or_else(|e| panic!("{e}"));
        assert!(config.map.enabled, "{}: a region is on a map", path.display());
    }
}

#[test]
fn every_setting_of_the_sanity_and_stress_suites_reads() {
    for suite in ["sanity.json", "stress.json"] {
        let text = std::fs::read_to_string(configs_dir().join(suite)).unwrap();
        let settings: Vec<Value> = serde_json::from_str(&text).unwrap();
        assert!(settings.len() > 20, "{suite}: {} settings", settings.len());
        for setting in settings {
            let _: Config =
                serde_json::from_value(setting["config"].clone()).unwrap_or_else(|e| panic!("{suite}: {e}"));
        }
    }
}

#[test]
fn a_parameter_rust_does_not_know_fails_loudly() {
    let mut data: Value = serde_json::from_str(econ_core::config::DEFAULT_CONFIG_JSON).unwrap();
    data["food"]["a_new_parameter"] = Value::from(1.0);
    assert!(Config::from_json(&data.to_string()).is_err());
    let mut data: Value = serde_json::from_str(econ_core::config::DEFAULT_CONFIG_JSON).unwrap();
    data["food"].as_object_mut().unwrap().remove("spoilage");
    assert!(Config::from_json(&data.to_string()).is_err());
}
