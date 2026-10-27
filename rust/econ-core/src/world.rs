//! The state of the simulated world, and how it is set up at the start.

use crate::config::Config;
use crate::council::{self, Councils};
use crate::crafts;
use crate::economy;
use crate::environment;
use crate::events::{ActiveEvent, LastStarted};
use crate::geography::{self, Geography};
use crate::grid::{average, div_or, Grid, Grid3};
use crate::households::{self, Households};
use crate::livestock;
use crate::lords::{self, Lords};
use crate::migration::Memory;
use crate::minds::Minds;
use crate::population::{Population, NO_JOB};
use crate::rng::{cumulative, RandomStreams, Rng};
use crate::rules;
use crate::town::Town;
use crate::towns;

/// On a map, a remote village's prices start at least this share of the
/// region's (it lives off its own grain, and sees few coins).
pub const MIN_PRICE_LEVEL: f64 = 0.3;

#[derive(Clone, Debug)]
pub struct World {
    pub month: i64, // months since the start; 0 is January of year 1
    pub population: Population,
    pub land: Vec<f64>, // farmland per place, in plots
    pub households: Households,
    /// Goods held by the business that made them (places x products); the
    /// food column is the farms' grain (the harvest before it is shared out).
    pub stock: Grid,
    pub prices: Grid,         // coins per unit (places x products): fair price x mark-up
    pub markup: Grid,         // (places x products), 1 = selling at the fair price
    pub wage_level: Vec<f64>, // customary monthly wage per place: fair prices are reckoned in it
    pub cash: Grid,           // coins held by each business (places x businesses)
    pub tools: Grid,          // tools in use by each business (places x businesses)
    pub supplies: Grid3,      // goods a business holds to make its own (places x businesses x products)
    pub pay: Grid,            // running average monthly pay per worker (places x businesses)
    pub orders: Grid,         // running average units asked for a month (places x products)
    pub food: usize,          // index of the food product
    pub council: Councils,
    pub names: Vec<String>, // place names
    pub active_events: Vec<ActiveEvent>,
    pub council_costs: Vec<f64>,         // last month's council running costs, per place
    pub coin_earnings: Option<Vec<f64>>, // running average of families' monthly coin income, per place
    pub seed: Option<Vec<f64>>,          // grain kept back to sow at the next sowing (set in the first month)
    pub lord: Lords,                     // the lord's demesne, barn and purse, and the state's purse
    pub town: Town,                      // the regional grain market, and coins gone to it or lost
    pub memory: Memory,                  // what people remember of each place
    pub sown: Vec<f64>,                  // share of the needed seed sown at the last sowing, per place
    pub last_started: LastStarted,       // (event, place) -> month it last started
    pub woods: Vec<f64>,                 // wood standing in each place's woods, in units of firewood
    pub woods_capacity: Vec<f64>,        // what the woods would hold untouched
    pub woods_normal: Vec<f64>,          // what they held at the start (in balance with the place's use)
    pub soil: Vec<f64>,                  // soil fertility per place, 1 = normal
    pub geo: Geography,
    // Market towns (on a map): their grain dealers' store and coins, the
    // grain they expect to come in each month, what each village sends
    // them, and what towns do to their people (death rates, per place) and
    // craftsmen (output a day, places x businesses).
    pub dealers_grain: Vec<f64>,
    pub dealers_cash: Vec<f64>,
    pub arrivals: Vec<f64>,
    pub sends: Vec<f64>,
    pub unhealthy: Vec<f64>,
    /// People who came as famine refugees, fading over
    /// `refugees.memory_months`, per place (with `refugees.enabled`).
    pub refugees: Vec<f64>,
    pub skill: Grid,
    /// Where a trade lacks its resource (places x businesses: see `crafts`).
    pub lacking: Vec<Vec<bool>>,
    /// Where each place lacking a good gets it from (per product: see `crafts`).
    pub supply: Vec<Option<crafts::SupplyLine>>,
}

