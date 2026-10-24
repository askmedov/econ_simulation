//! Monthly statistics: totals over the population table, never per-person logs.

use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::population::Population;

/// A value in a record's flat row.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Int(i64),
    Float(f64),
    Text(String),
}

impl Cell {
    pub fn as_f64(&self) -> f64 {
        match self {
            Cell::Int(v) => *v as f64,
            Cell::Float(v) => *v,
            Cell::Text(_) => f64::NAN,
        }
    }

    /// In full: floats with every digit needed to read them back exactly.
    pub fn full(&self) -> String {
        match self {
            Cell::Int(v) => v.to_string(),
            Cell::Float(v) => python_float(*v),
            Cell::Text(t) => csv_text(t),
        }
    }

    /// As Python's CSV writer shows it: floats rounded to 3 decimals.
    pub fn csv(&self) -> String {
        match self {
            Cell::Int(v) => v.to_string(),
            Cell::Float(v) => python_float(round_to(*v, 3)),
            Cell::Text(t) => csv_text(t),
        }
    }
}

/// `value` rounded to `decimals` decimals.
pub fn round_to(value: f64, decimals: i32) -> f64 {
    let scale = 10f64.powi(decimals);
    (value * scale).round() / scale
}

/// A float as Python's `str` shows it (2.0, not 2; nan and inf in lower case).
pub fn python_float(value: f64) -> String {
    if value.is_nan() {
        return "nan".into();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf".into() } else { "-inf".into() };
    }
    let text = format!("{value}");
    if text.contains('.') || text.contains('e') {
        text
    } else {
        format!("{text}.0")
    }
}

