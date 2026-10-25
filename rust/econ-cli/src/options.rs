//! The options of a run, as in Python's command line (`python -m econ_sim`),
//! turned into a config.

use clap::Args;
use econ_core::config::{default_config, Config, ScheduledEvent};
use econ_core::region::{region, RegionSettings};

#[derive(Args, Clone, Debug, Default)]
pub struct RunArgs {
    /// A config exported from Python (scripts/export_config.py), such as a
    /// region; default: the default village
    #[arg(long, value_name = "FILE")]
    pub config: Option<String>,
    /// How many months to simulate (default: the config's, 36)
    #[arg(long)]
    pub months: Option<i64>,
    /// Repeat with this many seeds and average
    #[arg(long, default_value_t = 1)]
    pub runs: usize,
    /// Random seed of the first run (default: the config's, 42)
    #[arg(long)]
    pub seed: Option<i64>,
    /// Calendar month to start in, 1-12
    #[arg(long)]
    pub start_month: Option<i64>,
    /// Force an event, e.g. drought@4; in a region, drought@4@20,10,8 strikes
    /// the places within 8 km of (20, 10) km on the map; repeatable
    #[arg(long = "event", value_name = "NAME@MONTH[@X,Y,KM]")]
    pub events: Vec<String>,
    /// Only the forced events happen
    #[arg(long)]
    pub no_random_events: bool,
    /// The village never has a council (no taxes, reserve or relief)
    #[arg(long)]
    pub no_council: bool,
    /// The council gives no famine relief
    #[arg(long)]
    pub no_relief: bool,
    /// The council employs no healers
    #[arg(long)]
    pub no_healthcare: bool,
    /// No council at the start; one forms once the village has had 500 people for 6 months
    #[arg(long)]
    pub council_forms_later: bool,
    /// No lord: no demesne, labour services or barn
    #[arg(long)]
    pub no_lord: bool,
    /// The lord opens his barn to the hungry in a famine
    #[arg(long)]
    pub lord_charity: bool,
    /// The state takes no tax
    #[arg(long)]
    pub no_state_tax: bool,
    /// The state remits its tax in a famine year
    #[arg(long)]
    pub remit_tax: bool,
    /// No merchants: grain neither leaves for nor comes from the town
    #[arg(long)]
    pub no_town: bool,
    /// Nobody leaves the village or comes to it
    #[arg(long)]
    pub no_migration: bool,
    /// Kin don't give each other grain
    #[arg(long)]
    pub no_kin_help: bool,
    /// A drought (or good year) doesn't make another the next year likelier
    #[arg(long)]
    pub no_drought_runs: bool,
    /// Families have minds of their own: traits, memory and expectations
    /// (Rust only; see MindsConfig), instead of all following the rules
    #[arg(long)]
    pub minds: bool,
    /// Villagers at the start; in a region, people in an average village of the plain
    #[arg(long)]
    pub population: Option<i64>,
    /// Farmland in plots (one village, not a region)
    #[arg(long)]
    pub land: Option<f64>,
    /// A region of this many villages on a map, with hills, woods, a river,
    /// market towns and roads (laid out by the Rust engine: the same kind of
    /// region as Python's, not the same map; for Python's map use --config)
    #[arg(long, value_name = "VILLAGES")]
    pub region: Option<usize>,
    /// Market towns in the region (default: one for every 25 villages)
    #[arg(long)]
    pub towns: Option<usize>,
    /// Lay out a different region
    #[arg(long, default_value_t = 1)]
    pub map_seed: i64,
    /// Months of food in store at the start
    #[arg(long)]
    pub food_months: Option<f64>,
    /// Threads to run the runs on (default: all cores)
    #[arg(long)]
    pub workers: Option<usize>,
    /// Folder for the CSV files
    #[arg(long, default_value = "output")]
    pub out: String,
    /// Show the events that can happen, then exit
    #[arg(long)]
    pub list_events: bool,
}