impl World {
    /// Grain held by the farms (this month's harvest before it is shared
    /// out, and what they keep to sell for tools), per place.
    pub fn farm_grain(&self) -> Vec<f64> {
        self.stock.column(self.food)
    }

    pub fn set_farm_grain(&mut self, grain: &[f64]) {
        let food = self.food;
        self.stock.set_column(food, grain);
    }

    /// All food in store per place: families' own stores plus the farms'
    /// (and in a town, its grain dealers').
    pub fn granary(&self) -> Vec<f64> {
        let families = self.households.by_place(self.n_locations(), |h| self.households.grain[h]);
        (0..self.n_locations()).map(|v| families[v] + self.stock.get(v, self.food) + self.dealers_grain[v]).collect()
    }

    pub fn food_price(&self) -> Vec<f64> {
        self.prices.column(self.food)
    }

    pub fn year(&self) -> i64 {
        self.month / 12 + 1
    }

    /// Calendar month, 1-12.
    pub fn month_of_year(&self) -> i64 {
        self.month % 12 + 1
    }

    pub fn n_locations(&self) -> usize {
        self.names.len()
    }

    /// Coins held in the places: by families, businesses, councils and grain dealers.
    pub fn village_money(&self) -> f64 {
        self.households.money.iter().sum::<f64>()
            + self.cash.sum()
            + self.council.treasury.iter().sum::<f64>()
            + self.dealers_cash.iter().sum::<f64>()
    }

    /// Every coin there was at the start: in the places, taken by the lord
    /// and the state, gone to the town (net), or lost. Never changes.
    pub fn money(&self) -> f64 {
        let outside = self.lord.purse.iter().sum::<f64>()
            + self.lord.state_purse.iter().sum::<f64>()
            + self.town.purse
            + self.town.coins_lost;
        self.village_money() + outside
    }
}

