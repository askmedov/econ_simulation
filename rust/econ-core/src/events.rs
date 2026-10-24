//! Random events: starting, ending and combining their effects.
//!
//! Event kinds are data (see `EventSpec` in the config). This module only
//! knows how to roll them and turn the active ones into multipliers for the
//! rules.

use crate::config::{Config, Duration, EffectValue, EventSpec, HealthConfig, ScheduledEvent};
use crate::geography::{self, Geography};
use crate::grid::Grid;
use crate::population::Population;
use crate::rng::Rng;

/// What an event does, every month it is active.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    ProductionMult,
    HealthDelta,
    MortalityMult,
    FertilityMult,
    GranaryLoss,
    HeatingMult,
    DebtCancel,
    Requisition,
    Debase,
    WoodsBurned,
    WoolPrice,
}

pub const EFFECTS: [(&str, Effect); 11] = [
    ("production_mult", Effect::ProductionMult),
    ("health_delta", Effect::HealthDelta),
    ("mortality_mult", Effect::MortalityMult),
    ("fertility_mult", Effect::FertilityMult),
    ("granary_loss", Effect::GranaryLoss),
    ("heating_mult", Effect::HeatingMult),
    ("debt_cancel", Effect::DebtCancel),
    ("requisition", Effect::Requisition),
    ("debase", Effect::Debase),
    ("woods_burned", Effect::WoodsBurned),
    ("wool_price", Effect::WoolPrice),
];
/// Effects that only mean something on a map (wool is sold through its
/// border towns): without one, events with only these are never rolled.
pub const MAP_ONLY_EFFECTS: [&str; 1] = ["wool_price"];
pub const PERSON_EFFECTS: [&str; 1] = ["health_delta"];
pub const REACHES: [&str; 3] = ["village", "weather", "region"];

pub fn effect_named(name: &str) -> Option<Effect> {
    EFFECTS.iter().find(|(n, _)| *n == name).map(|&(_, e)| e)
}

/// An event under way in one place.
#[derive(Clone, Debug, PartialEq)]
pub struct ActiveEvent {
    pub spec: usize, // index into Config.events
    pub location: usize,
    pub months_left: i64,            // including the current month
    pub effects: Vec<(Effect, f64)>, // ranges already drawn
}

/// When each event last started in each place (events x places).
#[derive(Clone, Debug, PartialEq)]
pub struct LastStarted {
    months: Vec<Vec<i64>>,
}

/// "Never" (far enough back that no year-long memory reaches it).
const NEVER: i64 = -1_000_000_000;

impl LastStarted {
    pub fn new(n_events: usize, n_places: usize) -> LastStarted {
        LastStarted { months: vec![vec![NEVER; n_places]; n_events] }
    }

    /// Whether event `spec` started in `place` in the 12 months up to `month`.
    pub fn within_a_year(&self, spec: usize, place: usize, month: i64) -> bool {
        month - self.months[spec][place] <= 12
    }

    pub fn set(&mut self, spec: usize, place: usize, month: i64) {
        self.months[spec][place] = month;
    }
}

/// Combined effect of all active events: one value per place, except
/// production_mult, which is per place and business.
#[derive(Clone, Debug)]
pub struct Modifiers {
    pub production_mult: Grid,
    pub health_delta: Vec<f64>,
    pub mortality_mult: Vec<f64>,
    pub fertility_mult: Vec<f64>,
    pub granary_loss: Vec<f64>,
    pub heating_mult: Vec<f64>,
    pub debt_cancel: Vec<f64>,
    pub requisition: Vec<f64>,
    pub debase: Vec<f64>,
    pub woods_burned: Vec<f64>,
    pub wool_price: Vec<f64>,
}

impl Modifiers {
    pub fn neutral(n: usize, n_businesses: usize) -> Modifiers {
        Modifiers {
            production_mult: Grid::new(n, n_businesses, 1.0),
            health_delta: vec![0.0; n],
            mortality_mult: vec![1.0; n],
            fertility_mult: vec![1.0; n],
            granary_loss: vec![0.0; n],
            heating_mult: vec![1.0; n],
            debt_cancel: vec![0.0; n],
            requisition: vec![0.0; n],
            debase: vec![0.0; n],
            woods_burned: vec![0.0; n],
            wool_price: vec![1.0; n],
        }
    }
}