/// Quote a CSV field if it needs it.
pub fn csv_text(text: &str) -> String {
    if text.contains(',') || text.contains('"') || text.contains('\n') {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

/// One month's totals (never per-person logs), in the order Python writes them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MonthRecord {
    /// Months from the start, 1 = first simulated month.
    pub month_number: i64,
    pub year: i64,
    /// Calendar month, 1-12.
    pub month: i64,
    pub population: i64,
    pub children: i64,
    pub workers: i64,
    pub elderly: i64,
    /// With at least one member.
    pub households: i64,
    /// Couples who left home to start a household.
    pub weddings: i64,
    pub births: i64,
    pub deaths: i64,
    pub food_produced: f64,
    pub food_needed: f64,
    /// From families' own stores, bought, relief from the council's reserve, and foraged.
    pub food_eaten: f64,
    /// Rations families ate from their own grain stores.
    pub own_food: f64,
    /// Rations of famine foods (roots, greens, fish) hungry families found.
    pub foraged: f64,
    /// Rations sold on the village market.
    pub grain_sold: f64,
    /// Share of food need met, 1.0 = everyone fully fed.
    pub ration: f64,
    pub food_spoiled: f64,
    /// Destroyed by events.
    pub food_lost: f64,
    /// Taken by the council into its reserve (levy, and stores of families who died out).
    pub food_levied: f64,
    /// Picked from the harvest as seed, or given by families at sowing.
    pub food_to_seed: f64,
    /// Rations of meat from animals slaughtered (the winter cull, or in hunger), into stores.
    pub meat: f64,
    /// Grain kept for the next sowing.
    pub seed_store: f64,
    /// Share of the needed seed sown at the last sowing (averaged over villages).
    pub sown: f64,
    /// Livestock units held by families.
    pub animals: f64,
    /// Died this month, net of births (negative: the herds grew).
    pub animals_lost: f64,
    /// Sold by families who couldn't afford their food.
    pub animals_sold: f64,
    /// Coins per animal at this month's sales.
    pub animal_price: f64,
    /// Coins families owe the village's lenders.
    pub debt: f64,
    /// Families in debt.
    pub debtors: i64,
    /// Coins of interest added to debts this month.
    pub interest: f64,
    /// Coins lent to families who couldn't afford their food.
    pub borrowed: f64,
    /// Coins paid back.
    pub repaid: f64,
    /// Rations paid back in kind.
    pub repaid_in_grain: f64,
    /// Coins of debt cancelled by decree.
    pub debts_cancelled: f64,
    /// Coins of debt that could never be repaid, lost to the lenders.
    pub debts_written_off: f64,
    /// Plots sold by families who couldn't afford their food.
    pub land_sold: f64,
    /// Plots taken by lenders for debts.
    pub land_foreclosed: f64,
    /// Animals taken by lenders for debts.
    pub animals_foreclosed: f64,
    /// Coins a plot is worth (years of its rent).
    pub land_price: f64,
    /// Rations of the harvest that went to the lord (his demesne and labour days).
    pub food_rent: f64,
    /// Rations carted from the lord's barn to his hall.
    pub lord_carted: f64,
    /// Rations the lord's steward sold in the village.
    pub lord_sold: f64,
    /// Rations a charitable lord gave the hungry.
    pub lord_relief: f64,
    /// Rations in the lord's barn.
    pub lord_barn: f64,
    /// Coins the lord has taken out of the village.
    pub lord_purse: f64,
    /// Coins the state collected this month.
    pub state_tax: f64,
    /// Rations seized from families who couldn't pay the state's tax.
    pub tax_grain: f64,
    /// Coins the state has taken out of the village.
    pub state_purse: f64,
    /// Rations soldiers or raiders took from families and farms.
    pub food_requisitioned: f64,
    /// Coins per ration in the town's grain market (with market towns: the world beyond's).
    pub town_price: f64,
    /// Rations merchants carried from villages to the town.
    pub grain_exported: f64,
    /// Rations merchants brought from the town to villages and sold there.
    pub grain_imported: f64,
    /// Coins the villages have paid the town (or the world beyond), net (negative: received).
    pub town_purse: f64,
    /// People in the market towns (on a map).
    pub town_people: i64,
    /// Share of the towns' food need eaten.
    pub town_ration: f64,
    /// Coins per ration in the market towns (by their need).
    pub market_price: f64,
    /// Rations in the towns' grain dealers' stores.
    pub dealers_grain: f64,
    /// Coins the towns' grain dealers hold.
    pub dealers_cash: f64,
    /// Rations border towns bought from the world beyond, net (negative: sold).
    pub grain_beyond: f64,
    /// Rations the dealers of neighbouring towns sold each other.
    pub grain_between_towns: f64,
    /// Coins the region's wool fetched beyond it (at shearing).
    pub wool_sold: f64,
    /// Coins lost, worn away or buried since the start.
    pub coins_lost: f64,
    /// People who left their village (for the town, beyond, or another village).
    pub emigrants: i64,
    /// People who came to a village (from beyond, or another village).
    pub immigrants: i64,
    /// Whole families that left their village.
    pub families_left: i64,
    /// People who settled in another village of the run.
    pub moved_within: i64,
    /// How good a place to live, 1 = an ordinary village (averaged over people).
    pub appeal: f64,
    /// Villages with nobody left.
    pub empty_villages: i64,
    /// Rations leavers took with them.
    pub food_emigrated: f64,
    /// At the end of the month.
    pub food_stock: f64,
    /// Normal harvest over need; below ~1.05 the land is crowded.
    pub food_margin: f64,
    /// Stores plus expected harvests, as a share of the coming year's need.
    pub food_cover: f64,
    /// Food a month's pay buys, over food need per worker.
    pub wage_cover: f64,
    /// Coins per ration (averaged over villages).
    pub food_price: f64,
    /// Average monthly wage of a worker, in coins.
    pub wage: f64,
    /// Coins held by families.
    pub savings: f64,
    /// Families' coins over a month of their coin earnings.
    pub money_months: f64,
    /// Coins held by businesses.
    pub business_cash: f64,
    /// Coins held by village councils.
    pub treasury: f64,
    /// Villages with a council.
    pub councils: i64,
    /// Council officials.
    pub officials: i64,
    /// Coins collected in tax this month.
    pub taxes: f64,
    /// Rations held by councils for famine relief.
    pub food_reserve: f64,
    /// Rations the council gave free to families who couldn't buy enough.
    pub relief: f64,
    /// Coins the council gave families who couldn't afford food and firewood.
    pub cash_relief: f64,
    /// Healers employed by the council.
    pub healers: i64,
    /// People healers saw this month.
    pub treated: i64,
    /// Coins given by better-off families to families short of food money.
    pub shared: f64,
    /// People whose family got less than 90% of its food need.
    pub underfed: i64,
    /// People in families holding no land.
    pub landless: i64,
    /// Share of food need met for people in landless families.
    pub landless_ration: f64,
    /// Share of food need met for the poorest fifth of people.
    pub poorest_fifth_ration: f64,
    /// Share of the firewood families needed that they got.
    pub warmth: f64,
    /// Garments worn per person this month: bought, and homespun.
    pub clothing: f64,
    /// Garments women spun and wove at home.
    pub homespun: f64,
    /// Workers' worth of help in the fields from outside farming.
    pub harvest_help: f64,
    /// Rations kin gave families who couldn't afford their food.
    pub kin_help: f64,
    /// Wood standing in the woods, as a share of what they held at the start.
    pub woods: f64,
    /// Firewood's worth of woods burned this month.
    pub woods_burned: f64,
    /// Soil fertility, 1 = normal (averaged over villages by land).
    pub soil: f64,
    /// Workers who moved to a better-paid trade.
    pub job_changes: i64,
    /// Coins per unit of each product.
    pub prices: Vec<(String, f64)>,
    /// Workers in each business.
    pub jobs: Vec<(String, i64)>,
    pub avg_health: f64,
    /// People below the danger threshold.
    pub poor_health: i64,
    pub accidents: i64,
    /// Active village events.
    pub events: String,
}

impl MonthRecord {
    /// The record as one flat row: prices and jobs become price_<product> and jobs_<business>.
    pub fn flat(&self) -> Vec<(String, Cell)> {
        let mut row: Vec<(String, Cell)> = Vec::with_capacity(128);
        row.push(("month_number".into(), Cell::Int(self.month_number)));
        row.push(("year".into(), Cell::Int(self.year)));
        row.push(("month".into(), Cell::Int(self.month)));
        row.push(("population".into(), Cell::Int(self.population)));
        row.push(("children".into(), Cell::Int(self.children)));
        row.push(("workers".into(), Cell::Int(self.workers)));
        row.push(("elderly".into(), Cell::Int(self.elderly)));
        row.push(("households".into(), Cell::Int(self.households)));
        row.push(("weddings".into(), Cell::Int(self.weddings)));
        row.push(("births".into(), Cell::Int(self.births)));
        row.push(("deaths".into(), Cell::Int(self.deaths)));
        row.push(("food_produced".into(), Cell::Float(self.food_produced)));
        row.push(("food_needed".into(), Cell::Float(self.food_needed)));
        row.push(("food_eaten".into(), Cell::Float(self.food_eaten)));
        row.push(("own_food".into(), Cell::Float(self.own_food)));
        row.push(("foraged".into(), Cell::Float(self.foraged)));
        row.push(("grain_sold".into(), Cell::Float(self.grain_sold)));
        row.push(("ration".into(), Cell::Float(self.ration)));
        row.push(("food_spoiled".into(), Cell::Float(self.food_spoiled)));
        row.push(("food_lost".into(), Cell::Float(self.food_lost)));
        row.push(("food_levied".into(), Cell::Float(self.food_levied)));
        row.push(("food_to_seed".into(), Cell::Float(self.food_to_seed)));
        row.push(("meat".into(), Cell::Float(self.meat)));
        row.push(("seed_store".into(), Cell::Float(self.seed_store)));
        row.push(("sown".into(), Cell::Float(self.sown)));
        row.push(("animals".into(), Cell::Float(self.animals)));
        row.push(("animals_lost".into(), Cell::Float(self.animals_lost)));
        row.push(("animals_sold".into(), Cell::Float(self.animals_sold)));
        row.push(("animal_price".into(), Cell::Float(self.animal_price)));
        row.push(("debt".into(), Cell::Float(self.debt)));
        row.push(("debtors".into(), Cell::Int(self.debtors)));
        row.push(("interest".into(), Cell::Float(self.interest)));
        row.push(("borrowed".into(), Cell::Float(self.borrowed)));
        row.push(("repaid".into(), Cell::Float(self.repaid)));
        row.push(("repaid_in_grain".into(), Cell::Float(self.repaid_in_grain)));
        row.push(("debts_cancelled".into(), Cell::Float(self.debts_cancelled)));
        row.push(("debts_written_off".into(), Cell::Float(self.debts_written_off)));
        row.push(("land_sold".into(), Cell::Float(self.land_sold)));
        row.push(("land_foreclosed".into(), Cell::Float(self.land_foreclosed)));
        row.push(("animals_foreclosed".into(), Cell::Float(self.animals_foreclosed)));
        row.push(("land_price".into(), Cell::Float(self.land_price)));
        row.push(("food_rent".into(), Cell::Float(self.food_rent)));
        row.push(("lord_carted".into(), Cell::Float(self.lord_carted)));
        row.push(("lord_sold".into(), Cell::Float(self.lord_sold)));
        row.push(("lord_relief".into(), Cell::Float(self.lord_relief)));
        row.push(("lord_barn".into(), Cell::Float(self.lord_barn)));
        row.push(("lord_purse".into(), Cell::Float(self.lord_purse)));
        row.push(("state_tax".into(), Cell::Float(self.state_tax)));
        row.push(("tax_grain".into(), Cell::Float(self.tax_grain)));
        row.push(("state_purse".into(), Cell::Float(self.state_purse)));
        row.push(("food_requisitioned".into(), Cell::Float(self.food_requisitioned)));
        row.push(("town_price".into(), Cell::Float(self.town_price)));
        row.push(("grain_exported".into(), Cell::Float(self.grain_exported)));
        row.push(("grain_imported".into(), Cell::Float(self.grain_imported)));
        row.push(("town_purse".into(), Cell::Float(self.town_purse)));
        row.push(("town_people".into(), Cell::Int(self.town_people)));
        row.push(("town_ration".into(), Cell::Float(self.town_ration)));
        row.push(("market_price".into(), Cell::Float(self.market_price)));
        row.push(("dealers_grain".into(), Cell::Float(self.dealers_grain)));
        row.push(("dealers_cash".into(), Cell::Float(self.dealers_cash)));
        row.push(("grain_beyond".into(), Cell::Float(self.grain_beyond)));
        row.push(("grain_between_towns".into(), Cell::Float(self.grain_between_towns)));
        row.push(("wool_sold".into(), Cell::Float(self.wool_sold)));
        row.push(("coins_lost".into(), Cell::Float(self.coins_lost)));
        row.push(("emigrants".into(), Cell::Int(self.emigrants)));
        row.push(("immigrants".into(), Cell::Int(self.immigrants)));
        row.push(("families_left".into(), Cell::Int(self.families_left)));
        row.push(("moved_within".into(), Cell::Int(self.moved_within)));
        row.push(("appeal".into(), Cell::Float(self.appeal)));
        row.push(("empty_villages".into(), Cell::Int(self.empty_villages)));
        row.push(("food_emigrated".into(), Cell::Float(self.food_emigrated)));
        row.push(("food_stock".into(), Cell::Float(self.food_stock)));
        row.push(("food_margin".into(), Cell::Float(self.food_margin)));
        row.push(("food_cover".into(), Cell::Float(self.food_cover)));
        row.push(("wage_cover".into(), Cell::Float(self.wage_cover)));
        row.push(("food_price".into(), Cell::Float(self.food_price)));
        row.push(("wage".into(), Cell::Float(self.wage)));
        row.push(("savings".into(), Cell::Float(self.savings)));
        row.push(("money_months".into(), Cell::Float(self.money_months)));
        row.push(("business_cash".into(), Cell::Float(self.business_cash)));
        row.push(("treasury".into(), Cell::Float(self.treasury)));
        row.push(("councils".into(), Cell::Int(self.councils)));
        row.push(("officials".into(), Cell::Int(self.officials)));
        row.push(("taxes".into(), Cell::Float(self.taxes)));
        row.push(("food_reserve".into(), Cell::Float(self.food_reserve)));
        row.push(("relief".into(), Cell::Float(self.relief)));
        row.push(("cash_relief".into(), Cell::Float(self.cash_relief)));
        row.push(("healers".into(), Cell::Int(self.healers)));
        row.push(("treated".into(), Cell::Int(self.treated)));
        row.push(("shared".into(), Cell::Float(self.shared)));
        row.push(("underfed".into(), Cell::Int(self.underfed)));
        row.push(("landless".into(), Cell::Int(self.landless)));
        row.push(("landless_ration".into(), Cell::Float(self.landless_ration)));
        row.push(("poorest_fifth_ration".into(), Cell::Float(self.poorest_fifth_ration)));
        row.push(("warmth".into(), Cell::Float(self.warmth)));
        row.push(("clothing".into(), Cell::Float(self.clothing)));
        row.push(("homespun".into(), Cell::Float(self.homespun)));
        row.push(("harvest_help".into(), Cell::Float(self.harvest_help)));
        row.push(("kin_help".into(), Cell::Float(self.kin_help)));
        row.push(("woods".into(), Cell::Float(self.woods)));
        row.push(("woods_burned".into(), Cell::Float(self.woods_burned)));
        row.push(("soil".into(), Cell::Float(self.soil)));
        row.push(("job_changes".into(), Cell::Int(self.job_changes)));
        row.extend(self.prices.iter().map(|(k, v)| (format!("price_{k}"), Cell::Float(*v))));
        row.extend(self.jobs.iter().map(|(k, v)| (format!("jobs_{k}"), Cell::Int(*v))));
        row.push(("avg_health".into(), Cell::Float(self.avg_health)));
        row.push(("poor_health".into(), Cell::Int(self.poor_health)));
        row.push(("accidents".into(), Cell::Int(self.accidents)));
        row.push(("events".into(), Cell::Text(self.events.clone())));
        row
    }
}

impl MonthRecord {
    /// A measure by name, including flattened ones like "price_firewood" or "jobs_farming".
    pub fn value(&self, name: &str) -> f64 {
        self.flat()
            .into_iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_f64())
            .unwrap_or_else(|| panic!("no measure named {name:?}"))
    }
}