/// Set up the world from `config` (see Python's `world.create_world`).
pub fn create_world(config: &Config, streams: &mut RandomStreams) -> Result<World, String> {
    let n = config.villages.len();
    let nb = config.businesses.len();
    let np = config.products.len();
    let mut population = Population::default();
    {
        let rng = streams.get("setup");
        for (location, village) in config.villages.iter().enumerate() {
            initial_people(&mut population, village.population as usize, location as u32, config, rng);
        }
    }
    let yearly_marriage = 1.0 - (1.0 - config.demography.marriage_chance).powi(12);
    let mut households = households::form_households(&mut population, n, streams.get("setup"), yearly_marriage);
    households::link_kin(&mut households, &population, streams.get("kin"));
    let geo = geography::build(config)?;
    let lacking = crafts::lacking(config, &geo);
    let lacks = crafts::any_lacking(&lacking).then_some(lacking.as_slice());
    let supply = crafts::supply_lines(config, &geo, &lacking);
    economy::assign_starting_jobs(&mut population, config, streams.get("setup"), Some(&geo.town), lacks);
    let land: Vec<f64> = config.villages.iter().map(|v| v.land).collect();
    let food = config.product_for("food");

    // Businesses start with a full set of tools, some cash, and some goods.
    let force = economy::workforce(&population, n, config);
    let workers = economy::headcount(&population, n, config, Some(&force));
    let mut tools = Grid::zeros(n, nb);
    for r in 0..n {
        for (b, spec) in config.businesses.iter().enumerate() {
            if spec.tool_boost > 0.0 {
                tools.set(r, b, workers.get(r, b));
            }
        }
    }

    // The lord holds his demesne; families hold the rest, and landholders
    // the plough animals.
    let demesne = lords::take_demesne(&land, config);
    let farmland: Vec<f64> = land.iter().zip(&demesne).map(|(l, d)| l - d).collect();
    households::assign_land(&mut households, &population, &farmland, config, streams.get("land"));
    livestock::place_herds(&mut households, &land, config, streams.get("land"), Some(&geo.pasture));
    for animals in households.animals.iter_mut() {
        *animals *= config.livestock.initial_herds; // (a starting point may have fewer)
    }
    let herds = households.by_place(n, |h| households.animals[h]);
    let animals = livestock::farm_factor(&herds, &land, config);

    // Prices start fair, with the wage set so food costs `food_price`; on a
    // map, less where carting grain to market costs more (a remote village
    // sells its grain for less, and has fewer coins). In a town, the
    // craftsmen are more skilled and the wage is the countryside's; grain
    // costs what it takes to bring it in.
    let has_towns = geo.has_towns();
    let farming = config.farming();
    let mut skill = Grid::new(n, nb, 1.0);
    let mut unhealthy = vec![1.0; n];
    if has_towns {
        for r in (0..n).filter(|&r| geo.town[r]) {
            for b in (0..nb).filter(|&b| b != farming) {
                skill.set(r, b, config.town.craft_skill);
            }
            unhealthy[r] = config.town.mortality;
        }
    }
    // A trade can't be worked where its resource is lacking.
    for (r, row) in lacking.iter().enumerate() {
        for (b, &lacks) in row.iter().enumerate() {
            if lacks {
                skill.set(r, b, 0.0);
            }
        }
    }
    let per_worker = economy::productivity(&population, &force, &tools, &workers, &land, config, Some(&animals), None)
        .zip_map(&skill, |p, s| p * s);
    let mut wage: Vec<f64> = (0..n).map(|r| config.money.food_price * per_worker.get(r, farming)).collect();
    let sizes: Vec<f64> = config.villages.iter().map(|v| v.population as f64).collect();
    let gets: Vec<f64> = geo.transport.iter().map(|t| (1.0 - t).clamp(MIN_PRICE_LEVEL, 1.0)).collect();
    if geo.on_map {
        let country: Vec<usize> = (0..n).filter(|&r| !geo.town[r]).collect();
        let pick = |values: &[f64], rows: &[usize]| rows.iter().map(|&r| values[r]).collect::<Vec<f64>>();
        let country_sizes = pick(&sizes, &country);
        let level = average(&pick(&gets, &country), &country_sizes);
        for r in 0..n {
            wage[r] *= gets[r] / level;
        }
        let town_wage = average(&pick(&wage, &country), &country_sizes);
        for r in (0..n).filter(|&r| geo.town[r]) {
            wage[r] = town_wage;
        }
    }
    let mut prices = Grid::zeros(n, np);
    for _ in 0..np {
        // supplies are valued at the prices being worked out
        prices = economy::fair_prices(&wage, &per_worker, &prices, config);
        if has_towns {
            let delivered = towns::delivered_price(&prices.column(food), &geo, &sizes);
            prices.set_column(food, &delivered);
        }
        // where a good can't be made, it costs what it takes to bring it
        let made_here = prices.clone();
        crafts::price_brought_in(&mut prices, &made_here, &Grid::zeros(n, np), &supply);
    }

    // Every family starts with a few months of food and firewood money saved.
    let need_rows = rules::food_need(&population, config);
    let n_hh = households.len();
    let family_need = crate::market::by_household(&population, n_hh, |i| need_rows[i]);
    let members = households::sizes(&population, n_hh);
    let fuel = config.product_for("heating");
    let mean_firewood = config.needs.firewood.iter().sum::<f64>() / config.needs.firewood.len() as f64;
    for h in 0..n_hh {
        let v = households.location[h] as usize;
        let monthly = family_need[h] * prices.get(v, food) + members[h] * mean_firewood * prices.get(v, fuel);
        households.money[h] = config.money.initial_savings_months * monthly;
    }
    // A starting point may have the landholders in debt.
    if config.credit.initial_debt_months > 0.0 {
        crate::credit::start_in_debt(&mut households, &wage, config.credit.initial_debt_months);
    }

    let effort = economy::labor(&population, &force, config, None, false);
    let made =
        economy::capacity(&effort, &tools, &workers, &land, &Grid::new(n, nb, 1.0), config, Some(&animals), None);
    // Food in store as configured; other goods, the stock their trade aims to keep.
    let food_need = rules::by_location(&population, n, |i| need_rows[i]);
    let product_of = economy::product_of(config);
    let mut orders = Grid::zeros(n, np);
    for r in 0..n {
        for (b, &p) in product_of.iter().enumerate() {
            orders.set(r, p, made.get(r, b));
        }
        orders.set(r, food, food_need[r]);
    }
    let mut stock = orders.map(|o| config.trade.stock_target_months * o);
    stock.set_column(food, &vec![0.0; n]);
    let cash = workers.map(|w| config.money.initial_business_cash_per_worker * w);
    let needs = economy::input_needs(config);

    // Families hold the food in store: half of each place's stores spread
    // by families' needs, half by the land they hold.
    let held = households.by_place(n, |h| households.land[h]);
    for h in 0..n_hh {
        let v = households.location[h] as usize;
        let in_store = food_need[v] * config.villages[v].initial_food_months;
        let by_need = div_or(family_need[h], food_need[v], 0.0);
        let by_land = div_or(households.land[h], held[v], by_need);
        households.grain[h] = in_store * (0.5 * by_need + 0.5 * by_land);
    }
    let mut supplies = Grid3::zeros(n, nb, np);
    for r in 0..n {
        for b in 0..nb {
            for p in 0..np {
                *supplies.at(r, b, p) = needs.get(b, p) * made.get(r, b); // a month's worth
            }
        }
    }

    // Big places start with an established council.
    let mut councils = Councils::none(n);
    let cc = &config.council;
    let people = rules::people_by_location(&population, n);
    if cc.enabled && cc.established_at_start {
        councils.formed = people.iter().map(|&p| p >= cc.forms_at_population as f64).collect();
        let official = council::official_job(config);
        council::staff(&mut population, &councils, official, cc.officials_per_1000, config, streams.get("setup"));
        if config.healthcare.enabled {
            let per_1000 = config.healthcare.healers_per_1000;
            council::staff(&mut population, &councils, official + 1, per_1000, config, streams.get("setup"));
        }
        let officials = rules::by_location(&population, n, |i| {
            if population.job[i] == official {
                population.count[i] as f64
            } else {
                0.0
            }
        });
        for v in 0..n {
            if councils.formed[v] {
                councils.treasury[v] = cc.treasury_months * officials[v] * cc.official_pay * wage[v];
                councils.reserve[v] = cc.reserve_months * food_need[v];
                councils.months_ready[v] = cc.forms_after_months;
            }
        }
    }
    // The woods are in balance with the place's use of them at the start.
    let richness: Vec<f64> = (0..n)
        .map(|r| if has_towns && geo.town[r] { geo.woods[r] * config.town.woods } else { geo.woods[r] })
        .collect();
    let (woods_capacity, woods) = environment::size_woods(&sizes, config, Some(&richness));
    // Town grain dealers start with a few months of the town's need, and the
    // coins to buy as much again.
    let dealers_grain: Vec<f64> =
        (0..n).map(|r| if geo.town[r] { config.town.dealers_months * food_need[r] } else { 0.0 }).collect();
    let dealers_cash: Vec<f64> = (0..n).map(|r| dealers_grain[r] * prices.get(r, food)).collect();
    // Each town expects its need to come in, and each village to send its share.
    let arrivals: Vec<f64> = (0..n).map(|r| if geo.town[r] { food_need[r] } else { 0.0 }).collect();
    let mut sends = vec![0.0; n];
    if has_towns {
        let country: Vec<usize> = (0..n).filter(|&r| !geo.town[r] && geo.market[r] >= 0).collect();
        let mut around = vec![0.0; n];
        for &r in &country {
            around[geo.market[r] as usize] += food_need[r];
        }
        for &r in &country {
            let m = geo.market[r] as usize;
            sends[r] = food_need[r] * div_or(arrivals[m], around[m], 0.0);
        }
    }
    // The town pays just enough to carry the village's grain there: the
    // village sells a little grain each month to pay its taxes in coin. With
    // market towns, the world beyond pays just enough to carry a border
    // town's grain away.
    let food_prices = prices.column(food);
    let town_level = if has_towns {
        let any_border = (0..n).any(|r| geo.town[r] && geo.border[r]);
        let border: Vec<usize> = (0..n).filter(|&r| geo.town[r] && (geo.border[r] || !any_border)).collect();
        let p: Vec<f64> = border.iter().map(|&r| food_prices[r]).collect();
        let w: Vec<f64> = border.iter().map(|&r| sizes[r]).collect();
        average(&p, &w) / (1.0 - config.town.outside_transport)
    } else if geo.on_map {
        let p: Vec<f64> = (0..n).map(|r| food_prices[r] / gets[r]).collect();
        average(&p, &sizes)
    } else {
        food_prices.iter().sum::<f64>() / n as f64 / (1.0 - config.town.transport)
    };
    let woods_normal = woods.clone();
    // (a starting point may have them thinned)
    let woods: Vec<f64> =
        (0..n).map(|r| (woods[r] * config.environment.initial_woods).min(woods_capacity[r])).collect();
    population.sort_by_family();
    if config.minds.enabled {
        households.minds = Some(Minds::new(households.len(), &config.minds, streams.get("minds")));
    }
    Ok(World {
        month: config.start_month - 1,
        population,
        land,
        households,
        stock,
        prices,
        markup: Grid::new(n, np, 1.0),
        cash,
        tools,
        supplies,
        pay: Grid::spread(&wage, nb),
        wage_level: wage,
        orders,
        food,
        council: councils,
        names: config.villages.iter().map(|v| v.name.clone()).collect(),
        active_events: Vec::new(),
        council_costs: vec![0.0; n],
        coin_earnings: None,
        seed: None,
        lord: Lords { land: demesne, ..Lords::none(n) },
        town: Town::at(town_level),
        memory: Memory::calm(n),
        sown: vec![1.0; n],
        last_started: LastStarted::new(config.events.len(), n),
        woods,
        woods_capacity,
        woods_normal,
        soil: vec![config.environment.initial_soil; n],
        geo,
        dealers_grain,
        dealers_cash,
        arrivals,
        sends,
        unhealthy,
        refugees: vec![0.0; n],
        skill,
        lacking,
        supply,
    })
}