pub fn validate(specs: &[EventSpec]) -> Result<(), String> {
    let mut group_chance: Vec<(String, f64)> = Vec::new();
    for spec in specs {
        if spec.scope != "location" && spec.scope != "person" {
            return Err(format!("{}: unknown scope {:?}", spec.name, spec.scope));
        }
        for effect in spec.effects.keys() {
            let allowed = if spec.scope == "location" {
                effect_named(effect).is_some()
            } else {
                PERSON_EFFECTS.contains(&effect.as_str())
            };
            if !allowed {
                return Err(format!("{}: effect {:?} not allowed for {} events", spec.name, effect, spec.scope));
            }
        }
        if !(0.0..=1.0).contains(&spec.chance) {
            return Err(format!("{}: chance must be between 0 and 1", spec.name));
        }
        if let Some(r) = spec.repeat_chance {
            if !((0.0..=1.0).contains(&r) && spec.chance < 1.0) {
                return Err(format!("{}: repeat_chance must be between 0 and 1, with chance below 1", spec.name));
            }
        }
        if spec.scope == "person" && spec.duration != Duration::Fixed(1) {
            return Err(format!("{}: person events last one month", spec.name));
        }
        if !REACHES.contains(&spec.reach.as_str()) || (spec.end != "bad" && spec.end != "good") {
            return Err(format!("{}: reach must be one of {}, and end 'bad' or 'good'", spec.name, REACHES.join(", ")));
        }
        if let Some(group) = &spec.group {
            match group_chance.iter_mut().find(|(g, _)| g == group) {
                Some((_, total)) => *total += spec.chance,
                None => group_chance.push((group.clone(), spec.chance)),
            }
        }
    }
    for (group, total) in &group_chance {
        if *total > 1.0 {
            return Err(format!("event group {group:?}: chances add up to more than 1"));
        }
    }
    // After a year with one of a group's events, its repeat chance replaces its usual one.
    for spec in specs {
        if let (Some(group), Some(repeat)) = (&spec.group, spec.repeat_chance) {
            let total = group_chance.iter().find(|(g, _)| g == group).map_or(0.0, |&(_, t)| t);
            if total - spec.chance + repeat > 1.0 {
                return Err(format!("event group {group:?}: chances add up to more than 1 after {}", spec.name));
            }
        }
    }
    Ok(())
}

/// An event's chance this month: its `repeat_chance` if it started in the
/// last 12 months, otherwise a chance lowered so that it still starts in
/// `chance` of eligible months in the long run.
pub fn chance_now(spec: &EventSpec, repeated: bool) -> f64 {
    match spec.repeat_chance {
        None => spec.chance,
        Some(r) if repeated => r,
        // Long-run share p = q / (1 - r + q), for a chance q after a year without it.
        Some(r) => spec.chance * (1.0 - r) / (1.0 - spec.chance),
    }
}

pub fn validate_schedule(
    scheduled: &[ScheduledEvent],
    specs: &[EventSpec],
    n_locations: usize,
    on_map: bool,
) -> Result<(), String> {
    let mut location_events: Vec<&str> =
        specs.iter().filter(|s| s.scope == "location").map(|s| s.name.as_str()).collect();
    for item in scheduled {
        if !location_events.contains(&item.event.as_str()) {
            location_events.sort();
            return Err(format!("unknown village event {:?}; choose from: {}", item.event, location_events.join(", ")));
        }
        if item.month < 1 {
            return Err(format!("{}: month must be 1 or later", item.event));
        }
        if let Some(v) = item.village {
            if v >= n_locations {
                return Err(format!("{}: no village number {v}", item.event));
            }
        }
        if item.near.is_some() && (!on_map || item.village.is_some()) {
            return Err(format!("{}: 'near' (x, y, km) needs a map, and no village number", item.event));
        }
    }
    Ok(())
}

fn can_start(spec: &EventSpec, scope: &str, month_of_year: i64) -> bool {
    spec.scope == scope && spec.months.as_ref().is_none_or(|m| m.contains(&month_of_year))
}

/// Count down active events and drop those that have run their course.
pub fn advance(active: &mut Vec<ActiveEvent>) {
    for event in active.iter_mut() {
        event.months_left -= 1;
    }
    active.retain(|e| e.months_left > 0);
}

/// What `start_location_events` and the others need of the world.
pub struct EventState<'a> {
    pub active: &'a mut Vec<ActiveEvent>,
    pub last_started: &'a mut LastStarted,
    pub month: i64,
    pub month_of_year: i64,
    pub n_locations: usize,
}

/// The map the weather falls on and how far it reaches; its draws come
/// from their own stream.
pub struct Weather<'a> {
    pub geo: &'a Geography,
    pub scale_km: f64,
    pub rng: &'a mut Rng,
}