/// (children, working age, elderly) headcounts.
pub fn age_groups(population: &Population, config: &Config) -> (i64, i64, i64) {
    let (mut children, mut workers) = (0i64, 0i64);
    for i in 0..population.len() {
        let age = population.age_years(i) as i64;
        if age < config.demography.adult_age {
            children += population.count[i] as i64;
        }
        if age >= config.demography.adult_age && age < config.demography.retirement_age {
            workers += population.count[i] as i64;
        }
    }
    (children, workers, population.size() as i64 - children - workers)
}

pub fn average_health(population: &Population) -> f64 {
    let size = population.size();
    if size == 0 {
        return 0.0;
    }
    let total: f64 = (0..population.len()).map(|i| population.health[i] * population.count[i] as f64).sum();
    total / size as f64
}

pub fn poor_health(population: &Population, config: &Config) -> i64 {
    (0..population.len())
        .filter(|&i| population.health[i] < config.health.danger_threshold)
        .map(|i| population.count[i] as i64)
        .sum()
}

/// Share of food need met for the fifth of people in the poorest families
/// (by `money` per person: savings, or savings plus grain in store).
pub fn poorest_fifth_ration(money: &[f64], size: &[f64], eaten: &[f64], need: &[f64]) -> f64 {
    let lived: Vec<usize> = (0..size.len()).filter(|&h| size[h] > 0.0).collect();
    if lived.is_empty() {
        return 1.0;
    }
    let mut order = lived.clone();
    order
        .sort_by(|&a, &b| (money[a] / size[a]).partial_cmp(&(money[b] / size[b])).unwrap_or(std::cmp::Ordering::Equal));
    let people: f64 = lived.iter().map(|&h| size[h]).sum();
    let (mut before, mut got, mut wanted) = (0.0, 0.0, 0.0);
    for &h in &order {
        if before >= 0.2 * people {
            break;
        }
        got += eaten[h];
        wanted += need[h];
        before += size[h];
    }
    if wanted > 0.0 {
        got / wanted
    } else {
        1.0
    }
}

/// Write the records as a CSV, one row per month, as Python's `write_csv`.
pub fn write_csv(records: &[MonthRecord], path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
    let header: Vec<String> = match records.first() {
        Some(r) => r.flat().into_iter().map(|(n, _)| n).collect(),
        None => MonthRecord::default().flat().into_iter().map(|(n, _)| n).collect(),
    };
    write!(out, "{}\r\n", header.join(","))?; // (line ends as Python's csv module writes them)
    for record in records {
        let row: Vec<String> = record.flat().iter().map(|(_, v)| v.csv()).collect();
        write!(out, "{}\r\n", row.join(","))?;
    }
    out.flush()
}