/// The starting people of one place: ages from a pyramid, sexes alternating
/// down the age order (so every age group is balanced), health and skill
/// drawn.
fn initial_people(population: &mut Population, n: usize, location: u32, config: &Config, rng: &mut Rng) {
    let demo = &config.demography;
    let ages: Vec<i32> = (0..=demo.max_initial_age as i32).collect();
    let weights: Vec<f64> = ages.iter().map(|&a| (-demo.initial_age_decay * a as f64).exp()).collect();
    let cum = cumulative(&weights);
    let years: Vec<i32> = (0..n).map(|_| ages[rng.choose_weighted(&cum)]).collect();
    // Alternate sexes down the age order.
    let jitter: Vec<f64> = (0..n).map(|_| rng.random()).collect();
    let mut by_age: Vec<usize> = (0..n).collect();
    by_age.sort_by(|&a, &b| (years[a] as f64 + jitter[a]).partial_cmp(&(years[b] as f64 + jitter[b])).unwrap());
    let first = rng.integers(0, 2) as usize;
    let mut female = vec![false; n];
    for &i in by_age.iter().skip(first).step_by(2) {
        female[i] = true;
    }
    let (low, high) = config.health.initial;
    for i in 0..n {
        let months = years[i] * 12 + rng.integers(0, 12) as i32;
        let health = rng.uniform(low, high);
        let skill = rules::draw_skill(1, &config.skill, rng);
        population.count.push(1);
        population.age_months.push(months);
        population.female.push(female[i]);
        population.health.push(health);
        population.skill.push(skill);
        population.location.push(location);
        population.household.push(0);
        population.job.push(NO_JOB);
        population.married.push(false);
        population.couple.push(-1);
    }
}
