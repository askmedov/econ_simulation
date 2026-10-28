//! The monthly loop: the same steps, in the same order, every month, as
//! Python's `Simulation.step`.

use std::sync::Arc;

use crate::chronicle::{self, Chronicle};
use crate::config::Config;
use crate::council;
use crate::crafts;
use crate::credit;
use crate::economy::{self, Workforce};
use crate::environment;
use crate::events::{self, EventState, Weather};
use crate::farms;
use crate::finance;
use crate::grid::{average, div_or, Grid};
use crate::healthcare;
use crate::households;
use crate::livestock;
use crate::lords;
use crate::market;
use crate::metrics::{self, MonthRecord};
use crate::migration;
use crate::rng::RandomStreams;
use crate::rules;
use crate::town;
use crate::towns::{self, Carts};
use crate::work;
use crate::world::{create_world, World};

/// Measures kept for each place, month by month (see `Simulation::place_series`):
/// people, born, died, share of food need eaten, grain price, people who
/// left and who came, grain sent to the town, wool coins, the share of
/// their staples (salt) families got (1 where they need none), the coins'
/// worth the trades made, the cost of living (a grown-up's grain, firewood
/// and salt for a month), people in families under 90% of their food, the
/// price of a plot of land, families' wealth a head (see
/// `metrics::family_wealth`), the yearly interest rate, and what its
/// borrowers owe the town's lenders (see `finance`).
pub const PLACE_SERIES: [&str; 17] = [
    "population",
    "births",
    "deaths",
    "ration",
    "food_price",
    "left",
    "arrived",
    "sent",
    "wool",
    "staples_bought",
    "output",
    "cost_of_living",
    "underfed",
    "land_price",
    "wealth",
    "interest_rate",
    "owed_to_lenders",
];