/// Groups of event kinds that may start this month, by group (or by name
/// for an event of no group), in the order they are listed.
fn candidates(config: &Config, month_of_year: i64, on_map: bool) -> Vec<(String, Vec<usize>)> {
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    for (i, spec) in config.events.iter().enumerate() {
        let map_only = spec.effects.keys().all(|e| MAP_ONLY_EFFECTS.contains(&e.as_str()));
        if can_start(spec, "location", month_of_year) && (on_map || !map_only) {
            let key = spec.group.clone().unwrap_or_else(|| format!("_{}", spec.name));
            match groups.iter_mut().find(|(k, _)| *k == key) {
                Some((_, members)) => members.push(i),
                None => groups.push((key, vec![i])),
            }
        }
    }
    groups
}

/// Roll place-wide events. Events in the same group exclude each other.
/// With `weather` (a map), weather events follow the season's weather over
/// the map and region events strike every place at once. Returns the
/// indices (into `state.active`) of the events that started.
pub fn start_location_events(
    state: &mut EventState,
    config: &Config,
    rng: &mut Rng,
    weather: Option<Weather>,
) -> Vec<usize> {
    let groups = candidates(config, state.month_of_year, weather.is_some());
    if let Some(weather) = weather {
        return start_on_map(state, config, &groups, rng, weather);
    }
    let mut started = Vec::new();
    for location in 0..state.n_locations {
        let here: Vec<usize> = state.active.iter().filter(|e| e.location == location).map(|e| e.spec).collect();
        for (key, members) in &groups {
            let roll = rng.random();
            let busy = here.iter().any(|&s| {
                let spec = &config.events[s];
                let name_hit = members.iter().any(|&m| config.events[m].name == spec.name) || spec.name == *key;
                let group_hit = spec
                    .group
                    .as_ref()
                    .is_some_and(|g| g == key || members.iter().any(|&m| config.events[m].name == *g));
                name_hit || group_hit
            });
            if busy {
                continue;
            }
            let mut threshold = 0.0;
            for &s in members {
                threshold += chance_now(&config.events[s], state.last_started.within_a_year(s, location, state.month));
                if roll < threshold {
                    started.push(start(state, config, s, location, rng));
                    break;
                }
            }
        }
    }
    started
}

/// Start this month's events on a map. Each place rolls for its own events
/// as without a map; weather events take the season's weather at each place
/// (bad ones where it is worst, good ones where it is best); region events
/// take one roll for all.
fn start_on_map(
    state: &mut EventState,
    config: &Config,
    groups: &[(String, Vec<usize>)],
    rng: &mut Rng,
    weather: Weather,
) -> Vec<usize> {
    let n = state.n_locations;
    let rolls: Vec<Vec<f64>> = (0..n).map(|_| (0..groups.len()).map(|_| rng.random()).collect()).collect();
    let mut started = Vec::new();
    for (column, (key, members)) in groups.iter().enumerate() {
        let mut free = vec![true; n];
        for event in state.active.iter() {
            let spec = &config.events[event.spec];
            if members.iter().any(|&m| config.events[m].name == spec.name) || spec.group.as_ref() == Some(key) {
                free[event.location] = false;
            }
        }
        for reach in REACHES {
            let group: Vec<usize> = members.iter().cloned().filter(|&m| config.events[m].reach == reach).collect();
            if group.is_empty() {
                continue;
            }
            let roll: Vec<f64> = match reach {
                "region" => vec![weather.rng.random(); n],
                "weather" => geography::weather(weather.geo, weather.scale_km, weather.rng),
                _ => (0..n).map(|i| rolls[i][column]).collect(),
            };
            let sides: &[Option<&str>] = if reach == "weather" { &[Some("bad"), Some("good")] } else { &[None] };
            for &side in sides {
                let at: Vec<f64> = roll.iter().map(|&r| if side == Some("good") { 1.0 - r } else { r }).collect();
                let mut threshold = vec![0.0; n];
                for &s in &group {
                    let spec = &config.events[s];
                    if side.is_some_and(|side| spec.end != side) {
                        continue;
                    }
                    let low = threshold.clone();
                    for (loc, t) in threshold.iter_mut().enumerate() {
                        *t += chance_now(spec, state.last_started.within_a_year(s, loc, state.month));
                    }
                    for loc in 0..n {
                        if free[loc] && at[loc] >= low[loc] && at[loc] < threshold[loc] {
                            started.push(start(state, config, s, loc, rng));
                            free[loc] = false;
                        }
                    }
                }
            }
        }
    }
    started
}