impl RunArgs {
    /// The config these options describe (the forced events not included).
    pub fn config(&self) -> Result<Config, String> {
        let mut config = match &self.config {
            Some(path) => Config::from_file(path).map_err(|e| e.to_string())?,
            None => default_config(),
        };
        if let Some(m) = self.months {
            config.months = m;
        }
        if let Some(s) = self.seed {
            config.seed = s;
        }
        if let Some(s) = self.start_month {
            config.start_month = s;
        }
        if let Some(villages) = self.region {
            if self.land.is_some() {
                return Err("--land sets one village's land; in a region each village has land for its size".into());
            }
            let settings = RegionSettings {
                villages,
                towns: self.towns,
                seed: self.map_seed,
                mean_size: self.population.map_or(RegionSettings::default().mean_size, |p| p as f64),
                ..RegionSettings::default()
            };
            // Villages get land at the usual density for their size.
            let mut usual = default_config().villages[0].clone();
            if let Some(f) = self.food_months {
                usual.initial_food_months = f;
            }
            config.villages = vec![usual];
            config = region(&settings, &config)?;
        } else if self.population.is_some() || self.land.is_some() {
            if config.villages.len() != 1 {
                return Err(
                    "--population and --land set one village's size; in a region each village has its own".into()
                );
            }
            let village = &mut config.villages[0];
            if let Some(p) = self.population {
                village.population = p;
            }
            if let Some(l) = self.land {
                village.land = l;
            }
        }
        if let Some(f) = self.food_months {
            for village in config.villages.iter_mut().filter(|v| v.kind == "village") {
                village.initial_food_months = f;
            }
        }
        if self.no_random_events {
            config.random_events = false;
        }
        if self.no_council {
            config.council.enabled = false;
        }
        if self.no_relief {
            config.council.relief = false;
        }
        if self.council_forms_later {
            config.council.established_at_start = false;
        }
        if self.no_healthcare {
            config.healthcare.enabled = false;
        }
        if self.no_lord {
            config.lord.enabled = false;
        }
        if self.lord_charity {
            config.lord.charity_in_famine = true;
        }
        if self.no_state_tax {
            config.state.enabled = false;
        }
        if self.remit_tax {
            config.state.remit_in_famine = true;
        }
        if self.no_town {
            config.town.enabled = false;
        }
        if self.no_migration {
            config.migration.enabled = false;
        }
        if self.no_kin_help {
            config.work.kin_share = 0.0;
        }
        if self.minds {
            config.minds.enabled = true;
        }
        if self.no_drought_runs {
            for event in config.events.iter_mut() {
                event.repeat_chance = None;
            }
        }
        Ok(config)
    }

    /// The forced events.
    pub fn forced(&self) -> Result<Vec<ScheduledEvent>, String> {
        self.events.iter().map(|text| parse_event(text)).collect()
    }
}

/// NAME@MONTH or NAME@MONTH@X,Y,KM.
pub fn parse_event(text: &str) -> Result<ScheduledEvent, String> {
    let expected = || format!("--event {text:?}: expected NAME@MONTH, e.g. drought@4, or NAME@MONTH@X,Y,KM");
    let mut parts = text.splitn(3, '@');
    let name = parts.next().unwrap_or("").trim().replace(' ', "_");
    let month = parts.next().ok_or_else(expected)?;
    if month.is_empty() || !month.chars().all(|c| c.is_ascii_digit()) {
        return Err(expected());
    }
    let near = match parts.next() {
        Some(place) => {
            let values: Vec<f64> = place
                .split(',')
                .map(|p| p.trim().parse::<f64>())
                .collect::<Result<_, _>>()
                .map_err(|_| format!("--event {text:?}: expected a place as X,Y,KM, e.g. drought@4@20,10,8"))?;
            if values.len() != 3 {
                return Err(format!("--event {text:?}: expected a place as X,Y,KM, e.g. drought@4@20,10,8"));
            }
            Some((values[0], values[1], values[2]))
        }
        None => None,
    };
    Ok(ScheduledEvent { event: name, month: month.parse().map_err(|_| expected())?, village: None, near })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_event_is_read_with_its_month_and_place() {
        let e = parse_event("drought@4").unwrap();
        assert_eq!((e.event.as_str(), e.month, e.near), ("drought", 4, None));
        let e = parse_event("drought@4@20,10,8").unwrap();
        assert_eq!(e.near, Some((20.0, 10.0, 8.0)));
        assert!(parse_event("drought").is_err());
        assert!(parse_event("drought@x").is_err());
        assert!(parse_event("drought@4@1,2").is_err());
    }

    #[test]
    fn options_change_only_what_they_name() {
        let args = RunArgs { no_council: true, months: Some(12), ..Default::default() };
        let config = args.config().unwrap();
        let default = default_config();
        assert!(!config.council.enabled);
        assert_eq!(config.months, 12);
        assert_eq!(config.lord, default.lord);
    }
}
