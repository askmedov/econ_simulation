//! The model's parameters, read from JSON exported by Python's `Config`
//! (`scripts/export_config.py`). Python stays the one source of numbers:
//! every field here mirrors a field of `econ_sim/config.py`, where each one
//! is explained, and an unknown field fails loudly, so a parameter added in
//! Python and not here can't be silently ignored.

use std::path::Path;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// A range (low, high) drawn uniformly when an event starts, or one value.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EffectValue {
    Fixed(f64),
    Range(f64, f64),
}

/// An event's length in months: fixed, or a range drawn when it starts.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Duration {
    Fixed(i64),
    Range(i64, i64),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VillageConfig {
    pub name: String,
    pub population: i64,
    pub land: f64,
    pub initial_food_months: f64,
    pub x: f64,
    pub y: f64,
    pub terrain: String,
    pub kind: String,
    pub border: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DemographyConfig {
    pub adult_age: i64,
    pub retirement_age: i64,
    pub fertile_ages: (i64, i64),
    pub annual_birth_chance: f64,
    pub fertility_by_age: Vec<(i64, f64)>,
    pub female_share_at_birth: f64,
    pub marriage_chance: f64,
    pub bride_ages: (i64, i64),
    pub groom_ages: (i64, i64),
    pub widow_ages: (i64, i64),
    pub crowded_marriage_factor: f64,
    pub wage_cover_for_marriage: (f64, f64),
    pub annual_mortality: Vec<(i64, f64)>,
    pub initial_age_decay: f64,
    pub max_initial_age: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FoodConfig {
    pub adult_need: f64,
    pub child_need: f64,
    pub labor_share: f64,
    pub seasonality: Vec<f64>,
    pub spoilage: f64,
    pub planning_months: i64,
    pub reserve_margin: f64,
    pub normal_cover: f64,
    pub price_elasticity: f64,
    pub price_adjustment: f64,
    pub output_noise: f64,
    pub seed_per_plot: f64,
    pub seed_months: Vec<i64>,
    pub max_seed_share: f64,
    pub foraging: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HealthConfig {
    pub initial: (f64, f64),
    pub maximum: f64,
    pub starvation_ration: f64,
    pub recovery: f64,
    pub hunger_damage: f64,
    pub danger_threshold: f64,
    pub max_extra_mortality: f64,
    pub work_at_zero_health: f64,
    pub fertility_health: (f64, f64),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillConfig {
    pub mean: f64,
    pub sd: f64,
    pub minimum: f64,
    pub maximum: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductSpec {
    pub name: String,
    #[serde(rename = "use")]
    pub use_: String,
    pub carriage: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BusinessSpec {
    pub name: String,
    pub product: String,
    pub output: f64,
    pub tool_boost: f64,
    pub inputs: Vec<(String, f64)>,
    pub uses_land: bool,
    pub initial_share: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NeedsConfig {
    pub firewood: Vec<f64>,
    pub cold_damage: f64,
    pub savings_months: f64,
    pub spare_spending: f64,
    pub gathering: f64,
    pub other_fuels: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TradeConfig {
    pub tool_wear: f64,
    pub buying_budget: f64,
    pub stock_target_months: f64,
    pub orders_memory: f64,
    pub markup_speed: f64,
    pub max_markup_change: f64,
    pub markup_range: (f64, f64),
    pub hiring_rate: f64,
    pub hiring_price_response: f64,
    pub pay_memory: f64,
    pub wage_adjustment: f64,
    pub money_months: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoneyConfig {
    pub food_price: f64,
    pub initial_savings_months: f64,
    pub initial_business_cash_per_worker: f64,
    pub wage_payout: f64,
    pub sharing_threshold_months: f64,
    pub sharing_rate: f64,
    pub gift_share: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LandConfig {
    pub landless_share: f64,
    pub holding_spread: f64,
    pub keep_months: f64,
    pub partible: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LivestockConfig {
    pub per_plot: f64,
    pub farm_boost: f64,
    pub growth: f64,
    pub winter_capacity_per_plot: f64,
    pub meat_rations: f64,
    pub wool: f64,
    pub shearing_month: i64,
    pub drought_losses: f64,
    pub winter_losses: f64,
    pub value_months: f64,
    pub buyers_spend: f64,
    pub lowest_price: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreditConfig {
    pub enabled: bool,
    pub interest: f64,
    pub lend_share: f64,
    pub loan_to_value: f64,
    pub personal_months: f64,
    pub repay_share: f64,
    pub grain_repay_share: f64,
    pub grain_kept_months: f64,
    pub foreclose_at: f64,
    pub default_at: f64,
    pub years_purchase: f64,
    pub buyers_spend: f64,
    pub lowest_price: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LordConfig {
    pub enabled: bool,
    pub demesne: f64,
    pub labour_service: f64,
    pub carted_away: f64,
    pub sells: f64,
    pub spends_locally: f64,
    pub to_town: f64,
    pub spends_in_town: f64,
    pub servants: f64,
    pub charity_in_famine: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateConfig {
    pub enabled: bool,
    pub collection_month: i64,
    pub hearth_tax_months: f64,
    pub land_tax: f64,
    pub remit_in_famine: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TownConfig {
    pub enabled: bool,
    pub transport: f64,
    pub capacity: f64,
    pub seasonal: f64,
    pub dearest_month: i64,
    pub swing_sd: f64,
    pub swing_memory: f64,
    pub regional_share: f64,
    pub coins_lost_a_year: f64,
    pub mortality: f64,
    pub craft_skill: f64,
    pub woods: f64,
    pub money_months: f64,
    pub store_elasticity: f64,
    pub dealers_months: f64,
    pub keep_months: f64,
    pub dealers_payout: f64,
    pub outside_transport: f64,
    pub outside_capacity: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainSpec {
    pub name: String,
    pub carriage: f64,
    pub pasture: f64,
    pub woods: f64,
    pub forage: f64,
    pub arable: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapConfig {
    pub enabled: bool,
    pub roads: Vec<(usize, usize)>,
    pub rivers: Vec<(usize, usize)>,
    pub handling: f64,
    pub road_per_km: f64,
    pub river_per_km: f64,
    pub weather_km: f64,
    pub capacity: f64,
    pub full_load_margin: f64,
    pub relief_capacity: f64,
    pub town_capacity: f64,
    pub water_capacity: f64,
    pub wholesale_handling: f64,
    pub terrains: Vec<TerrainSpec>,
}

impl MapConfig {
    pub fn terrain_index(&self, name: &str) -> Option<usize> {
        self.terrains.iter().position(|t| t.name == name)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentConfig {
    pub enabled: bool,
    pub woods_years: f64,
    pub regrowth: f64,
    pub reach_exponent: f64,
    pub people_per_plot: f64,
    pub overcropping: f64,
    pub fertility_range: (f64, f64),
    pub soil_recovery: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkConfig {
    pub enabled: bool,
    pub harvest_months: Vec<i64>,
    pub craft_help: f64,
    pub helper_ages: (i64, i64),
    pub old_helper_age: i64,
    pub helper_effort: f64,
    pub home_cloth: f64,
    pub kin_share: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationConfig {
    pub enabled: bool,
    pub usual_prospects: f64,
    pub memory_months: f64,
    pub hunger_weight: f64,
    pub danger_weight: f64,
    pub normal_mortality: f64,
    pub plunder_weight: f64,
    pub small_village: f64,
    pub size_exponent: f64,
    pub land_range: (f64, f64),
    pub usual_take: f64,
    pub burden_exponent: f64,
    pub burden_range: (f64, f64),
    pub young_ages: (i64, i64),
    pub leave_chance: f64,
    pub family_leave_chance: f64,
    pub rooted: f64,
    pub push: f64,
    pub chain: f64,
    pub flee_below: f64,
    pub flee_chance: f64,
    pub flee_danger: f64,
    pub stay_in_region: f64,
    pub distance_km: f64,
    pub to_town: f64,
    pub welcome: f64,
    pub arrive_rate: f64,
    pub max_pull: f64,
    pub settle_rate: f64,
    pub settle_appeal: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CouncilConfig {
    pub enabled: bool,
    pub forms_at_population: i64,
    pub forms_after_months: i64,
    pub established_at_start: bool,
    pub tax_rate: f64,
    pub treasury_months: f64,
    pub officials_per_1000: f64,
    pub official_pay: f64,
    pub grain_levy: f64,
    pub reserve_months: f64,
    pub relief: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HealthcareConfig {
    pub enabled: bool,
    pub healers_per_1000: f64,
    pub healer_pay: f64,
    pub patients_per_healer: f64,
    pub min_risk: f64,
    pub treatment_recovery: f64,
    pub treatment_mortality: f64,
}

/// Families with minds of their own (see `minds.rs`; Python's `MindsConfig`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MindsConfig {
    pub enabled: bool,
    pub trait_spread: f64,
    pub inherited_spread: f64,
    pub memory_months: f64,
    pub learning: f64,
    pub horizon_months: f64,
    pub speculation: f64,
    pub hoard_limit: f64,
    pub hunger_saving: f64,
    pub loss_fear: f64,
    pub hunger_leaving: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventSpec {
    pub name: String,
    pub scope: String,
    pub chance: f64,
    /// Kept in Python's order: ranges are drawn in it when an event starts.
    pub effects: IndexMap<String, EffectValue>,
    pub duration: Duration,
    pub months: Option<Vec<i64>>,
    pub group: Option<String>,
    pub repeat_chance: Option<f64>,
    pub businesses: Option<Vec<String>>,
    pub message: String,
    pub reach: String,
    pub end: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduledEvent {
    pub event: String,
    pub month: i64,
    pub village: Option<usize>,
    pub near: Option<(f64, f64, f64)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub seed: i64,
    pub months: i64,
    pub start_month: i64,
    pub villages: Vec<VillageConfig>,
    pub demography: DemographyConfig,
    pub food: FoodConfig,
    pub health: HealthConfig,
    pub skill: SkillConfig,
    pub money: MoneyConfig,
    pub needs: NeedsConfig,
    pub trade: TradeConfig,
    pub land: LandConfig,
    pub livestock: LivestockConfig,
    pub credit: CreditConfig,
    pub lord: LordConfig,
    pub state: StateConfig,
    pub town: TownConfig,
    pub migration: MigrationConfig,
    pub work: WorkConfig,
    pub environment: EnvironmentConfig,
    pub council: CouncilConfig,
    pub healthcare: HealthcareConfig,
    pub map: MapConfig,
    pub minds: MindsConfig,
    pub products: Vec<ProductSpec>,
    pub businesses: Vec<BusinessSpec>,
    pub events: Vec<EventSpec>,
    pub random_events: bool,
    pub scheduled_events: Vec<ScheduledEvent>,
}

/// A config file that could not be read or doesn't match the parameters.
#[derive(Debug)]
pub struct ConfigError(pub String);

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    pub fn from_json(text: &str) -> Result<Config, ConfigError> {
        serde_json::from_str(text).map_err(|e| ConfigError(format!("config doesn't match the parameters: {e}")))
    }

    pub fn from_file(path: impl AsRef<Path>) -> Result<Config, ConfigError> {
        let path = path.as_ref();
        let text =
            std::fs::read_to_string(path).map_err(|e| ConfigError(format!("can't read {}: {e}", path.display())))?;
        Config::from_json(&text).map_err(|e| ConfigError(format!("{}: {}", path.display(), e.0)))
    }

    pub fn product_index(&self, name: &str) -> usize {
        self.products.iter().position(|p| p.name == name).unwrap_or_else(|| panic!("no product named {name:?}"))
    }

    /// Index of the product with this use (food, heating, comfort, tool).
    pub fn product_for(&self, use_: &str) -> usize {
        self.products.iter().position(|p| p.use_ == use_).unwrap_or_else(|| panic!("no product used for {use_:?}"))
    }

    pub fn business_index(&self, name: &str) -> usize {
        self.businesses.iter().position(|b| b.name == name).unwrap_or_else(|| panic!("no business named {name:?}"))
    }

    /// Index of the business that grows food on the land.
    pub fn farming(&self) -> usize {
        self.businesses.iter().position(|b| b.uses_land).expect("a business that uses the land")
    }

    pub fn n_places(&self) -> usize {
        self.villages.len()
    }
}

/// Python's default `Config()` (the default village), as exported to
/// `rust/configs/village.json` and built into the engine.
pub const DEFAULT_CONFIG_JSON: &str = include_str!("../../configs/village.json");

/// The default village, as Python's `Config()`.
pub fn default_config() -> Config {
    Config::from_json(DEFAULT_CONFIG_JSON).expect("rust/configs/village.json matches the parameters")
}

/// The folder of checked-in configs exported from Python (`rust/configs`),
/// for tests and tools run from the source tree.
pub fn configs_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("configs")
}