/// Start the events forced for this month (`month_number` counts from 1).
///
/// They start whatever their usual chance or season, unless the same event
/// is already under way in that place.
pub fn start_scheduled_events(
    state: &mut EventState,
    config: &Config,
    geo: &Geography,
    month_number: i64,
    rng: &mut Rng,
) -> Vec<usize> {
    let mut started = Vec::new();
    for item in &config.scheduled_events {
        if item.month != month_number {
            continue;
        }
        let s = config.events.iter().position(|e| e.name == item.event).expect("validated");
        let locations: Vec<usize> = if let Some(near) = item.near {
            geo.near(near).iter().enumerate().filter(|(_, &yes)| yes).map(|(i, _)| i).collect()
        } else if let Some(v) = item.village {
            vec![v]
        } else {
            (0..state.n_locations).collect()
        };
        for location in locations {
            let name = &config.events[s].name;
            if state.active.iter().any(|e| config.events[e.spec].name == *name && e.location == location) {
                continue;
            }
            started.push(start(state, config, s, location, rng));
        }
    }
    started
}

fn draw(value: EffectValue, rng: &mut Rng) -> f64 {
    match value {
        EffectValue::Fixed(v) => v,
        EffectValue::Range(low, high) => rng.uniform(low, high),
    }
}

fn start(state: &mut EventState, config: &Config, spec: usize, location: usize, rng: &mut Rng) -> usize {
    let s = &config.events[spec];
    let months_left = match s.duration {
        Duration::Fixed(d) => d,
        Duration::Range(low, high) => rng.integers(low, high + 1),
    };
    let effects = s.effects.iter().map(|(name, &v)| (effect_named(name).expect("validated"), draw(v, rng))).collect();
    state.active.push(ActiveEvent { spec, location, months_left, effects });
    state.last_started.set(spec, location, state.month);
    state.active.len() - 1
}

/// Hit individual people with personal events; returns how many each event hit.
pub fn apply_person_events(
    population: &mut Population,
    config: &Config,
    month_of_year: i64,
    health: &HealthConfig,
    rng: &mut Rng,
) -> Vec<(String, u64)> {
    let mut hits = Vec::new();
    for spec in &config.events {
        if !can_start(spec, "person", month_of_year) {
            continue;
        }
        let taken: Vec<u32> = population.count.iter().map(|&c| rng.binomial(c as u64, spec.chance) as u32).collect();
        let total: u64 = taken.iter().map(|&t| t as u64).sum();
        hits.push((spec.name.clone(), total));
        if total == 0 {
            continue;
        }
        let rows = population.split(&taken);
        if let Some(&value) = spec.effects.get("health_delta") {
            for &row in &rows {
                let delta = draw(value, rng);
                population.health[row] = (population.health[row] + delta).clamp(0.0, health.maximum);
            }
        }
    }
    hits
}

fn hits(spec: &EventSpec, business: &str) -> bool {
    spec.businesses.as_ref().is_none_or(|b| b.iter().any(|x| x == business))
}

/// Multiplier on one business's output in each of the next `months` months
/// from events under way.
pub fn production_outlook(active: &[ActiveEvent], config: &Config, n: usize, months: usize, business: &str) -> Grid {
    let mut outlook = Grid::new(n, months, 1.0);
    for event in active {
        if !hits(&config.events[event.spec], business) {
            continue;
        }
        for &(effect, mult) in &event.effects {
            if effect == Effect::ProductionMult {
                let ahead = (event.months_left - 1).clamp(0, months as i64) as usize;
                for m in 0..ahead {
                    *outlook.at(event.location, m) *= mult;
                }
            }
        }
    }
    outlook
}

/// Combine active events; `businesses` names the columns of production_mult.
pub fn modifiers(active: &[ActiveEvent], config: &Config, n: usize, businesses: &[String]) -> Modifiers {
    let mut mods = Modifiers::neutral(n, businesses.len());
    for event in active {
        let loc = event.location;
        let spec = &config.events[event.spec];
        for &(effect, value) in &event.effects {
            let share = |s: &mut f64| *s = 1.0 - (1.0 - *s) * (1.0 - value);
            match effect {
                Effect::ProductionMult => {
                    for (b, name) in businesses.iter().enumerate() {
                        if hits(spec, name) {
                            *mods.production_mult.at(loc, b) *= value;
                        }
                    }
                }
                Effect::HealthDelta => mods.health_delta[loc] += value,
                Effect::GranaryLoss => share(&mut mods.granary_loss[loc]),
                Effect::DebtCancel => share(&mut mods.debt_cancel[loc]),
                Effect::Requisition => share(&mut mods.requisition[loc]),
                Effect::Debase => share(&mut mods.debase[loc]),
                Effect::WoodsBurned => share(&mut mods.woods_burned[loc]),
                Effect::MortalityMult => mods.mortality_mult[loc] *= value,
                Effect::FertilityMult => mods.fertility_mult[loc] *= value,
                Effect::HeatingMult => mods.heating_mult[loc] *= value,
                Effect::WoolPrice => mods.wool_price[loc] *= value,
            }
        }
    }
    mods
}
