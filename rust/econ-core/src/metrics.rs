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
    /// Coins the region's crafts fetched beyond it, at its border towns
    /// (Rust only: see `CraftsConfig`).
    pub crafts_sold: f64,
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
    /// Each field in order with its name, as one flat row: prices and jobs
    /// become price_<product> and jobs_<business>.
    pub fn visit(&self, mut f: impl FnMut(&str, Cell)) {
        f("month_number", Cell::Int(self.month_number));
        f("year", Cell::Int(self.year));
        f("month", Cell::Int(self.month));
        f("population", Cell::Int(self.population));
        f("children", Cell::Int(self.children));
        f("workers", Cell::Int(self.workers));
        f("elderly", Cell::Int(self.elderly));
        f("households", Cell::Int(self.households));
        f("weddings", Cell::Int(self.weddings));
        f("births", Cell::Int(self.births));
        f("deaths", Cell::Int(self.deaths));
        f("food_produced", Cell::Float(self.food_produced));
        f("food_needed", Cell::Float(self.food_needed));
        f("food_eaten", Cell::Float(self.food_eaten));
        f("own_food", Cell::Float(self.own_food));
        f("foraged", Cell::Float(self.foraged));
        f("grain_sold", Cell::Float(self.grain_sold));
        f("ration", Cell::Float(self.ration));
        f("food_spoiled", Cell::Float(self.food_spoiled));
        f("food_lost", Cell::Float(self.food_lost));
        f("food_levied", Cell::Float(self.food_levied));
        f("food_to_seed", Cell::Float(self.food_to_seed));
        f("meat", Cell::Float(self.meat));
        f("seed_store", Cell::Float(self.seed_store));
        f("sown", Cell::Float(self.sown));
        f("animals", Cell::Float(self.animals));
        f("animals_lost", Cell::Float(self.animals_lost));
        f("animals_sold", Cell::Float(self.animals_sold));
        f("animal_price", Cell::Float(self.animal_price));
        f("debt", Cell::Float(self.debt));
        f("debtors", Cell::Int(self.debtors));
        f("interest", Cell::Float(self.interest));
        f("borrowed", Cell::Float(self.borrowed));
        f("repaid", Cell::Float(self.repaid));
        f("repaid_in_grain", Cell::Float(self.repaid_in_grain));
        f("debts_cancelled", Cell::Float(self.debts_cancelled));
        f("debts_written_off", Cell::Float(self.debts_written_off));
        f("land_sold", Cell::Float(self.land_sold));
        f("land_foreclosed", Cell::Float(self.land_foreclosed));
        f("animals_foreclosed", Cell::Float(self.animals_foreclosed));
        f("land_price", Cell::Float(self.land_price));
        f("food_rent", Cell::Float(self.food_rent));
        f("lord_carted", Cell::Float(self.lord_carted));
        f("lord_sold", Cell::Float(self.lord_sold));
        f("lord_relief", Cell::Float(self.lord_relief));
        f("lord_barn", Cell::Float(self.lord_barn));
        f("lord_purse", Cell::Float(self.lord_purse));
        f("state_tax", Cell::Float(self.state_tax));
        f("tax_grain", Cell::Float(self.tax_grain));
        f("state_purse", Cell::Float(self.state_purse));
        f("food_requisitioned", Cell::Float(self.food_requisitioned));
        f("town_price", Cell::Float(self.town_price));
        f("grain_exported", Cell::Float(self.grain_exported));
        f("grain_imported", Cell::Float(self.grain_imported));
        f("town_purse", Cell::Float(self.town_purse));
        f("town_people", Cell::Int(self.town_people));
        f("town_ration", Cell::Float(self.town_ration));
        f("market_price", Cell::Float(self.market_price));
        f("dealers_grain", Cell::Float(self.dealers_grain));
        f("dealers_cash", Cell::Float(self.dealers_cash));
        f("grain_beyond", Cell::Float(self.grain_beyond));
        f("grain_between_towns", Cell::Float(self.grain_between_towns));
        f("wool_sold", Cell::Float(self.wool_sold));
        f("crafts_sold", Cell::Float(self.crafts_sold));
        f("coins_lost", Cell::Float(self.coins_lost));
        f("emigrants", Cell::Int(self.emigrants));
        f("immigrants", Cell::Int(self.immigrants));
        f("families_left", Cell::Int(self.families_left));
        f("moved_within", Cell::Int(self.moved_within));
        f("appeal", Cell::Float(self.appeal));
        f("empty_villages", Cell::Int(self.empty_villages));
        f("food_emigrated", Cell::Float(self.food_emigrated));
        f("food_stock", Cell::Float(self.food_stock));
        f("food_margin", Cell::Float(self.food_margin));
        f("food_cover", Cell::Float(self.food_cover));
        f("wage_cover", Cell::Float(self.wage_cover));
        f("food_price", Cell::Float(self.food_price));
        f("wage", Cell::Float(self.wage));
        f("savings", Cell::Float(self.savings));
        f("money_months", Cell::Float(self.money_months));
        f("business_cash", Cell::Float(self.business_cash));
        f("treasury", Cell::Float(self.treasury));
        f("councils", Cell::Int(self.councils));
        f("officials", Cell::Int(self.officials));
        f("taxes", Cell::Float(self.taxes));
        f("food_reserve", Cell::Float(self.food_reserve));
        f("relief", Cell::Float(self.relief));
        f("cash_relief", Cell::Float(self.cash_relief));
        f("healers", Cell::Int(self.healers));
        f("treated", Cell::Int(self.treated));
        f("shared", Cell::Float(self.shared));
        f("underfed", Cell::Int(self.underfed));
        f("landless", Cell::Int(self.landless));
        f("landless_ration", Cell::Float(self.landless_ration));
        f("poorest_fifth_ration", Cell::Float(self.poorest_fifth_ration));
        f("warmth", Cell::Float(self.warmth));
        f("clothing", Cell::Float(self.clothing));
        f("homespun", Cell::Float(self.homespun));
        f("harvest_help", Cell::Float(self.harvest_help));
        f("kin_help", Cell::Float(self.kin_help));
        f("woods", Cell::Float(self.woods));
        f("woods_burned", Cell::Float(self.woods_burned));
        f("soil", Cell::Float(self.soil));
        f("job_changes", Cell::Int(self.job_changes));
        for (k, v) in &self.prices {
            f(&format!("price_{k}"), Cell::Float(*v));
        }
        for (k, v) in &self.jobs {
            f(&format!("jobs_{k}"), Cell::Int(*v));
        }
        f("avg_health", Cell::Float(self.avg_health));
        f("poor_health", Cell::Int(self.poor_health));
        f("accidents", Cell::Int(self.accidents));
        f("events", Cell::Text(self.events.clone()));
    }

    /// The record as one flat row of (name, value).
    pub fn flat(&self) -> Vec<(String, Cell)> {
        let mut row = Vec::with_capacity(128);
        self.visit(|name, cell| row.push((name.to_string(), cell)));
        row
    }
}

impl MonthRecord {
    /// A measure by name, including flattened ones like "price_firewood" or "jobs_farming".
    pub fn value(&self, name: &str) -> f64 {
        // (looked up thousands of times for a report: without building the row)
        let named = |list: &[(String, f64)], key: &str| list.iter().find(|(k, _)| k == key).map(|&(_, v)| v);
        if let Some(found) = name.strip_prefix("price_").and_then(|product| named(&self.prices, product)) {
            return found;
        }
        if let Some(business) = name.strip_prefix("jobs_") {
            if let Some(&(_, v)) = self.jobs.iter().find(|(k, _)| k == business) {
                return v as f64;
            }
        }
        let mut found = None;
        self.visit(|n, cell| {
            if found.is_none() && n == name {
                found = Some(cell.as_f64());
            }
        });
        found.unwrap_or_else(|| panic!("no measure named {name:?}"))
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
    // Poorest first by coins a head (a stable sort: equals in family order).
    let mut order: Vec<(f64, usize)> = lived.iter().map(|&h| (money[h] / size[h], h)).collect();
    order.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let people: f64 = lived.iter().map(|&h| size[h]).sum();
    let (mut before, mut got, mut wanted) = (0.0, 0.0, 0.0);
    for &(_, h) in &order {
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