const MONTH_ABBR: [&str; 13] = ["", "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

pub struct Simulation {
    pub config: Arc<Config>,
    pub streams: RandomStreams,
    pub world: World,
    pub records: Vec<MonthRecord>,
    pub log: Vec<String>,                               // notable happenings, in order (as Python's)
    pub chronicle: Chronicle,                           // what happened, ranked by importance
    short_since: Vec<Option<i64>>,                      // place -> month rationing began
    pub places: Vec<Vec<Vec<f64>>>,                     // per series in PLACE_SERIES: month by month, per place
    at_risk: std::collections::VecDeque<rules::AtRisk>, // the last twelve months, for the life table
}

/// What the world beyond pays a unit of a craft at each place (0: it buys
/// none there), and the most it buys there a month.
struct Beyond {
    pays: Vec<f64>,
    most: Vec<f64>,
}

/// Where a market's buyers in families get their coins.
enum FamilyCoins {
    Theirs,           // their own savings
    Budget(Vec<f64>), // a budget set aside for it
}

impl Simulation {
    pub fn new(config: Config) -> Result<Simulation, String> {
        let config = crafts::with_crafts(&config);
        events::validate(&config.events)?;
        events::validate_schedule(&config.scheduled_events, &config.events, config.villages.len(), config.map.enabled)?;
        if !(1..=12).contains(&config.start_month) {
            return Err("start_month must be between 1 and 12".into());
        }
        if config.villages.iter().any(|v| v.population < 1) {
            return Err("every village needs at least one person at the start".into());
        }
        let mut streams = RandomStreams::new(config.seed);
        let world = create_world(&config, &mut streams)?;
        let n = config.villages.len();
        Ok(Simulation {
            config: Arc::new(config),
            streams,
            world,
            records: Vec::new(),
            log: Vec::new(),
            chronicle: Chronicle::new(n),
            short_since: vec![None; n],
            places: vec![Vec::new(); PLACE_SERIES.len()],
            at_risk: std::collections::VecDeque::new(),
        })
    }

    pub fn extinct(&self) -> bool {
        self.world.population.size() == 0
    }

    pub fn run(&mut self) -> &[MonthRecord] {
        for _ in 0..self.config.months {
            self.step();
        }
        &self.records
    }

    /// Each per-place measure as (months x places), by name.
    pub fn place_series(&self) -> Vec<(&'static str, Vec<Vec<f64>>)> {
        PLACE_SERIES.iter().zip(&self.places).map(|(&name, v)| (name, v.clone())).collect()
    }

    pub fn step(&mut self) -> MonthRecord {
        let config = Arc::clone(&self.config);
        let config = &*config;
        let n = self.world.n_locations();
        let has_towns = self.world.geo.has_towns();
        let is_town = self.world.geo.town.clone(); // market towns as places, on a map
        let on_map = self.world.geo.on_map;
        let alive_at_start = !self.extinct();
        let coins_before = self.world.village_money();
        let month_of_year = self.world.month_of_year();
        // the stronghold where the state pays a garrison, if it does
        let garrisoned = (has_towns && config.state.enabled && config.state.garrison > 0.0)
            .then(|| config.villages.iter().position(|v| v.stronghold))
            .flatten();
        let nb = config.businesses.len();

        // 1. Events: end old ones, start scheduled and random ones, combine their effects.
        events::advance(&mut self.world.active_events);
        let month_number = self.records.len() as i64 + 1;
        let scheduled = {
            let w = &mut self.world;
            let mut state = EventState {
                active: &mut w.active_events,
                last_started: &mut w.last_started,
                month: w.month,
                month_of_year,
                n_locations: n,
            };
            events::start_scheduled_events(
                &mut state,
                config,
                &w.geo,
                month_number,
                self.streams.get("scheduled_events"),
            )
        };
        self.note_events(&scheduled, " (scheduled)");
        // (for the chronicle: each event that started, where, and whether forced)
        let begun = |sim: &Simulation, started: &[usize], forced: bool| -> Vec<(usize, usize, bool)> {
            started
                .iter()
                .map(|&i| (sim.world.active_events[i].spec, sim.world.active_events[i].location, forced))
                .collect()
        };
        let mut started_now = begun(self, &scheduled, true);
        let mut hits: Vec<(String, u64)> = Vec::new();
        if config.random_events {
            let started = {
                let w = &mut self.world;
                let mut state = EventState {
                    active: &mut w.active_events,
                    last_started: &mut w.last_started,
                    month: w.month,
                    month_of_year,
                    n_locations: n,
                };
                let (events_rng, weather_rng) = self.streams.pair("events", "weather");
                let weather =
                    on_map.then_some(Weather { geo: &w.geo, scale_km: config.map.weather_km, rng: weather_rng });
                events::start_location_events(&mut state, config, events_rng, weather)
            };
            self.note_events(&started, "");
            started_now.extend(begun(self, &started, false));
            hits = events::apply_person_events(
                &mut self.world.population,
                config,
                month_of_year,
                &config.health,
                self.streams.get("person_events"),
            );
        }
        let names: Vec<String> = config.businesses.iter().map(|b| b.name.clone()).collect();
        let mut mods = events::modifiers(&self.world.active_events, config, n, &names);
        let farm = config.farming();
        let world = &mut self.world;
        let food = world.food;
        // The towns' lenders, if any (see `finance`).
        let finance_on = config.finance.enabled;
        let has_lenders = finance_on && world.lenders.any();
        world.lenders.flows = finance::Flows::default();
        // Debts grow by a month's interest; a ruler may cancel them.
        let interest =
            credit::accrue_interest(&mut world.households, &world.interest, has_lenders.then_some(&mut world.lenders));
        let cancelled =
            credit::cancel_debts(&mut world.households, &mods.debt_cancel, has_lenders.then_some(&mut world.lenders));
        // Soldiers or raiders take grain and animals.
        let mut farm_grain = world.farm_grain();
        let (requisitioned, _taken_from_lord) =
            lords::requisition(&mut world.households, &mut world.lord, &mut farm_grain, &mods.requisition);
        // The region's grain price swings; a ruler may debase the coinage;
        // some coins are lost or buried.
        town::drift(&mut world.town, config, self.streams.get("town"));
        let debase = mods.debase.iter().cloned().fold(0.0, f64::max);
        if debase > 0.0 {
            world.town.level /= 1.0 - debase;
        }
        town::lose_coins(&mut world.households.money, &mut world.town, config);
        // Fire or pests strike stored grain: families' stores and the farms'.
        let mut lost = vec![0.0; n];
        for h in 0..world.households.len() {
            let v = world.households.location[h] as usize;
            let gone = world.households.grain[h] * mods.granary_loss[v];
            world.households.grain[h] -= gone;
            lost[v] += gone;
        }
        for v in 0..n {
            lost[v] += farm_grain[v] * mods.granary_loss[v];
            farm_grain[v] *= 1.0 - mods.granary_loss[v];
        }
        world.set_farm_grain(&farm_grain);
        // Herds grow, or die off in bad weather; each November they are
        // thinned to what can be fed through winter, the meat into stores.
        let farm_weather_now = mods.production_mult.column(farm);
        let animals_lost =
            livestock::grow_and_die(&mut world.households, &farm_weather_now, &mods.heating_mult, config);
        let (mut culled, mut meat) = (vec![0.0; n], vec![0.0; n]);
        if month_of_year == 11 {
            (culled, meat) =
                livestock::winter_cull(&mut world.households, &world.land, config, Some(&world.geo.pasture));
        }
        // Families with no food and no coins slaughter their animals.
        let n_hh_start = world.households.len();
        let need_now = rules::food_need(&world.population, config);
        let hungry_need = market::by_household(&world.population, n_hh_start, |i| need_now[i]);
        let last_price = world.food_price(); // (last month's)
        let (killed, eaten_animals) =
            livestock::slaughter_in_hunger(&mut world.households, &hungry_need, &last_price, config);
        for v in 0..n {
            culled[v] += killed[v];
            meat[v] += eaten_animals[v];
        }

        // Wood takes longer to cut and gather where the woods have thinned.
        let seller = economy::seller_of(config);
        let fuel = config.product_for("heating");
        let wood = seller[fuel].expect("a trade that cuts firewood");
        let reach = environment::woods_reach(&world.woods, &world.woods_normal, config);
        for v in 0..n {
            *mods.production_mult.at(v, wood) *= reach[v];
        }
        // And so does the work of the trades that burn their own: the
        // ironworks' charcoal, the salt pans' and the kilns' fires (in a
        // town, from the woods of its countryside too).
        let burns: Vec<f64> = config.businesses.iter().map(|b| b.woods).collect();
        if burns.iter().any(|&w| w > 0.0) {
            let around = crafts::woods_around(&world.woods, &world.geo);
            let normal = crafts::woods_around(&world.woods_normal, &world.geo);
            let reach = environment::woods_reach(&around, &normal, config);
            for v in 0..n {
                for (b, _) in burns.iter().enumerate().filter(|&(_, &w)| w > 0.0) {
                    *mods.production_mult.at(v, b) *= reach[v];
                }
            }
        }

        // 2. Businesses make goods, using up supplies and wearing out tools.
        // Other than farms, they work less when unsold goods pile up. Farms
        // grow more with plough animals, and only as much as was sown.
        let product_of = economy::product_of(config);
        let force: Workforce = economy::workforce(&world.population, n, config); // nobody joins, leaves or changes trade until wages are paid
        let workers = economy::headcount(&world.population, n, config, Some(&force));
        let mut effort = economy::labor(&world.population, &force, config, Some(self.streams.get("production")), false);
        // At harvest everyone helps in the fields: the trades lose part of
        // their days, and children and the old glean, bind and carry.
        let n_hh = world.households.len();
        let mut helping = work::harvest_help(&world.population, n, n_hh, month_of_year, config, &force);
        // With harvest hands, townsfolk help in the fields of the villages around.
        let hands = (has_towns && config.harvest_hands.enabled)
            .then(|| work::harvest_hands(&helping.households, &world.households, &world.land, &world.geo, config));
        if has_towns {
            // a town has no fields of its own
            for v in (0..n).filter(|&v| is_town[v]) {
                helping.farm[v] = 0.0;
            }
            for h in 0..n_hh {
                if is_town[world.households.location[h] as usize] {
                    helping.households[h] = 0.0;
                }
            }
        }
        for v in 0..n {
            for b in 0..nb {
                if !(has_towns && is_town[v]) {
                    *effort.at(v, b) *= helping.kept[b];
                } else if hands.as_ref().is_some_and(|h| h.hands[v] > 0.0) {
                    // the town's craftsmen out in the fields lose those days
                    *effort.at(v, b) *= 1.0 - config.harvest_hands.town_share * (1.0 - helping.kept[b]);
                }
            }
            if let Some(hands) = &hands {
                helping.farm[v] += hands.visiting[v];
            }
            *effort.at(v, farm) += helping.farm[v];
        }
        let herd = world.households.by_place(n, |h| world.households.animals[h]);
        let animal_factor = livestock::farm_factor(&herd, &world.land, config);
        let farm_mult: Vec<f64> = (0..n).map(|v| animal_factor[v] * world.soil[v]).collect(); // plough animals, and how fertile the soil is
        let tool_factor = economy::tool_factor(&world.tools, &workers, config);
        let farm_output: Vec<f64> = (0..n).map(|v| config.businesses[farm].output * tool_factor.get(v, farm)).collect();
        let per_plot: Vec<f64> = (0..n).map(|v| 12.0 * farm_output[v] * farm_mult[v]).collect();
        let farmed = economy::land_in_use(&world.land, &workers.column(farm), config, &per_plot);
        let making = if has_towns {
            mods.production_mult.zip_map(&world.skill, |m, s| m * s)
        } else {
            mods.production_mult.clone()
        }; // town craftsmen
        let mut possible = economy::capacity(
            &effort,
            &world.tools,
            &workers,
            &world.land,
            &making,
            config,
            Some(&farm_mult),
            Some(&world.sown),
        );
        let season = rules::season_factors(config);
        for v in 0..n {
            *possible.at(v, farm) *= season[(month_of_year - 1) as usize];
        }
        let mut target = world.orders.take_columns(&product_of).map(|o| config.trade.stock_target_months * o);
        // Woodcutters also stock up ahead of winter.
        let heating = &config.needs.firewood;
        let mean_heating = heating.iter().sum::<f64>() / heating.len() as f64;
        let people_here = rules::people_by_location(&world.population, n);
        let usual_heating: Vec<f64> = (0..n).map(|v| people_here[v] * mean_heating * mods.heating_mult[v]).collect();
        let buffer = market::seasonal_buffer(heating);
        for v in 0..n {
            *target.at(v, wood) += buffer[(month_of_year - 1) as usize] * usual_heating[v];
        }
        let mut pace = Grid::zeros(n, nb);
        for v in 0..n {
            for b in 0..nb {
                let piled_up = div_or(world.stock.get(v, product_of[b]), target.get(v, b), 0.0);
                pace.set(v, b, if b == farm { 1.0 } else { (2.0 - piled_up).clamp(0.0, 1.0) });
            }
        }
        let made = economy::limit_by_supplies(&possible.zip_map(&pace, |p, q| p * q), &mut world.supplies, config);
        for v in 0..n {
            for (b, &p) in product_of.iter().enumerate() {
                *world.stock.at(v, p) += made.get(v, b);
            }
        }
        world.tools.data.iter_mut().for_each(|t| *t *= 1.0 - config.trade.tool_wear);
        let produced = made.column(farm);

        // Prices: what each product fairly costs to make in a normal year
        // (a poor sowing or harvest shows in the grain mark-up), times its mark-up.
        let mut per_worker = economy::productivity(
            &world.population,
            &force,
            &world.tools,
            &workers,
            &world.land,
            config,
            Some(&farm_mult),
            None,
        );
        if has_towns {
            per_worker = per_worker.zip_map(&world.skill, |p, s| p * s);
        }
        let mut fair = economy::fair_prices(&world.wage_level, &per_worker, &world.prices, config);
        if has_towns {
            // A town's grain costs what it takes to bring it in from where it
            // is cheapest: its villages, at their prices, or a neighbouring town.
            let marked: Vec<f64> = (0..n).map(|v| fair.get(v, food) * world.markup.get(v, food)).collect();
            let delivered = towns::delivered_price(&marked, &world.geo, &people_here);
            let neighbours = towns::from_neighbours(&world.food_price(), &world.geo);
            for v in (0..n).filter(|&v| is_town[v]) {
                fair.set(v, food, delivered[v].min(neighbours[v]));
            }
        }
        // where a good can't be made, it costs what it takes to bring it
        crafts::price_brought_in(&mut fair, &world.prices, &world.stock, &world.supply);
        world.prices = fair.zip_map(&world.markup, |f, m| f * m);

        // 3. How well the place's stores and coming harvests cover the year
        // (this sets the grain price and says whether it is a famine year).
        let need_rows = rules::food_need(&world.population, config);
        let need = rules::by_location(&world.population, n, |i| need_rows[i]);
        let full_strength = economy::labor(&world.population, &force, config, None, true);
        let no_events = Grid::new(n, nb, 1.0);
        let normal_gross = economy::capacity(
            &full_strength,
            &world.tools,
            &workers,
            &world.land,
            &no_events,
            config,
            Some(&farm_mult),
            None,
        )
        .column(farm);
        let months_ahead = (config.food.planning_months - 1).max(0) as usize;
        let mut ahead = events::production_outlook(&world.active_events, config, n, months_ahead, &names[farm]);
        let sown_ahead = farms::sown_outlook(&world.sown, month_of_year, months_ahead, config);
        ahead = ahead.zip_map(&sown_ahead, |a, s| a * s);
        if config.work.enabled {
            // harvests gathered with everyone's help
            let harvest = work::harvest_help(&world.population, n, n_hh, config.work.harvest_months[0], config, &force);
            let mut at_harvest = harvest.farm.clone();
            if hands.is_some() {
                let coming =
                    work::harvest_hands(&harvest.households, &world.households, &world.land, &world.geo, config);
                for v in 0..n {
                    at_harvest[v] += coming.visiting[v];
                }
            }
            let boost = work::harvest_boost(&full_strength.column(farm), &at_harvest, config);
            ahead = ahead.zip_map(&work::outlook_boost(&boost, month_of_year, months_ahead, config), |a, b| a * b);
        }
        // Food to come: the harvests expected, less the seed picked from them,
        // plus what merchants bring from the town (or less what they carry off).
        let harvests = rules::harvest_outlook(&normal_gross, &ahead, month_of_year, config);
        let seed_needed = farms::seed_needed(&farmed, config);
        let seed_ahead = farms::seed_outlook(&seed_needed, month_of_year, months_ahead, config);
        let mut outlook = harvests.zip_map(&seed_ahead, |h, s| h - s.min(config.food.max_seed_share * h));
        // On a map the town's price follows the whole region's harvest.
        let mut farm_weather = mods.production_mult.column(farm);
        if on_map {
            let weights: Vec<f64> = need.iter().map(|x| x.max(1e-9)).collect();
            farm_weather = vec![average(&farm_weather, &weights); n];
        }
        let town_price = town::price(&world.town, month_of_year, &farm_weather, config); // beyond the region, on a map
        let mut cart: Option<Carts> = None;
        let (exports, imports) = if has_towns {
            // Merchants carry goods between each village and its market town;
            // a town expects the grain that has been coming in.
            let mut wanted = world.orders.clone();
            wanted.set_column(food, &need);
            let mut in_store = world.stock.clone();
            in_store.set_column(food, &world.dealers_grain);
            let mut c =
                towns::carts(&world.prices, &world.geo, &wanted, &in_store, &world.dealers_cash, &need, food, config);
            for p in (0..world.supply.len()).filter(|&p| world.supply[p].is_some()) {
                for q in 0..c.src.len() {
                    c.goods.set(q, p, 0.0); // (sold across the region from the makers' stock instead)
                }
            }
            for v in 0..n {
                for m in 0..outlook.cols {
                    *outlook.at(v, m) += world.arrivals[v];
                }
            }
            let ex = c.grain_out.clone();
            let im = c.grain_in.clone();
            cart = Some(c);
            (ex, im)
        } else {
            let m = town::merchants(&world.food_price(), &town_price, &need, config, &world.geo.transport);
            (m.exports, m.imports)
        };
        for v in 0..n {
            for m in 0..outlook.cols {
                let o = outlook.at(v, m);
                *o = (*o + imports[v] - exports[v]).max(0.0);
            }
        }
        // This month's seed is about to be picked from the harvest: not food.
        let mut farm_grain = world.farm_grain();
        if world.seed.is_none() {
            // the first month: seed already picked this season
            let gathered = farms::seed_gathered(month_of_year, config);
            world.seed = Some(seed_needed.iter().map(|s| s * gathered).collect());
        }
        let mut seed = world.seed.take().expect("set");
        let seed_now = farms::seed_due(&farm_grain, &seed, &seed_needed, &produced, month_of_year, config);
        let granary = world.granary();
        let stores: Vec<f64> = (0..n).map(|v| granary[v] - seed_now[v]).collect();
        let supply_cover = rules::plan_ration_realistically(&stores, &need, &outlook, config, 3, 2.0);
        let famine: Vec<bool> = supply_cover.iter().map(|&c| c < 1.0).collect();

        // The harvest is shared out in kind. The council's levy comes off the
        // top, then (at harvest time) next year's seed; the farms keep enough
        // to sell for new tools; the rest goes to the owners of the plough
        // animals, the families who worked the fields and the families who
        // hold them.
        let mut levied = council::levy_grain(&mut world.council, &produced, &mut farm_grain, &need, &famine, config);
        let seed_kept = farms::keep_seed(&mut farm_grain, &mut seed, &seed_needed, &produced, month_of_year, config);
        let tool = config.product_for("tool");
        let food_price = world.food_price();
        let keep_for_tools: Vec<f64> = (0..n)
            .map(|v| {
                div_or(config.trade.tool_wear * workers.get(v, farm) * world.prices.get(v, tool), food_price[v], 0.0)
            })
            .collect();
        let owners = livestock::owners_part(&animal_factor);
        let (shares, to_lord, visitors) = farms::income_shares(
            &world.population,
            &world.households,
            &world.land,
            config,
            Some(&owners),
            Some(&world.lord.land),
            Some(&helping.households),
            &force,
            hands.as_ref().map(|h| h.visiting.as_slice()),
        );
        let (harvest_share, rent, to_hands) = farms::share_harvest(
            &mut world.households,
            &mut farm_grain,
            &keep_for_tools,
            &shares,
            Some(&to_lord),
            hands.as_ref().map(|_| visitors.as_slice()),
        );
        if let Some(hands) = &hands {
            // The hands carry their pay home: each town's, by what each family gave.
            let mut earned_by_town = vec![0.0; n];
            for v in 0..n {
                if to_hands[v] > 0.0 {
                    earned_by_town[world.geo.market[v] as usize] += to_hands[v];
                }
            }
            for h in 0..n_hh {
                if hands.away[h] > 0.0 {
                    let t = world.households.location[h] as usize;
                    world.households.grain[h] += earned_by_town[t] * hands.away[h] / hands.hands[t];
                }
            }
        }
        world.set_farm_grain(&farm_grain);
        let shared_out = market::by_village(&harvest_share, &world.households.location, n);
        let labour_grain: Vec<f64> = (0..n)
            .map(|v| {
                let service = if world.lord.land[v] > 0.0 { config.lord.labour_service } else { 0.0 };
                config.food.labor_share * (1.0 - owners[v]) * (1.0 - service) * (shared_out[v] + rent[v])
            })
            .collect();
        // The lord's share goes to his barn; part of it is carted away to his hall.
        lords::into_barn(&mut world.lord, &rent);
        let mut carted_away = lords::cart_away(&mut world.lord, config);
        if has_towns {
            // part of it to his market town, where the dealers buy it
            let to_town: Vec<f64> = carted_away.iter().map(|c| config.lord.to_town * c).collect();
            let mut promised = vec![0.0; n]; // (for the villages' grain they are carting in)
            let town_of = world.geo.town_of();
            for v in 0..n {
                carted_away[v] -= to_town[v];
                promised[town_of[v]] += exports[v] * food_price[v];
            }
            let sold_in_town = lords::sell_in_town(
                &mut world.lord,
                &to_town,
                &world.geo.market,
                &mut world.dealers_grain,
                &mut world.dealers_cash,
                &food_price,
                Some(&promised),
            );
            for v in 0..n {
                carted_away[v] += sold_in_town[v];
            }
        }

        // 4. What families need this month. They eat from their own store as
        // far as it lasts until their next harvest, offer what they can
        // spare, and need coins for the rest of their food and for firewood.
        let pop = &world.population;
        let members = market::by_household(pop, n_hh, |i| pop.count[i] as f64);
        let family_food = market::by_household(pop, n_hh, |i| need_rows[i]);
        let after_levy: Vec<f64> =
            (0..n).map(|v| 1.0 - if world.council.formed[v] { config.council.grain_levy } else { 0.0 }).collect();
        let mut plan = farms::plan_family_food(
            &world.households,
            &family_food,
            &outlook,
            &shares,
            &after_levy,
            config,
            Some(&world.markup.column(food)),
        );
        // Kin with grain to spare help families who can't afford their food.
        let hh_loc = world.households.location.clone();
        let short_of: Vec<f64> = (0..n_hh)
            .map(|h| plan.want[h] - div_or(world.households.money[h], food_price[hh_loc[h] as usize], 0.0))
            .collect();
        let alive: Vec<bool> = members.iter().map(|&m| m > 0.0).collect();
        let from_kin = work::kin_help(&mut world.households, &alive, &short_of, &plan.spare, config);
        // It goes to this month's food; the givers have that much less to sell.
        for h in 0..n_hh {
            let got = from_kin[h].max(0.0);
            plan.own[h] += got;
            plan.want[h] = (plan.want[h] - got).max(0.0);
            plan.spare[h] = (plan.spare[h] + from_kin[h].min(0.0)).max(0.0);
        }
        let firewood_need = heating[(month_of_year - 1) as usize];
        let family_fuel: Vec<f64> =
            (0..n_hh).map(|h| members[h] * firewood_need * mods.heating_mult[hh_loc[h] as usize]).collect();
        let price_at = |h: usize, p: usize| world.prices.get(hh_loc[h] as usize, p);
        let essentials: Vec<f64> =
            (0..n_hh).map(|h| plan.want[h] * price_at(h, food) + family_fuel[h] * price_at(h, fuel)).collect();
        // Families keep coins for a few months of firewood and of whatever food
        // their own store won't cover, before they spend on comforts, lend, or
        // buy animals and land.
        let decide = world.households.decide();
        let coin_target: Vec<f64> = (0..n_hh)
            .map(|h| {
                let months = decide.savings_months(h, config.needs.savings_months);
                let uncovered = (months * family_food[h] - world.households.grain[h]).max(0.0);
                uncovered * price_at(h, food) + months * family_fuel[h] * price_at(h, fuel)
            })
            .collect();
        let lord_offer = lords::offer(&world.lord, config);
        let mut brought = imports.clone(); // grain merchants offer: from the town, or in a town its dealers'
        let mut offer = world.stock.clone();
        if let Some(c) = &cart {
            // Grain goes out of the dealers' store to the villages; the rest is
            // on offer in the town; goods go out of the town's businesses.
            let outgoing = c.outgoing(n);
            for v in 0..n {
                world.dealers_grain[v] -= c.grain_held[v];
                for p in 0..outgoing.cols {
                    *world.stock.at(v, p) -= outgoing.get(v, p); // (no grain: the food column is the farms')
                }
                if is_town[v] {
                    brought[v] = world.dealers_grain[v];
                    world.dealers_grain[v] = 0.0;
                }
            }
            let incoming = c.incoming(n);
            offer = world.stock.zip_map(&incoming, |s, i| s + i);
        }
        let spare_by_place = market::by_village(&plan.spare, &hh_loc, n);
        for v in 0..n {
            offer.set(v, food, spare_by_place[v] + farm_grain[v] + lord_offer[v] + brought[v]);
        }
        let stock_before = world.stock.clone();
        let shared = market::share_with_neighbours(&mut world.households.money, &essentials, &hh_loc, n, config);
        // The council helps families who still can't afford them.
        let keep: Vec<f64> = world.council_costs.iter().map(|c| 2.0 * c).collect();
        let short: Vec<f64> = (0..n_hh).map(|h| (essentials[h] - world.households.money[h]).max(0.0)).collect();
        let cash_relief =
            council::cash_relief(&mut world.council, &mut world.households.money, &short, &hh_loc, &keep, config);
        // Families still short sell animals to families with coins to spare;
        // when many must sell at once, the price collapses.
        let mut animal_worth: Vec<f64> = world.wage_level.iter().map(|w| config.livestock.value_months * w).collect();
        let short: Vec<f64> = (0..n_hh).map(|h| (essentials[h] - world.households.money[h]).max(0.0)).collect();
        let spare: Vec<f64> = (0..n_hh).map(|h| world.households.money[h] - coin_target[h]).collect();
        let animal_sales = livestock::distress_sales(&mut world.households, &short, &spare, &animal_worth, config);
        // Then they borrow against their land and animals (and a little on
        // their word); failing that, they sell land. Land is worth some years
        // of its rent at the usual grain price.
        let markup_food = world.markup.column(food);
        let rent_per_plot: Vec<f64> = (0..n)
            .map(|v| {
                let normal_food_price = food_price[v] / markup_food[v].max(1e-9);
                let yearly_net = (12.0 * normal_gross[v] - seed_needed[v]).max(0.0);
                let land_part = (1.0 - config.food.labor_share) * (1.0 - owners[v]);
                div_or(land_part * yearly_net, world.land[v], 0.0) * normal_food_price
            })
            .collect();
        let plot_price = credit::land_price(&rent_per_plot, config);
        // Only food is worth debt or land: firewood can be gathered.
        let food_cost: Vec<f64> = (0..n_hh).map(|h| plan.want[h] * food_price[hh_loc[h] as usize]).collect();
        let mut borrowed = vec![0.0; n];
        if config.credit.enabled {
            let short: Vec<f64> = (0..n_hh).map(|h| (food_cost[h] - world.households.money[h]).max(0.0)).collect();
            let spare: Vec<f64> = (0..n_hh).map(|h| world.households.money[h] - coin_target[h]).collect();
            let worth = credit::collateral(&world.households, &plot_price, &animal_worth);
            let personal: Vec<f64> =
                (0..n_hh).map(|h| config.credit.personal_months * world.wage_level[hh_loc[h] as usize]).collect();
            if finance_on {
                // Neighbours lend more of their spare coins at a high rate, the
                // town's lenders what they can't; the rate follows the borrowing
                // asked against the coins on offer.
                let willing = finance::willing(&world.interest, config);
                let borrowing = credit::borrow_from(
                    &mut world.households,
                    &short,
                    &spare,
                    &worth,
                    &personal,
                    n,
                    config,
                    Some(&willing),
                    has_lenders.then_some(&mut world.lenders),
                );
                let could_lend = world.lenders.could_lend(&market::by_village(&members, &hh_loc, n), config);
                let offer: Vec<f64> = (0..n).map(|v| borrowing.offered[v] + could_lend[v]).collect();
                world.lenders.move_rates(&mut world.interest, &borrowing.asked, &offer, config);
                borrowed = borrowing.lent;
            } else {
                borrowed = credit::borrow(&mut world.households, &short, &spare, &worth, &personal, n, config);
            }
        }
        let short: Vec<f64> = (0..n_hh).map(|h| (food_cost[h] - world.households.money[h]).max(0.0)).collect();
        let spare: Vec<f64> = (0..n_hh).map(|h| world.households.money[h] - coin_target[h]).collect();
        let land_sales = credit::sell_land(&mut world.households, &short, &spare, &plot_price, config);

        // 5. Markets, most needed first. Businesses buy supplies and tools
        // with part of their cash, before paying wages.
        let np = config.products.len();
        let mut demand = Grid::zeros(n, np);
        let mut sold = Grid::zeros(n, np);
        let mut bought: Vec<Option<Vec<f64>>> = vec![None; np];

        // Grain: families short of it buy from the farms' store, merchants'
        // grain from the town, and families and the lord with grain to spare,
        // all at the place's price. Then merchants buy what is left, if the
        // place is cheap enough to carry grain to the town.
        let offer_food = offer.column(food);
        let sale = market::buy(&mut world.households.money, &plan.want, &food_price, &offer_food, &hh_loc);
        demand.set_column(food, &sale.demand);
        bought[food] = Some(sale.bought.clone());
        // (merchants never bring grain and carry it off in the same month)
        let exported: Vec<f64> = (0..n).map(|v| exports[v].min((offer_food[v] - sale.sold[v]).max(0.0))).collect();
        let sold_food: Vec<f64> = (0..n).map(|v| sale.sold[v] + exported[v]).collect();
        let sellers =
            farms::sellers_share(&sold_food, &plan.spare, &farm_grain, &hh_loc, Some(&lord_offer), Some(&brought));
        sold.set_column(food, &sold_food);
        let sold_by_lord = sellers.lord.clone();
        lords::sold(&mut world.lord, &sold_by_lord, &food_price);
        let grain_receipts: Vec<f64> =
            (0..n_hh).map(|h| sellers.families[h] * food_price[hh_loc[h] as usize]).collect();
        for v in 0..n {
            *world.cash.at(v, farm) += sellers.farms[v] * food_price[v];
            farm_grain[v] -= sellers.farms[v];
        }
        world.set_farm_grain(&farm_grain);
        for h in 0..n_hh {
            world.households.grain[h] -= plan.own[h] + sellers.families[h];
        }
        let imported = sellers.imports.clone();
        let mut beyond = vec![0.0; n];
        let mut between = vec![0.0; n];
        if has_towns {
            // The dealers are paid for the grain they sold, in the town and in
            // its villages, and take back what was not sold; they pay for the
            // grain carted in from the villages, which they sell from next
            // month. A border town's dealers trade with the world beyond.
            let to = world.geo.town_of();
            for v in 0..n {
                world.dealers_cash[to[v]] += (imported[v] - exported[v]) * food_price[v];
                world.dealers_grain[to[v]] += brought[v] - imported[v] + exported[v];
            }
            between = towns::between_towns(
                &mut world.dealers_grain,
                &mut world.dealers_cash,
                &food_price,
                &need,
                &world.geo,
                config,
            );
            beyond = towns::border_trade(
                &mut world.dealers_grain,
                &mut world.dealers_cash,
                &food_price,
                &town_price,
                &need,
                &world.geo,
                config,
            );
            world.town.purse += towns::outside_coins(&beyond, &town_price, config);
            let mut came_in = between.clone();
            for v in 0..n {
                if !is_town[v] {
                    came_in[to[v]] += exported[v] - imported[v];
                }
            }
            let weight = towns::MEMORY;
            for v in 0..n {
                let arriving = if is_town[v] { came_in[v] + beyond[v] } else { 0.0 };
                world.arrivals[v] = (1.0 - weight) * world.arrivals[v] + weight * arriving;
                let sending = if is_town[v] { 0.0 } else { exported[v] };
                world.sends[v] = (1.0 - weight) * world.sends[v] + weight * sending;
            }
        } else {
            world.town.purse += (0..n).map(|v| (imported[v] - exported[v]) * food_price[v]).sum::<f64>();
        }

        let homespun = work::home_cloth(&world.population, n_hh, month_of_year, config);
        let input_needs = economy::input_needs(config);
        // What the world beyond pays a unit of each craft it buys at the
        // border towns, and the most it buys at each: so much for every
        // 1,000 of the region's people, shared among them.
        let mut crafts_sold = 0.0;
        let borders: Vec<usize> = (0..n).filter(|&v| world.geo.town[v] && world.geo.border[v]).collect();
        let thousands_here = people_here.iter().sum::<f64>() / 1000.0;
        let pays_beyond: Vec<Option<Beyond>> = (0..config.products.len())
            .map(|p| {
                let &(_, rations, most) = config.crafts.exports.iter().find(|e| e.0 == config.products[p].name)?;
                (has_towns && config.crafts.enabled && !borders.is_empty()).then(|| {
                    let mut beyond = Beyond { pays: vec![0.0; n], most: vec![0.0; n] };
                    for &v in &borders {
                        beyond.pays[v] = rations * town_price[v] * (1.0 - config.town.outside_transport);
                        beyond.most[v] = most * thousands_here / borders.len() as f64;
                    }
                    beyond
                })
            })
            .collect();
        let cloth = config.product_for("comfort");
        let (mut staples_needed, mut staples_bought) = (0.0, 0.0);
        // (and per place, for the viewer: only read, never used by a rule)
        let (mut staples_needed_here, mut staples_bought_here) = (vec![0.0; n], vec![0.0; n]);
        // (the staples first: a family short of coins buys its salt, and
        // gathers its fuel on the commons)
        let staples_first = (0..config.products.len())
            .filter(|&p| config.products[p].use_ == "staple")
            .chain((0..config.products.len()).filter(|&p| config.products[p].use_ != "staple"));
        for product in staples_first {
            let spec = &config.products[product];
            let business_want = business_want(world, config, &workers, &possible, &input_needs, product);
            match spec.use_.as_str() {
                "food" => continue,
                "heating" => {
                    bought[product] = trade(
                        world,
                        config,
                        cart.as_ref(),
                        &offer,
                        &mut demand,
                        &mut sold,
                        product,
                        Some((&family_fuel, FamilyCoins::Theirs)),
                        business_want,
                        None,
                        pays_beyond[product].as_ref(),
                        &mut crafts_sold,
                        &possible,
                    );
                }
                "comfort" => {
                    // (each comfort good its share of what families spend on them)
                    let share = crafts::comfort_share(config, product);
                    let mut spend = vec![0.0; n_hh];
                    let mut want = vec![0.0; n_hh];
                    for h in 0..n_hh {
                        let spare = (world.households.money[h] - coin_target[h]).max(0.0);
                        let price = world.prices.get(hh_loc[h] as usize, product);
                        // Families wear their homespun first and buy less cloth.
                        let home = if product == cloth { homespun[h] * price } else { 0.0 };
                        spend[h] = (share * config.needs.spare_spending * spare - home).max(0.0);
                        want[h] = if spend[h] > 0.0 && price > 0.0 { spend[h] / price } else { 0.0 };
                    }
                    // The lord's household buys the place's cloth and pots too.
                    let lord_spend: Vec<f64> =
                        lords::local_spending(&world.lord, config, has_towns).iter().map(|s| share * s).collect();
                    bought[product] = trade(
                        world,
                        config,
                        cart.as_ref(),
                        &offer,
                        &mut demand,
                        &mut sold,
                        product,
                        Some((&want, FamilyCoins::Budget(spend))),
                        business_want,
                        Some(lord_spend),
                        pays_beyond[product].as_ref(),
                        &mut crafts_sold,
                        &possible,
                    );
                }
                "staple" => {
                    // Every family needs a little each month, bought with its coins.
                    let need = crafts::staple_need(config, product);
                    let want: Vec<f64> = members.iter().map(|m| need * m).collect();
                    staples_needed += want.iter().sum::<f64>();
                    let got = trade(
                        world,
                        config,
                        cart.as_ref(),
                        &offer,
                        &mut demand,
                        &mut sold,
                        product,
                        Some((&want, FamilyCoins::Theirs)),
                        business_want,
                        None,
                        pays_beyond[product].as_ref(),
                        &mut crafts_sold,
                        &possible,
                    );
                    staples_bought += got.as_ref().map_or(0.0, |g| g.iter().sum::<f64>());
                    for (h, &wanted) in want.iter().enumerate() {
                        let v = hh_loc[h] as usize;
                        staples_needed_here[v] += wanted;
                        staples_bought_here[v] += got.as_ref().map_or(0.0, |g| g[h]);
                    }
                }
                _ => {
                    trade(
                        world,
                        config,
                        cart.as_ref(),
                        &offer,
                        &mut demand,
                        &mut sold,
                        product,
                        None,
                        business_want,
                        None,
                        pays_beyond[product].as_ref(),
                        &mut crafts_sold,
                        &possible,
                    );
                }
            }
        }
        let bought_food = bought[food].clone().expect("grain market");
        let bought_fuel = bought[fuel].clone().unwrap_or_else(|| vec![0.0; n_hh]);

        // The council gives food from its reserve to families who couldn't eat enough.
        let had: Vec<f64> = (0..n_hh).map(|h| plan.own[h] + bought_food[h]).collect();
        let council_relief = council::give_relief(&mut world.council, &had, &family_food, &hh_loc, config);
        // Families still hungry find famine foods: roots, greens, nuts, fish
        // (more in summer and autumn; fewer in a drought).
        let summer = if (5..=10).contains(&month_of_year) { 4.0 / 3.0 } else { 2.0 / 3.0 };
        let can_find: Vec<f64> = (0..n)
            .map(|v| config.food.foraging * summer * mods.production_mult.get(v, farm).min(1.0) * world.geo.forage[v])
            .collect();
        let gap: Vec<f64> = (0..n_hh).map(|h| (family_food[h] - had[h] - council_relief[h]).max(0.0)).collect();
        let lord_relief = lords::charity(&mut world.lord, &gap, &hh_loc, &famine, config);
        let mut foraged = vec![0.0; n_hh];
        let mut family_eaten = vec![0.0; n_hh];
        let mut family_share = vec![1.0; n_hh];
        for h in 0..n_hh {
            let fed = had[h] + council_relief[h] + lord_relief[h];
            foraged[h] = (family_food[h] - fed).clamp(0.0, (family_food[h] * can_find[hh_loc[h] as usize]).max(0.0));
            family_eaten[h] = fed + foraged[h];
            family_share[h] = div_or(family_eaten[h], family_food[h], 1.0);
        }
        // Families with minds remember how they ate and what grain cost them.
        if let Some(minds) = &mut world.households.minds {
            let paid: Vec<f64> = (0..n_hh).map(|h| food_price[hh_loc[h] as usize]).collect();
            minds.remember_month(&family_share, &paid, &hh_loc, &members);
        }
        // Families who couldn't buy all their firewood gather some from the
        // commons: dung, straw and furze, and wood unless the woods have
        // burned or thinned.
        let other = config.needs.other_fuels;
        let gathered: Vec<f64> = (0..n_hh)
            .map(|h| {
                let wood_mult = mods.production_mult.get(hh_loc[h] as usize, wood);
                let can_gather = config.needs.gathering * family_fuel[h] * (other + (1.0 - other) * wood_mult);
                (family_fuel[h] - bought_fuel[h]).clamp(0.0, can_gather.max(0.0))
            })
            .collect();
        // The woods regrow, less what was cut, gathered and burned by the
        // trades, and what burned in fires; the soil tires on crowded land
        // and recovers on land left to rest.
        let gathered_wood =
            market::by_village(&gathered.iter().map(|g| g * (1.0 - other)).collect::<Vec<_>>(), &hh_loc, n);
        let burned: Vec<f64> = (0..n).map(|v| (0..nb).map(|b| made.get(v, b) * burns[b]).sum()).collect();
        let burned = crafts::spread_burned(&burned, &world.woods, &world.geo);
        let cut: Vec<f64> = (0..n).map(|v| made.get(v, wood) + gathered_wood[v] + burned[v]).collect();
        let woods_burned =
            environment::grow_woods(&mut world.woods, &world.woods_capacity, &cut, &mods.woods_burned, config);
        let usual_land = world.geo.usual_land(&world.land);
        environment::tire_soil(&mut world.soil, &people_here, &usual_land, config);
        let warmth: Vec<f64> = (0..n_hh).map(|h| div_or(bought_fuel[h] + gathered[h], family_fuel[h], 1.0)).collect();
        let eaten = market::by_village(&family_eaten, &hh_loc, n);
        let share: Vec<f64> = (0..n).map(|v| div_or(eaten[v], need[v], 1.0)).collect();

        // 6. Businesses pay out their takings as wages. Keep a running average
        // of what each trade pays (or could pay, if nobody works in it).
        let unit_supplies = economy::supplies_cost(&world.prices, config);
        let mut keep_back = Grid::zeros(n, nb);
        for v in 0..n {
            for (b, spec) in config.businesses.iter().enumerate() {
                let tools_cost = if spec.tool_boost > 0.0 {
                    config.trade.tool_wear * workers.get(v, b) * world.prices.get(v, tool)
                } else {
                    0.0
                };
                keep_back.set(v, b, unit_supplies.get(v, b) * possible.get(v, b) + tools_cost);
            }
        }
        let (mut income, payout) =
            market::pay_wages(&mut world.cash, &world.population, n_hh, config, Some(&keep_back), &force);
        // Farm workers are paid mostly in grain: count its value too.
        let mut earned = payout.clone();
        for v in 0..n {
            *earned.at(v, farm) += labour_grain[v] * food_price[v];
        }
        let hoped = economy::potential_pay(&world.prices, &per_worker, config);
        let weight = config.trade.pay_memory;
        for i in 0..world.pay.data.len() {
            let w = workers.data[i];
            world.pay.data[i] = if w < 1.0 {
                hoped.data[i]
            } else {
                (1.0 - weight) * world.pay.data[i] + weight * div_or(earned.data[i], w, 0.0)
            };
        }
        let all_workers = workers.row_sums();
        let average_pay: Vec<f64> = (0..n)
            .map(|v| {
                let total: f64 = (0..nb).map(|b| world.pay.get(v, b) * workers.get(v, b)).sum();
                div_or(total, all_workers[v], 0.0)
            })
            .collect();

        // The council taxes wages and grain sales (while its treasury is below
        // target), and pays its officials and healers.
        let cc = &config.council;
        let official = council::official_job(config);
        let healer = healthcare::healer_job(config);
        let pop = &world.population;
        let officials = rules::by_location(pop, n, |i| if pop.job[i] == official { pop.count[i] as f64 } else { 0.0 });
        let healers = rules::by_location(pop, n, |i| if pop.job[i] == healer { pop.count[i] as f64 } else { 0.0 });
        let official_pay: Vec<f64> = average_pay.iter().map(|p| cc.official_pay * p).collect();
        let healer_pay: Vec<f64> = average_pay.iter().map(|p| config.healthcare.healer_pay * p).collect();
        let running_costs: Vec<f64> =
            (0..n).map(|v| officials[v] * official_pay[v] + healers[v] * healer_pay[v]).collect();
        for h in 0..n_hh {
            income[h] += grain_receipts[h];
        }
        if has_towns {
            // the grain dealers' earnings go to the town's families
            let dealt = towns::dealers_payout(&mut world.dealers_cash, &need, &food_price, config);
            let townsfolk = market::by_village(&members, &hh_loc, n);
            for h in 0..n_hh {
                let v = hh_loc[h] as usize;
                income[h] += div_or(dealt[v] * members[h], townsfolk[v], 0.0);
            }
            let servants = lords::pay_servants(&mut world.lord, &world.geo.market, &hh_loc, &members, config); // and the lords' wages
            for h in 0..n_hh {
                income[h] += servants[h];
            }
            // and what the towns' lenders spend of their profits there
            if has_lenders {
                let (paid, _) = world.lenders.spend(&hh_loc, &members, config);
                for h in 0..n_hh {
                    income[h] += paid[h];
                }
            }
            // and the state's pay to its garrison and officials in the stronghold
            if let Some(stronghold) = garrisoned {
                if world.lord.garrison_pay.is_none() {
                    let due =
                        lords::tax_due(&world.households, &world.wage_level, &rent_per_plot, config, Some(&alive));
                    world.lord.garrison_pay = Some(config.state.garrison * due / 12.0);
                }
                let pay = lords::pay_garrison(&mut world.lord, stronghold, &hh_loc, &members);
                for h in 0..n_hh {
                    income[h] += pay[h];
                }
            }
        }
        let mut wool = vec![0.0; n_hh];
        if has_towns && month_of_year == config.livestock.shearing_month {
            // Shearing: the wool goes beyond the region, the coins come in.
            let wool_worth: Vec<f64> = (0..n)
                .map(|v| {
                    config.livestock.wool * town_price[v] * (1.0 - world.geo.to_border[v]).max(0.0) * mods.wool_price[v]
                })
                .collect();
            wool = livestock::shear(&world.households, &wool_worth);
            for h in 0..n_hh {
                income[h] += wool[h];
            }
            world.town.purse -= wool.iter().sum::<f64>();
            // Python names the wool's worth as it named the animals' worth,
            // so foreclosure and the credit limit below value animals at it
            // in a shearing month: kept so, for parity (see PLAN.md, "Rust
            // engine").
            animal_worth = wool_worth;
        }
        let wool_by_place = market::by_village(&wool, &hh_loc, n);
        let tax_target: Vec<f64> = running_costs.iter().map(|c| cc.treasury_months * c).collect();
        let taxes = council::collect_taxes(&mut income, &hh_loc, &mut world.council, &tax_target, &famine, config);
        for h in 0..n_hh {
            world.households.money[h] += income[h];
        }
        let (staff_income, _) =
            council::pay_staff(&world.population, &mut world.council, official, &official_pay, n_hh);
        let (healer_income, _) = council::pay_staff(&world.population, &mut world.council, healer, &healer_pay, n_hh);
        for h in 0..n_hh {
            world.households.money[h] += staff_income[h];
        }
        for h in 0..n_hh {
            world.households.money[h] += healer_income[h];
        }
        world.council_costs = running_costs;
        // Borrowers pay part of their income toward their debts; debts that
        // outgrow a family's animals and land cost them those.
        let all_income: Vec<f64> = (0..n_hh).map(|h| income[h] + staff_income[h] + healer_income[h]).collect();
        let spare: Vec<f64> = (0..n_hh).map(|h| world.households.money[h] - coin_target[h]).collect();
        let repaid = credit::repay(
            &mut world.households,
            &all_income,
            n,
            config,
            Some(&spare),
            has_lenders.then_some(&mut world.lenders),
        );
        let repaid_in_grain = credit::repay_in_grain(
            &mut world.households,
            &family_food,
            &food_price,
            config,
            has_lenders.then_some(&world.lenders),
        );
        let foreclosed = credit::foreclose(
            &mut world.households,
            &plot_price,
            &animal_worth,
            config,
            has_lenders.then_some(&world.lenders),
        );
        // Once a year, after harvest, the state collects its tax in coin (or
        // seizes grain from families without the coins).
        let mut state_tax = lords::TaxTake { coins: vec![0.0; n], grain: vec![0.0; n] };
        if month_of_year == config.state.collection_month {
            state_tax = lords::collect_state_tax(
                &mut world.households,
                &mut world.lord,
                &world.wage_level,
                &rent_per_plot,
                &food_price,
                &famine,
                config,
                Some(&alive),
                Some(&coin_target),
            );
            if garrisoned.is_some() {
                world.lord.garrison_pay = Some(config.state.garrison * state_tax.coins.iter().sum::<f64>() / 12.0);
            }
        }
        let worth = credit::collateral(&world.households, &plot_price, &animal_worth);
        let limit: Vec<f64> = (0..n_hh)
            .map(|h| {
                config.credit.loan_to_value * worth[h]
                    + config.credit.personal_months * world.wage_level[hh_loc[h] as usize]
            })
            .collect();
        let owed = world.households.minds.is_some().then(|| world.households.lent.clone());
        let written_off =
            credit::default(&mut world.households, &limit, n, config, has_lenders.then_some(&mut world.lenders));
        if let (Some(owed), Some(minds)) = (&owed, &mut world.households.minds) {
            minds.remember_losses(owed, &world.households.lent);
        }
        // The customary wage that fair prices are reckoned in rises slowly
        // while families hold more money than `money_months` of their coin
        // earnings (averaged over a year), and falls while they hold less:
        // prices follow the money there is.
        let this_month = market::by_village(&all_income, &hh_loc, n);
        let previous = world.coin_earnings.take().unwrap_or_else(|| this_month.clone());
        let earnings: Vec<f64> = (0..n).map(|v| (1.0 - 1.0 / 12.0) * previous[v] + this_month[v] / 12.0).collect();
        let held = market::by_village(&world.households.money, &hh_loc, n);
        for v in 0..n {
            let usual_months =
                if has_towns && is_town[v] { config.town.money_months } else { config.trade.money_months };
            let months_held = div_or(held[v], usual_months * earnings[v], 1.0);
            let months_held = if earnings[v] > 0.0 { months_held } else { 1.0 };
            world.wage_level[v] *= 1.0 + config.trade.wage_adjustment * (months_held - 1.0).clamp(-1.0, 1.0);
        }
        world.coin_earnings = Some(earnings.clone());

        // 7. Some stored food spoils: in families' stores, the farms' and the council's reserve.
        let spoilage = config.food.spoilage;
        let spoiled_families = rules::spoil(&mut world.households.grain, spoilage);
        let mut spoiled = market::by_village(&spoiled_families, &hh_loc, n);
        let mut farm_grain = world.farm_grain();
        let farm_spoiled = rules::spoil(&mut farm_grain, spoilage);
        world.set_farm_grain(&farm_grain);
        let dealers_spoiled = rules::spoil(&mut world.dealers_grain, spoilage);
        for v in 0..n {
            spoiled[v] += farm_spoiled[v] + dealers_spoiled[v];
        }
        rules::spoil(&mut world.council.reserve, spoilage);
        rules::spoil(&mut world.lord.barn, spoilage);
        // Sowing, after the month's harvest is in.
        let mut seed_from_families = vec![0.0; n];
        if month_of_year == farms::sowing_month(config) {
            let (sown, given) = farms::sow(&mut world.households, &mut seed, &seed_needed, &family_food);
            world.sown = sown;
            seed_from_families = given;
        }
        world.seed = Some(seed);

        // 8. Health follows each family's food and warmth, plus events.
        let coldness = firewood_need / heating.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let cold: Vec<f64> = warmth.iter().map(|w| config.needs.cold_damage * coldness * (1.0 - w)).collect();
        {
            let pop = &mut world.population;
            let household = pop.household.clone();
            let location = pop.location.clone();
            rules::update_health(
                pop,
                |i| family_share[household[i] as usize],
                |i| mods.health_delta[location[i] as usize] - cold[household[i] as usize],
                config,
            );
        }

        // Healers see the people most likely to die first.
        let mortality: Vec<f64> = if has_towns {
            // a town crowded with famine refugees has its fevers too
            let crowded = |v: usize| {
                if config.refugees.enabled && is_town[v] {
                    1.0 + config.refugees.fevers * div_or(world.refugees[v], people_here[v], 0.0)
                } else {
                    1.0
                }
            };
            (0..n).map(|v| mods.mortality_mult[v] * world.unhealthy[v] * crowded(v)).collect()
        // towns die faster
        } else {
            mods.mortality_mult.clone()
        };
        let risk = rules::death_chance(&world.population, &mortality, config, None);
        let healers_at_work: Vec<f64> =
            (0..n).map(|v| if world.council.formed[v] { healers[v] } else { 0.0 }).collect();
        let (care, treated) = healthcare::treat(&mut world.population, &healers_at_work, &risk, config);

        // 9. Deaths, births, ageing. Businesses with more orders than workers
        // take people on; those with too many let some go to trades that are
        // short. Farms aim to grow a little more than the place eats.
        let mut at_risk = rules::AtRisk::default();
        let died = rules::deaths(
            &mut world.population,
            &mortality,
            config,
            self.streams.get("deaths"),
            n,
            Some(&care),
            &mut at_risk,
        );
        let cover = rules::wage_cover(&average_pay, &food_price, &need, &all_workers);
        // How well a family can live here: by its wage, or by what the land
        // yields per person after seed and the lord's share (a place thinned
        // by famine has land to spare, and marries and draws people).
        let prospects: Vec<f64> = (0..n)
            .map(|v| {
                let lord_part = div_or(world.lord.land[v], world.land[v], 0.0);
                let land_cover = div_or(
                    (normal_gross[v] - seed_needed[v] / 12.0) * (1.0 - (1.0 - config.food.labor_share) * lord_part),
                    need[v],
                    0.0,
                );
                cover[v].max(land_cover)
            })
            .collect();
        let willing = rules::marriage_factor(&prospects, config);
        let born = rules::births(&mut world.population, &mods.fertility_mult, config, self.streams.get("births"), n);
        rules::grow_older(&mut world.population);
        let people = rules::people_by_location(&world.population, n);
        let formed_now = council::check_formation(&mut world.council, &people, config);
        council::staff(
            &mut world.population,
            &world.council,
            official,
            cc.officials_per_1000,
            config,
            self.streams.get("council"),
        );
        let per_1000 = if config.healthcare.enabled { config.healthcare.healers_per_1000 } else { 0.0 };
        council::staff(&mut world.population, &world.council, healer, per_1000, config, self.streams.get("council"));
        // Firewood orders count families' need at its yearly average, not
        // this month's, so woodcutters work steadily all year.
        let weight = config.trade.orders_memory;
        let families_heating = market::by_village(&family_fuel, &hh_loc, n);
        let mut steady = demand.clone();
        if firewood_need > 0.0 {
            for v in 0..n {
                *steady.at(v, fuel) += families_heating[v] * (mean_heating / firewood_need - 1.0);
            }
        }
        world.orders = world.orders.zip_map(&steady, |o, s| (1.0 - weight) * o + weight * s);
        let mut orders = world.orders.take_columns(&product_of);
        for v in 0..n {
            let wanted = if has_towns {
                // villages grow the grain they send to town; towns grow none
                if is_town[v] {
                    0.0
                } else {
                    (need[v] + world.sends[v]) * (1.0 + config.food.reserve_margin)
                }
            } else {
                need[v] * (1.0 + config.food.reserve_margin)
            };
            orders.set(v, farm, wanted);
        }
        let mut work_needed = Grid::zeros(n, nb);
        for v in 0..n {
            for (b, &p) in product_of.iter().enumerate() {
                let pull = world.markup.get(v, p).powf(config.trade.hiring_price_response);
                let lacks = world.lacking[v][b]; // (what can't be made here is brought in)
                work_needed.set(
                    v,
                    b,
                    if lacks { 0.0 } else { orders.get(v, b) / per_worker.get(v, b).max(1e-9) * pull },
                );
            }
        }
        let current = economy::headcount(&world.population, n, config, None);
        let wanted = economy::wanted_workers(&work_needed, &current, config);
        let closed: Option<Vec<Vec<bool>>> = has_towns.then(|| {
            // a town has no fields to work, and no place works a resource it lacks
            (0..n).map(|v| (0..nb).map(|b| (is_town[v] && b == farm) || world.lacking[v][b]).collect()).collect()
        });
        economy::assign_new_workers(&mut world.population, &wanted, config, closed.as_deref());
        let switched = economy::move_workers(&mut world.population, &wanted, config, self.streams.get("jobs"));
        // Measure families before weddings move young people into new homes.
        let pop = &world.population;
        let underfed_here = rules::by_location(pop, n, |i| {
            if family_share[pop.household[i] as usize] < 0.9 {
                pop.count[i] as f64
            } else {
                0.0
            }
        });
        let underfed = underfed_here.iter().sum::<f64>() as i64;
        let (mut landless_need, mut landless_eaten) = (0.0, 0.0);
        for h in 0..n_hh {
            if world.households.land[h] <= 0.0 {
                landless_need += family_food[h];
                landless_eaten += family_eaten[h];
            }
        }
        let landless_ration = if landless_need > 0.0 { landless_eaten / landless_need } else { 1.0 };
        let wealth: Vec<f64> = (0..n_hh)
            .map(|h| world.households.money[h] + world.households.grain[h] * food_price[hh_loc[h] as usize])
            .collect();
        let family_sizes = households::sizes(pop, n_hh);
        let poorest = metrics::poorest_fifth_ration(&wealth, &family_sizes, &family_eaten, &family_food);
        // People leave a place, or come to it, by how good a place it is to
        // live: what a living there is worth, what people remember of its
        // hunger and dangers, its size, the land it has to spare and its
        // lord's take.
        let people = rules::people_by_location(pop, n);
        migration::remember(
            &mut world.memory,
            &share,
            &died,
            &people,
            &mods.requisition,
            config,
            Some(&mods.mortality_mult),
        );
        // (a town is not judged by its farmland)
        let mut land_seen = world.geo.usual_land(&world.land);
        // (nor by its lord: its rents, tolls and dues count as an ordinary take)
        let mut take_seen = to_lord.clone();
        if has_towns {
            for v in (0..n).filter(|&v| is_town[v]) {
                land_seen[v] = people[v] / config.environment.people_per_plot;
                take_seen[v] = config.migration.usual_take;
            }
        }
        let place = migration::appeal(&prospects, &world.memory, &people, &land_seen, &take_seen, config);
        let mut moves = migration::move_people(
            &mut world.population,
            &mut world.households,
            &place,
            &world.memory,
            &family_share,
            config,
            self.streams.get("migration"),
            &world.geo,
        );
        world.town.purse += moves.coins;
        if config.refugees.enabled {
            let fade = 1.0 / config.refugees.memory_months.max(1.0);
            for v in 0..n {
                world.refugees[v] = world.refugees[v] * (1.0 - fade) + moves.refugees[v];
            }
        }
        let room: Vec<f64> = usual_land.iter().map(|l| l * config.environment.people_per_plot).collect(); // people the land feeds at a usual density
        let arrived = migration::arrive(
            &mut world.population,
            &world.households,
            &place,
            &room,
            config,
            self.streams.get("migration"),
            &world.geo,
        );
        for v in 0..n {
            moves.arrived[v] += arrived[v];
        }
        let people_now = rules::people_by_location(&world.population, n);
        migration::remember_leaving(&mut world.memory, &moves.left, &people_now);
        let families_before = world.households.len();
        let weddings = households::marry(
            &mut world.population,
            &mut world.households,
            config,
            self.streams.get("marriage"),
            Some(&willing),
        );
        if let Some(minds) = &mut world.households.minds {
            // New families have their parents' minds, a little changed.
            let rng = self.streams.get("minds");
            for founded in families_before..minds.len() {
                minds.vary(founded, rng);
            }
        }
        let size = households::sizes(&world.population, world.households.len());
        let gone: Vec<bool> = size.iter().map(|&s| s == 0.0).collect();
        let owed = world.households.minds.is_some().then(|| world.households.lent.clone());
        credit::write_off(&mut world.households, &gone, n, has_lenders.then_some(&mut world.lenders));
        if let (Some(owed), Some(minds)) = (&owed, &mut world.households.minds) {
            minds.remember_losses(owed, &world.households.lent);
        }
        households::pass_on_savings(&mut world.households, &size, &mut world.council.treasury, &world.council.formed);
        let to_reserve = households::pass_on_land_and_grain(
            &mut world.households,
            &size,
            &mut world.council.reserve,
            &world.council.formed,
        );
        for v in 0..n {
            levied[v] += to_reserve[v];
        }
        world.population.remove_empty(); // rows emptied by deaths and departures, once a month
        if month_of_year == 12 {
            // Births and weddings put people out of family order: once a year, back in it.
            world.population.sort_by_family();
        }

        // 10. Mark-ups move with the gap between demand and supply: for most
        // goods, what was asked for against what was made this month plus any
        // stock piled up beyond the target.
        let target_months = config.trade.stock_target_months;
        let target_by_product = world.orders.map(|o| target_months * o);
        let mut supply = Grid::zeros(n, np);
        for v in 0..n {
            for p in 0..np {
                let excess = (stock_before.get(v, p) - target_by_product.get(v, p)).max(0.0) / target_months;
                supply.set(v, p, excess);
            }
            for (b, &p) in product_of.iter().enumerate() {
                *supply.at(v, p) += made.get(v, b);
            }
            // Firewood: steady orders against what was cut plus the stock beyond
            // the usual stock and next season's buffer (or short of them).
            let ahead = target_by_product.get(v, fuel) + buffer[(month_of_year % 12) as usize] * usual_heating[v];
            let made_fuel = made.get(v, wood);
            supply.set(v, fuel, (made_fuel + (world.stock.get(v, fuel) - ahead) / 12.0).max(0.0));
        }
        let food_markup = world.markup.column(food);
        world.markup = market::adjust_markup(&world.markup, &steady, &supply, config);
        // (a good a place only brings in is priced where it is made, and
        // carried at its price plus the way: no mark-up of its own)
        for (v, row) in world.lacking.iter().enumerate() {
            for (b, _) in row.iter().enumerate().filter(|&(_, &lacks)| lacks) {
                world.markup.set(v, product_of[b], 1.0);
            }
        }
        // Grain follows how short the year's supply looks (King-Davenant).
        world.markup.set_column(food, &market::grain_markup(&food_markup, &supply_cover, config));
        if has_towns {
            // a town's by its dealers' store
            let by_store = towns::store_markup(&food_markup, &world.dealers_grain, &need, config);
            for v in (0..n).filter(|&v| is_town[v]) {
                world.markup.set(v, food, by_store[v]);
            }
        }

        // 11. Record the month.
        let pop = &world.population;
        let (children, working, elderly) = metrics::age_groups(pop, config);
        let jobs = economy::headcount(pop, n, config, None).column_sums();
        let need_total: f64 = need.iter().sum();
        let need_weights: Vec<f64> = need.iter().map(|x| x.max(1e-9)).collect();
        let sum = |v: &[f64]| v.iter().sum::<f64>();
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        let towns_here: Vec<usize> = (0..n).filter(|&v| is_town[v]).collect();
        let town_need: f64 = towns_here.iter().map(|&v| need[v]).sum();
        let town_eaten: f64 = towns_here.iter().map(|&v| eaten[v]).sum();
        let final_people = rules::people_by_location(pop, n);
        let food_price_now = world.food_price();
        let workers_total = workers.sum();
        let comfort = config.product_for("comfort");
        let bought_comfort = bought[comfort].clone().unwrap_or_else(|| vec![0.0; n_hh]);
        // Economic and welfare measures: only read, never used by a rule.
        let output_here: Vec<f64> =
            (0..n).map(|v| (0..nb).map(|b| made.get(v, b) * world.prices.get(v, product_of[b])).sum()).collect();
        let living_here = metrics::cost_of_living(&world.prices, config);
        let living = average(&living_here, &final_people.iter().map(|p| p.max(1e-9)).collect::<Vec<_>>());
        let wage_now = if workers_total > 0.0 { earned.sum() / workers_total } else { 0.0 };
        let net_worth = metrics::family_wealth(world, &plot_price, config);
        let wealth_here = {
            let total = market::by_village(&net_worth, &world.households.location, n);
            (0..n).map(|v| div_or(total[v], final_people[v], 0.0)).collect::<Vec<f64>>()
        };
        let (wealth_gini, top_tenth_wealth) = {
            let size = households::sizes(&world.population, world.households.len());
            let head: Vec<f64> = (0..size.len()).map(|h| div_or(net_worth[h], size[h], 0.0)).collect();
            metrics::inequality(&head, &size)
        };
        self.at_risk.push_back(at_risk);
        if self.at_risk.len() > 12 {
            self.at_risk.pop_front();
        }
        let (life_expectancy, under_five_deaths) = {
            let mut window = rules::AtRisk::default();
            for month in &self.at_risk {
                for age in 0..=rules::OLDEST {
                    window.people[age] += month.people[age];
                    window.dying[age] += month.dying[age];
                }
            }
            let usual = rules::MortalityTable::new(config);
            metrics::life_table(&window.people, &window.dying, |age| usual.rate(age as i32))
        };
        let record = MonthRecord {
            month_number,
            year: world.year(),
            month: month_of_year,
            population: pop.size() as i64,
            children,
            workers: working,
            elderly,
            households: size.iter().filter(|&&s| s > 0.0).count() as i64,
            weddings: weddings as i64,
            births: sum(&born) as i64,
            deaths: sum(&died) as i64,
            food_produced: sum(&produced),
            food_needed: need_total,
            food_eaten: sum(&eaten),
            own_food: sum(&plan.own),
            foraged: sum(&foraged),
            grain_sold: sum(&sale.sold),
            ration: if need_total > 0.0 { sum(&eaten) / need_total } else { 1.0 },
            food_spoiled: sum(&spoiled),
            food_lost: sum(&lost),
            food_levied: sum(&levied),
            food_to_seed: sum(&seed_kept) + sum(&seed_from_families),
            meat: sum(&meat),
            seed_store: world.seed.as_ref().map_or(0.0, |s| sum(s)),
            sown: mean(&world.sown),
            animals: sum(&world.households.animals),
            animals_lost: sum(&animals_lost) + sum(&culled),
            animals_sold: sum(&animal_sales.sold),
            animal_price: mean(&animal_sales.price),
            debt: sum(&world.households.debt),
            debtors: (0..world.households.len()).filter(|&h| world.households.debt[h] > 0.01 && size[h] > 0.0).count()
                as i64,
            interest: sum(&interest),
            borrowed: sum(&borrowed),
            repaid: sum(&repaid),
            repaid_in_grain: sum(&repaid_in_grain),
            debts_cancelled: sum(&cancelled),
            debts_written_off: sum(&written_off),
            land_sold: sum(&land_sales.plots),
            land_foreclosed: sum(&foreclosed.land),
            animals_foreclosed: sum(&foreclosed.animals),
            land_price: mean(&plot_price),
            food_rent: sum(&rent),
            lord_carted: sum(&carted_away),
            lord_sold: sum(&sold_by_lord),
            lord_relief: sum(&lord_relief),
            lord_barn: sum(&world.lord.barn),
            lord_purse: sum(&world.lord.purse),
            state_tax: sum(&state_tax.coins),
            tax_grain: sum(&state_tax.grain),
            state_purse: sum(&world.lord.state_purse),
            food_requisitioned: sum(&requisitioned),
            town_price: mean(&town_price),
            grain_exported: sum(&exported),
            grain_imported: (0..n).filter(|&v| !is_town[v]).map(|v| imported[v]).sum(),
            town_purse: world.town.purse,
            town_people: towns_here.iter().map(|&v| people[v]).sum::<f64>() as i64,
            town_ration: if town_need > 0.0 { town_eaten / town_need } else { 1.0 },
            market_price: if towns_here.is_empty() {
                0.0
            } else {
                let p: Vec<f64> = towns_here.iter().map(|&v| food_price_now[v]).collect();
                let w: Vec<f64> = towns_here.iter().map(|&v| need[v].max(1e-9)).collect();
                average(&p, &w)
            },
            dealers_grain: sum(&world.dealers_grain),
            dealers_cash: sum(&world.dealers_cash),
            grain_beyond: sum(&beyond),
            grain_between_towns: between.iter().map(|b| b.max(0.0)).sum(),
            wool_sold: sum(&wool_by_place),
            staples_bought: div_or(staples_bought, staples_needed, 1.0),
            crafts_sold,
            coins_lost: world.town.coins_lost,
            emigrants: sum(&moves.left) as i64,
            immigrants: sum(&moves.arrived) as i64,
            families_left: moves.families as i64,
            moved_within: moves.within as i64,
            appeal: (0..n).map(|v| place[v] * people[v]).sum::<f64>() / sum(&people).max(1e-9),
            empty_villages: final_people.iter().filter(|&&p| p == 0.0).count() as i64,
            food_emigrated: moves.grain,
            food_stock: sum(&world.granary()),
            food_margin: if need_total > 0.0 {
                (0..n).map(|v| normal_gross[v] - seed_needed[v] / 12.0).sum::<f64>() / need_total
            } else {
                0.0
            },
            food_cover: average(&supply_cover, &need_weights),
            wage_cover: average(&cover, &need_weights),
            food_price: average(&food_price_now, &need_weights),
            wage: wage_now,
            savings: sum(&world.households.money),
            money_months: if sum(&earnings) > 0.0 { sum(&held) / sum(&earnings) } else { 0.0 },
            business_cash: world.cash.sum(),
            treasury: sum(&world.council.treasury),
            councils: world.council.formed.iter().filter(|&&f| f).count() as i64,
            officials: sum(&officials).round() as i64,
            taxes: sum(&taxes),
            food_reserve: sum(&world.council.reserve),
            relief: sum(&council_relief),
            healers: sum(&healers).round() as i64,
            treated: sum(&treated).round() as i64,
            cash_relief: sum(&cash_relief),
            shared: sum(&shared),
            underfed,
            landless: (0..world.households.len())
                .filter(|&h| world.households.land[h] <= 0.0 && size[h] > 0.0)
                .map(|h| size[h])
                .sum::<f64>() as i64,
            landless_ration,
            poorest_fifth_ration: poorest,
            warmth: {
                let needed = sum(&family_fuel);
                if needed > 0.0 {
                    (sum(&bought_fuel) + sum(&gathered)) / needed
                } else {
                    1.0
                }
            },
            clothing: (sum(&bought_comfort) + sum(&homespun)) / (pop.size().max(1) as f64),
            homespun: sum(&homespun),
            harvest_help: sum(&helping.farm),
            kin_help: from_kin.iter().map(|k| k.max(0.0)).sum(),
            woods: sum(&world.woods) / sum(&world.woods_normal).max(1e-9),
            woods_burned: sum(&woods_burned),
            soil: (0..n).map(|v| world.soil[v] * world.land[v]).sum::<f64>() / sum(&world.land).max(1e-9),
            job_changes: switched as i64,
            prices: config
                .products
                .iter()
                .enumerate()
                .map(|(p, spec)| (spec.name.clone(), mean(&world.prices.column(p))))
                .collect(),
            jobs: config
                .businesses
                .iter()
                .enumerate()
                .map(|(b, spec)| (spec.name.clone(), jobs[b].round() as i64))
                .collect(),
            avg_health: metrics::average_health(pop),
            poor_health: metrics::poor_health(pop, config),
            accidents: hits.iter().map(|(_, k)| *k as i64).sum(),
            events: String::new(),
            output: sum(&output_here),
            cost_of_living: living,
            real_wage: div_or(wage_now, living, 0.0),
            coins_held: world.village_money(),
            coins_gained: world.village_money() - coins_before,
            family_income: sum(&all_income),
            wealth_gini,
            top_tenth_wealth,
            wealth: div_or(net_worth.iter().sum(), sum(&final_people), 0.0),
            life_expectancy,
            under_five_deaths,
            interest_rate: average(&world.interest, &final_people.iter().map(|p| p.max(1e-9)).collect::<Vec<_>>()),
            lenders_coins: sum(&world.lenders.cash),
            lenders_claims: sum(&world.lenders.claims),
            lenders_lent: world.lenders.flows.lent,
            lenders_repaid: world.lenders.flows.repaid,
            lenders_lost: world.lenders.flows.lost,
            lenders_spent: world.lenders.flows.spent,
        };
        let staples_share: Vec<f64> =
            (0..n).map(|v| div_or(staples_bought_here[v], staples_needed_here[v], 1.0)).collect();
        let series: [&[f64]; 17] = [
            &final_people,
            &born,
            &died,
            &share,
            &food_price_now,
            &moves.left,
            &moves.arrived,
            &exported,
            &wool_by_place,
            &staples_share,
            &output_here,
            &living_here,
            &underfed_here,
            &plot_price,
            &wealth_here,
            &world.interest,
            &world.lenders.claims,
        ];
        for (k, values) in series.iter().enumerate() {
            self.places[k].push(values.to_vec());
        }
        let mut record = record;
        record.events = self.active_event_names();
        self.track_shortages(&share);
        for location in (0..n).filter(|&v| formed_now[v]) {
            self.note(
                Some(location),
                &format!(
                    "The village has formed a council: {:.0}% tax, officials, a food reserve",
                    100.0 * config.council.tax_rate
                ),
            );
        }
        if alive_at_start && self.extinct() {
            self.note(None, "The last villager has died");
        }
        let councils: Vec<usize> = (0..n).filter(|&v| formed_now[v]).collect();
        let month = chronicle::Month {
            record: &record,
            people: &final_people,
            deaths: &died,
            ration: &share,
            left: &moves.left,
            started: &started_now,
            councils: &councils,
        };
        self.chronicle.month(&month, config, &self.world.geo);
        self.records.push(record.clone());
        self.world.month += 1;
        record
    }

    /// Note each event that started; on a map, one line for each kind.
    fn note_events(&mut self, started: &[usize], suffix: &str) {
        let started: Vec<(usize, usize)> =
            started.iter().map(|&i| (self.world.active_events[i].spec, self.world.active_events[i].location)).collect();
        if !self.world.geo.on_map {
            for (spec, location) in started {
                let message = format!("{}{suffix}", self.config.events[spec].message);
                self.note(Some(location), &message);
            }
            return;
        }
        let n = self.world.n_locations();
        let mut kinds: Vec<usize> = Vec::new();
        for &(spec, _) in &started {
            if !kinds.contains(&spec) {
                kinds.push(spec);
            }
        }
        for spec in kinds {
            let where_: Vec<usize> = started.iter().filter(|(s, _)| *s == spec).map(|&(_, l)| l).collect();
            let message = format!("{}{suffix}", self.config.events[spec].message);
            if where_.len() == 1 {
                self.note(Some(where_[0]), &message);
            } else if where_.len() <= 3 {
                let places: Vec<&str> = where_.iter().map(|&l| self.world.names[l].as_str()).collect();
                let line = format!("{message} ({})", places.join(", "));
                self.note(None, &line);
            } else {
                self.note(None, &format!("{message} (in {} of {n} villages)", where_.len()));
            }
        }
    }

    fn note(&mut self, location: Option<usize>, message: &str) {
        let world = &self.world;
        let place = match location {
            Some(l) if world.n_locations() > 1 => format!("{}: ", world.names[l]),
            _ => String::new(),
        };
        self.log.push(format!(
            "Year {}, {}: {place}{message}",
            world.year(),
            MONTH_ABBR[world.month_of_year() as usize]
        ));
    }

    fn track_shortages(&mut self, share: &[f64]) {
        for (location, &fed) in share.iter().enumerate() {
            match self.short_since[location] {
                None if fed < 0.97 => {
                    self.short_since[location] = Some(self.world.month);
                    self.note(
                        Some(location),
                        &format!("Food short: the village ate {:.0}% of what it needs", 100.0 * fed),
                    );
                }
                Some(started) if fed >= 0.99 => {
                    let months = self.world.month - started;
                    let s = if months != 1 { "s" } else { "" };
                    self.note(Some(location), &format!("Enough food again after {months} month{s}"));
                    self.short_since[location] = None;
                }
                _ => {}
            }
        }
    }

    fn active_event_names(&self) -> String {
        let world = &self.world;
        if world.geo.on_map {
            // one entry for each kind, with how many places it is in
            let mut counts: Vec<(usize, Vec<usize>)> = Vec::new();
            for event in &world.active_events {
                match counts.iter_mut().find(|(s, _)| *s == event.spec) {
                    Some((_, places)) => places.push(event.location),
                    None => counts.push((event.spec, vec![event.location])),
                }
            }
            return counts
                .iter()
                .map(|(spec, places)| {
                    let name = &self.config.events[*spec].name;
                    if places.len() == 1 {
                        format!("{}:{name}", world.names[places[0]])
                    } else {
                        format!("{name} ({} villages)", places.len())
                    }
                })
                .collect::<Vec<_>>()
                .join("+");
        }
        world
            .active_events
            .iter()
            .map(|event| {
                let prefix =
                    if world.n_locations() > 1 { format!("{}:", world.names[event.location]) } else { String::new() };
                format!("{prefix}{}", self.config.events[event.spec].name)
            })
            .collect::<Vec<_>>()
            .join("+")
    }
}

/// What the businesses want to buy of `product`: tools to replace worn ones
/// (and close any other gap over a few months), or the supplies their work
/// needs; None if none of them use it.
fn business_want(
    world: &World,
    config: &Config,
    workers: &Grid,
    possible: &Grid,
    input_needs: &Grid,
    product: usize,
) -> Option<Grid> {
    let n = workers.rows;
    let nb = workers.cols;
    if config.products[product].use_ == "tool" {
        let mut wanted = Grid::zeros(n, nb);
        for v in 0..n {
            for (b, spec) in config.businesses.iter().enumerate() {
                if spec.tool_boost > 0.0 {
                    let gap = (workers.get(v, b) - world.tools.get(v, b)).max(0.0);
                    let w =
                        gap.min(config.trade.tool_wear * workers.get(v, b) + gap / config.trade.stock_target_months);
                    wanted.set(v, b, w);
                }
            }
        }
        return Some(wanted);
    }
    if (0..nb).any(|b| input_needs.get(b, product) != 0.0) {
        let mut wanted = Grid::zeros(n, nb);
        for v in 0..n {
            for b in 0..nb {
                let w = (possible.get(v, b) * input_needs.get(b, product) - world.supplies.get(v, b, product)).max(0.0);
                wanted.set(v, b, w);
            }
        }
        return Some(wanted);
    }
    None
}

/// One market in `product` (not grain): families (with `family`'s wants and
/// coins), the lord's household (`lord_spend` coins, in its market town on a
/// map) and the businesses (`business_want`, from part of their cash) buy
/// from what is on `offer` at the place's price. Returns what families bought.
#[allow(clippy::too_many_arguments)]
fn trade(
    world: &mut World,
    config: &Config,
    cart: Option<&Carts>,
    offer: &Grid,
    demand: &mut Grid,
    sold: &mut Grid,
    product: usize,
    family: Option<(&[f64], FamilyCoins)>,
    business_want: Option<Grid>,
    lord_spend: Option<Vec<f64>>,
    beyond: Option<&Beyond>,
    sold_beyond: &mut f64,
    possible: &Grid,
) -> Option<Vec<f64>> {
    let n = world.n_locations();
    let nb = config.businesses.len();
    let n_hh = world.households.len();
    let prices = world.prices.column(product);
    let mut money: Vec<f64> = Vec::new();
    let mut want: Vec<f64> = Vec::new();
    let mut place: Vec<u32> = Vec::new();
    let family_budget = match &family {
        Some((wanted, coins)) => {
            match coins {
                FamilyCoins::Theirs => money.extend_from_slice(&world.households.money),
                FamilyCoins::Budget(b) => money.extend_from_slice(b),
            }
            want.extend_from_slice(wanted);
            place.extend_from_slice(&world.households.location);
            true
        }
        None => false,
    };
    let lord_first = money.len();
    if let Some(spend) = &lord_spend {
        // on a map with towns, the lord spends in his market town
        let spends_at: Vec<usize> = if world.geo.has_towns() { world.geo.town_of() } else { (0..n).collect() };
        for v in 0..n {
            let price = prices[spends_at[v]];
            money.push(spend[v]);
            want.push(div_or(spend[v], price, 0.0));
            place.push(spends_at[v] as u32);
        }
    }
    let business_first = money.len();
    if let Some(wanted) = &business_want {
        for v in 0..n {
            for b in 0..nb {
                money.push(config.trade.buying_budget * world.cash.get(v, b));
                want.push(wanted.get(v, b));
                place.push(v as u32);
            }
        }
    }
    // The world beyond buys the region's crafts at its border towns, up to
    // its most there, a full load once the price it pays beats the town's
    // by `full_load_margin`.
    let beyond_first = money.len();
    let mut buying_beyond: Vec<usize> = Vec::new();
    if let Some(Beyond { pays, most }) = beyond {
        let ramp = config.map.full_load_margin;
        for v in (0..n).filter(|&v| pays[v] > 0.0 && prices[v] > 0.0) {
            let load = crate::grid::clip((pays[v] / prices[v] - 1.0) / ramp, 0.0, 1.0);
            let w = load * most[v];
            money.push(w * prices[v]);
            want.push(w);
            place.push(v as u32);
            buying_beyond.push(v);
        }
    }
    let offer_here = offer.column(product);
    let seller = economy::seller_of(config)[product];
    let regional = world.supply.get(product).and_then(|line| line.clone());
    let sale = if let Some(line) = regional {
        // a good made only where a resource is: sold across the region from the makers' stock
        let at_makers = crafts::makers_price(&world.prices, &world.stock, &line, product);
        let held = world.stock.column(product);
        let b = seller.expect("a business makes every good");
        let can = possible.column(b);
        let r = crafts::regional_market(&mut money, &want, &place, &prices, &held, &can, &line, at_makers, &world.geo);
        for v in 0..n {
            let s = world.stock.at(v, product);
            *s = (*s - r.sold_by[v]).max(0.0);
            *world.cash.at(v, b) += r.paid_to[v];
            world.dealers_cash[v] += r.merchants[v];
        }
        demand.set_column(product, &r.sale.demand);
        r.sale
    } else {
        let sale = market::buy(&mut money, &want, &prices, &offer_here, &place);
        let spent_at = market::by_village(&sale.spent, &place, n);
        match cart {
            None => {
                for v in 0..n {
                    let s = world.stock.at(v, product);
                    *s = (*s - sale.sold[v]).max(0.0);
                    if let Some(b) = seller {
                        *world.cash.at(v, b) += spent_at[v];
                    }
                }
                demand.set_column(product, &sale.demand);
            }
            Some(cart) => {
                // goods carted in are sold for the business that made them, and count in its orders
                let mut stock = world.stock.column(product);
                let b = seller.expect("a business makes every good that is carted");
                let mut cash = world.cash.column(b);
                let mut asked = demand.column(product);
                towns::settle(
                    cart,
                    product,
                    &mut stock,
                    &mut cash,
                    &mut asked,
                    &offer_here,
                    &sale.sold,
                    &spent_at,
                    &sale.demand,
                );
                world.stock.set_column(product, &stock);
                world.cash.set_column(b, &cash);
                demand.set_column(product, &asked);
            }
        }
        sale
    };
    for (k, _) in buying_beyond.iter().enumerate() {
        let coins = sale.spent[beyond_first + k];
        world.town.purse -= coins; // (the world beyond pays)
        *sold_beyond += coins;
    }
    sold.set_column(product, &sale.sold);
    let mut bought = None;
    if family_budget {
        for h in 0..n_hh {
            world.households.money[h] -= sale.spent[h];
        }
        bought = Some(sale.bought[..n_hh].to_vec());
    }
    if lord_spend.is_some() {
        for v in 0..n {
            world.lord.purse[v] -= sale.spent[lord_first + v];
        }
    }
    if business_want.is_some() {
        let tool = config.products[product].use_ == "tool";
        for v in 0..n {
            for b in 0..nb {
                let k = business_first + v * nb + b;
                *world.cash.at(v, b) -= sale.spent[k];
                if tool {
                    *world.tools.at(v, b) += sale.bought[k];
                } else {
                    *world.supplies.at(v, b, product) += sale.bought[k];
                }
            }
        }
    }
    bought
}
