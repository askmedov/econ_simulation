//! The Rust rules against Python's on the golden fixtures: every
//! deterministic function, called on states from real runs, must give what
//! Python gave to a relative 1e-9.

mod common;

use common::*;
use econ_core::config::Config;
use econ_core::events::{self, ActiveEvent};
use econ_core::geography;
use econ_core::{
    council, credit, economy, environment, farms, healthcare, households, livestock, lords, market, metrics, migration,
    rules, town, towns, work,
};
use serde_json::Value;

fn s(v: &Value) -> &str {
    v.as_str().expect("a string")
}

fn n_of(v: &Value) -> usize {
    num(v) as usize
}

/// Run one case of `function` in Rust and compare it with Python's.
fn run_case(fx: &Fixture, function: &str, a: &Value, out: &Value) {
    let cfg: &Config = &fx.config;
    let n = cfg.villages.len();
    let what = format!("{} {function}", fx.name);
    let geo = geography::build(cfg).expect("map");
    match function {
        // rules
        "rules.season_factors" => check(&rules::season_factors(cfg), &out["returned"], &what),
        "rules.monthly_chance" => {
            let got: Vec<f64> = vecf(&a["annual"]).iter().map(|&x| rules::monthly_chance(x)).collect();
            check(&got, &out["returned"], &what)
        }
        "rules.food_need" => check(&rules::food_need(&fx.population(&a["population"]), cfg), &out["returned"], &what),
        "rules.spouse_alive" => {
            check_bools(&rules::spouse_alive(&fx.population(&a["population"])), &out["returned"], &what)
        }
        "rules.plan_ration" => check(
            &rules::plan_ration(&vecf(&a["stock"]), &vecf(&a["need"]), &grid(&a["outlook"]), cfg, num(&a["most"])),
            &out["returned"],
            &what,
        ),
        "rules.plan_ration_realistically" => check(
            &rules::plan_ration_realistically(
                &vecf(&a["stock"]),
                &vecf(&a["need"]),
                &grid(&a["outlook"]),
                cfg,
                3,
                num(&a["most"]),
            ),
            &out["returned"],
            &what,
        ),
        "rules.stock_to_keep" => {
            check(&rules::stock_to_keep(&vecf(&a["need"]), &grid(&a["outlook"]), cfg), &out["returned"], &what)
        }
        "rules.target_health" => {
            let got: Vec<f64> = vecf(&a["food_share"]).iter().map(|&x| rules::target_health(x, cfg)).collect();
            check(&got, &out["returned"], &what)
        }
        "rules.expected_work_factor" => {
            let got: Vec<f64> = vecf(&a["food_share"]).iter().map(|&x| rules::expected_work_factor(x, cfg)).collect();
            check(&got, &out["returned"], &what)
        }
        "rules.update_health" => {
            let mut pop = fx.population(&a["population"]);
            let (share, delta) = (vecf(&a["food_share"]), vecf(&a["health_delta"]));
            rules::update_health(&mut pop, |i| share[i], |i| delta[i], cfg);
            check(&pop.health, &out["health"], &what)
        }
        "rules.death_chance" => {
            let pop = fx.population(&a["population"]);
            let care = vecf(&a["care"]);
            check(&rules::death_chance(&pop, &vecf(&a["mortality_mult"]), cfg, Some(&care)), &out["returned"], &what)
        }
        "rules.wage_cover" => check(
            &rules::wage_cover(
                &vecf(&a["average_pay"]),
                &vecf(&a["food_price"]),
                &vecf(&a["need"]),
                &vecf(&a["workers"]),
            ),
            &out["returned"],
            &what,
        ),
        "rules.marriage_factor" => check(&rules::marriage_factor(&vecf(&a["cover"]), cfg), &out["returned"], &what),
        "rules.harvest_outlook" => check_grid(
            &rules::harvest_outlook(
                &vecf(&a["normal_capacity"]),
                &grid(&a["event_outlook"]),
                num(&a["month_of_year"]) as i64,
                cfg,
            ),
            &out["returned"],
            &what,
        ),

        // economy
        "economy.headcount" => {
            let pop = fx.population(&a["population"]);
            check_grid(&economy::headcount(&pop, n, cfg, None), &out["returned"], &what)
        }
        "economy.labor" => {
            let pop = fx.population(&a["population"]);
            let force = economy::workforce(&pop, n, cfg);
            check_grid(&economy::labor(&pop, &force, cfg, None, false), &out["returned"], &what);
            check_grid(&economy::labor(&pop, &force, cfg, None, true), &out["full"], &what)
        }
        "economy.tool_factor" => {
            check_grid(&economy::tool_factor(&grid(&a["tools"]), &grid(&a["workers"]), cfg), &out["returned"], &what)
        }
        "economy.capacity" => {
            let (farm_mult, sown) = (vecf(&a["farm_mult"]), vecf(&a["sown"]));
            let got = economy::capacity(
                &grid(&a["labor"]),
                &grid(&a["tools"]),
                &grid(&a["workers"]),
                &vecf(&a["land"]),
                &grid(&a["production_mult"]),
                cfg,
                Some(&farm_mult),
                Some(&sown),
            );
            check_grid(&got, &out["returned"], &what)
        }
        "economy.farm_land" => {
            let farm_mult = vecf(&a["farm_mult"]);
            let got =
                economy::farm_land(&vecf(&a["land"]), &grid(&a["tools"]), &grid(&a["workers"]), cfg, Some(&farm_mult));
            check(&got, &out["returned"], &what)
        }
        "economy.limit_by_supplies" => {
            let mut supplies = grid3(&a["supplies"]);
            let made = economy::limit_by_supplies(&grid(&a["planned"]), &mut supplies, cfg);
            check_grid(&made, &out["returned"], &what);
            check(&supplies.data, &Value::from(grid3(&out["supplies"]).data), &what)
        }
        "economy.productivity" => {
            let pop = fx.population(&a["population"]);
            let force = economy::workforce(&pop, n, cfg);
            let farm_mult = vecf(&a["farm_mult"]);
            let got = economy::productivity(
                &pop,
                &force,
                &grid(&a["tools"]),
                &grid(&a["workers"]),
                &vecf(&a["land"]),
                cfg,
                Some(&farm_mult),
                None,
            );
            check_grid(&got, &out["returned"], &what)
        }
        "economy.fair_prices" => check_grid(
            &economy::fair_prices(&vecf(&a["wage"]), &grid(&a["per_worker"]), &grid(&a["prices"]), cfg),
            &out["returned"],
            &what,
        ),
        "economy.potential_pay" => check_grid(
            &economy::potential_pay(&grid(&a["prices"]), &grid(&a["per_worker"]), cfg),
            &out["returned"],
            &what,
        ),
        "economy.wanted_workers" => check_grid(
            &economy::wanted_workers(&grid(&a["work_needed"]), &grid(&a["workers"]), cfg),
            &out["returned"],
            &what,
        ),

        // work
        "work.harvest_help" => {
            let pop = fx.population(&a["population"]);
            let force = economy::workforce(&pop, n, cfg);
            let got =
                work::harvest_help(&pop, n, n_of(&a["n_households"]), num(&a["month_of_year"]) as i64, cfg, &force);
            check(&got.farm, &out["farm"], &what);
            check(&got.kept, &out["kept"], &what);
            check(&got.households, &out["households"], &what)
        }
        "work.harvest_boost" => check(
            &work::harvest_boost(&vecf(&a["farm_effort"]), &vecf(&a["help_effort"]), cfg),
            &out["returned"],
            &what,
        ),
        "work.outlook_boost" => check_grid(
            &work::outlook_boost(&vecf(&a["boost"]), num(&a["month_of_year"]) as i64, n_of(&a["months"]), cfg),
            &out["returned"],
            &what,
        ),
        "work.home_cloth" => {
            let pop = fx.population(&a["population"]);
            check(
                &work::home_cloth(&pop, n_of(&a["n_households"]), num(&a["month_of_year"]) as i64, cfg),
                &out["returned"],
                &what,
            )
        }
        "work.kin_help" => {
            let mut hh = fx.households(&a["households"]);
            let moved = work::kin_help(&mut hh, &vecb(&a["alive"]), &vecf(&a["short"]), &vecf(&a["spare"]), cfg);
            check(&moved, &out["returned"], &what);
            check(&hh.grain, &out["grain"], &what)
        }

        // farms
        "farms.income_shares" => {
            let pop = fx.population(&a["population"]);
            let hh = fx.households(&a["households"]);
            let force = economy::workforce(&pop, n, cfg);
            let (animals, lord, extra) = (vecf(&a["animals_part"]), vecf(&a["lord_land"]), vecf(&a["extra_effort"]));
            let (shares, to_lord, _) = farms::income_shares(
                &pop,
                &hh,
                &vecf(&a["village_land"]),
                cfg,
                Some(&animals),
                Some(&lord),
                Some(&extra),
                &force,
                None,
            );
            check(&shares, &out["shares"], &what);
            check(&to_lord, &out["to_lord"], &what)
        }
        "farms.share_harvest" => {
            let mut hh = fx.households(&a["households"]);
            let mut fg = vecf(&a["farm_grain"]);
            let to_lord = vecf(&a["to_lord"]);
            let (received, rent, _) = farms::share_harvest(
                &mut hh,
                &mut fg,
                &vecf(&a["keep_for_tools"]),
                &vecf(&a["shares"]),
                Some(&to_lord),
                None,
            );
            check(&received, &out["received"], &what);
            check(&rent, &out["rent"], &what);
            check(&fg, &out["farm_grain"], &what);
            check(&hh.grain, &out["grain"], &what)
        }
        "farms.plan_family_food" => {
            let hh = fx.households(&a["households"]);
            let markup = vecf(&a["markup"]);
            let plan = farms::plan_family_food(
                &hh,
                &vecf(&a["need"]),
                &grid(&a["village_outlook"]),
                &vecf(&a["shares"]),
                &vecf(&a["after_deductions"]),
                cfg,
                Some(&markup),
            );
            check(&plan.own, &out["own"], &what);
            check(&plan.spare, &out["spare"], &what);
            check(&plan.want, &out["want"], &what)
        }
        "farms.sellers_share" => {
            let (lord, imports) = (vecf(&a["lord_offer"]), vecf(&a["imports"]));
            let got = farms::sellers_share(
                &vecf(&a["sold"]),
                &vecf(&a["offered_by_families"]),
                &vecf(&a["farm_offer"]),
                &vecu(&a["location"]),
                Some(&lord),
                Some(&imports),
            );
            check(&got.families, &out["families"], &what);
            check(&got.farms, &out["farms"], &what);
            check(&got.lord, &out["lord"], &what);
            check(&got.imports, &out["imports"], &what)
        }
        "farms.seed_plan" => check_one(farms::seed_plan(num(&a["month_of_year"]) as i64, cfg), &out["returned"], &what),
        "farms.seed_gathered" => {
            check_one(farms::seed_gathered(num(&a["month_of_year"]) as i64, cfg), &out["returned"], &what)
        }
        "farms.keep_seed" => {
            let (mut fg, mut seed) = (vecf(&a["farm_grain"]), vecf(&a["seed"]));
            let kept = farms::keep_seed(
                &mut fg,
                &mut seed,
                &vecf(&a["needed"]),
                &vecf(&a["harvest"]),
                num(&a["month_of_year"]) as i64,
                cfg,
            );
            check(&kept, &out["returned"], &what);
            check(&fg, &out["farm_grain"], &what);
            check(&seed, &out["seed"], &what)
        }
        "farms.seed_outlook" => check_grid(
            &farms::seed_outlook(&vecf(&a["needed"]), num(&a["month_of_year"]) as i64, n_of(&a["months"]), cfg),
            &out["returned"],
            &what,
        ),
        "farms.sown_outlook" => check_grid(
            &farms::sown_outlook(&vecf(&a["sown"]), num(&a["month_of_year"]) as i64, n_of(&a["months"]), cfg),
            &out["returned"],
            &what,
        ),
        "farms.sow" => {
            let mut hh = fx.households(&a["households"]);
            let mut seed = vecf(&a["seed"]);
            let (sown, given) = farms::sow(&mut hh, &mut seed, &vecf(&a["needed"]), &vecf(&a["need"]));
            check(&sown, &out["sown"], &what);
            check(&given, &out["given"], &what);
            check(&hh.grain, &out["grain"], &what);
            check(&seed, &out["seed_after"], &what)
        }

        // market
        "market.share_with_neighbours" => {
            let mut money = vecf(&a["money"]);
            let pot =
                market::share_with_neighbours(&mut money, &vecf(&a["cost"]), &vecu(&a["location"]), n_of(&a["n"]), cfg);
            check(&pot, &out["returned"], &what);
            check(&money, &out["money"], &what)
        }
        "market.buy" => {
            let mut money = vecf(&a["money"]);
            let sale = market::buy(
                &mut money,
                &vecf(&a["want"]),
                &vecf(&a["price"]),
                &vecf(&a["offer"]),
                &vecu(&a["location"]),
            );
            check(&sale.bought, &out["bought"], &what);
            check(&sale.spent, &out["spent"], &what);
            check(&sale.sold, &out["sold"], &what);
            check(&sale.demand, &out["demand"], &what);
            check(&money, &out["money"], &what)
        }
        "market.adjust_markup" => check_grid(
            &market::adjust_markup(&grid(&a["markup"]), &grid(&a["demand"]), &grid(&a["supply"]), cfg),
            &out["returned"],
            &what,
        ),
        "market.seasonal_buffer" => check(&market::seasonal_buffer(&vecf(&a["profile"])), &out["returned"], &what),
        "market.grain_markup" => {
            check(&market::grain_markup(&vecf(&a["markup"]), &vecf(&a["cover"]), cfg), &out["returned"], &what)
        }
        "market.pay_wages" => {
            let pop = fx.population(&a["population"]);
            let force = economy::workforce(&pop, n, cfg);
            let mut cash = grid(&a["cash"]);
            let keep = grid(&a["keep"]);
            let (income, payout) =
                market::pay_wages(&mut cash, &pop, n_of(&a["n_households"]), cfg, Some(&keep), &force);
            check(&income, &out["income"], &what);
            check_grid(&payout, &out["payout"], &what);
            check_grid(&cash, &out["cash"], &what)
        }

        // credit
        "credit.land_price" => check(&credit::land_price(&vecf(&a["rent_per_plot"]), cfg), &out["returned"], &what),
        "credit.collateral" => check(
            &credit::collateral(&fx.households(&a["households"]), &vecf(&a["plot_price"]), &vecf(&a["animal_price"])),
            &out["returned"],
            &what,
        ),
        "credit.biggest_first" => check(
            &credit::biggest_first(&vecf(&a["capacity"]), &vecf(&a["totals"]), &vecu(&a["location"])),
            &out["returned"],
            &what,
        ),
        "credit.pro_rata" => check(
            &credit::pro_rata(&vecf(&a["weights"]), &vecf(&a["totals"]), &vecu(&a["location"])),
            &out["returned"],
            &what,
        ),
        "credit.accrue_interest" => {
            let mut hh = fx.households(&a["households"]);
            // (Python's one rate, in every place; no town lenders)
            let places = hh.location.iter().map(|&v| v as usize + 1).max().unwrap_or(1);
            check(&credit::accrue_interest(&mut hh, &vec![cfg.credit.interest; places], None), &out["returned"], &what);
            check_households(&hh, &out["households"], &what)
        }
        "credit.borrow" => {
            let mut hh = fx.households(&a["households"]);
            let lent = credit::borrow(
                &mut hh,
                &vecf(&a["short"]),
                &vecf(&a["spare"]),
                &vecf(&a["worth"]),
                &vecf(&a["personal"]),
                n_of(&a["n"]),
                cfg,
            );
            check(&lent, &out["returned"], &what);
            check_households(&hh, &out["households"], &what)
        }
        "credit.repay" => {
            let mut hh = fx.households(&a["households"]);
            let spare = vecf(&a["spare"]);
            let repaid = credit::repay(&mut hh, &vecf(&a["income"]), n_of(&a["n"]), cfg, Some(&spare), None);
            check(&repaid, &out["returned"], &what);
            check_households(&hh, &out["households"], &what)
        }
        "credit.repay_in_grain" => {
            let mut hh = fx.households(&a["households"]);
            check(
                &credit::repay_in_grain(&mut hh, &vecf(&a["need"]), &vecf(&a["food_price"]), cfg, None),
                &out["returned"],
                &what,
            );
            check_households(&hh, &out["households"], &what)
        }
        "credit.foreclose" => {
            let mut hh = fx.households(&a["households"]);
            let f = credit::foreclose(&mut hh, &vecf(&a["plot_price"]), &vecf(&a["animal_price"]), cfg, None);
            check(&f.animals, &out["animals"], &what);
            check(&f.land, &out["land"], &what);
            check(&f.value, &out["value"], &what);
            check_households(&hh, &out["households"], &what)
        }
        "credit.default" => {
            let mut hh = fx.households(&a["households"]);
            check(&credit::default(&mut hh, &vecf(&a["limit"]), n_of(&a["n"]), cfg, None), &out["returned"], &what);
            check_households(&hh, &out["households"], &what)
        }
        "credit.sell_land" => {
            let mut hh = fx.households(&a["households"]);
            let ls = credit::sell_land(&mut hh, &vecf(&a["short"]), &vecf(&a["spare"]), &vecf(&a["worth"]), cfg);
            check(&ls.plots, &out["plots"], &what);
            check(&ls.price, &out["price"], &what);
            check_households(&hh, &out["households"], &what)
        }
        "credit.cancel_debts" => {
            let mut hh = fx.households(&a["households"]);
            check(&credit::cancel_debts(&mut hh, &vecf(&a["share"]), None), &out["returned"], &what);
            check_households(&hh, &out["households"], &what)
        }
        "credit.write_off" => {
            let mut hh = fx.households(&a["households"]);
            credit::write_off(&mut hh, &vecb(&a["gone"]), n_of(&a["n"]), None);
            check_households(&hh, &out["households"], &what)
        }

        // livestock
        "livestock.farm_factor" => {
            check(&livestock::farm_factor(&vecf(&a["animals"]), &vecf(&a["land"]), cfg), &out["returned"], &what)
        }
        "livestock.owners_part" => check(&livestock::owners_part(&vecf(&a["factor"])), &out["returned"], &what),
        "livestock.grow_and_die" => {
            let mut hh = fx.households(&a["households"]);
            let lost = livestock::grow_and_die(&mut hh, &vecf(&a["farm_weather"]), &vecf(&a["heating_mult"]), cfg);
            check(&lost, &out["returned"], &what);
            check(&hh.animals, &out["animals"], &what)
        }
        "livestock.winter_cull" => {
            let mut hh = fx.households(&a["households"]);
            let pasture = vecf(&a["pasture"]);
            let (culled, meat) = livestock::winter_cull(&mut hh, &vecf(&a["village_land"]), cfg, Some(&pasture));
            check(&culled, &out["culled"], &what);
            check(&meat, &out["meat"], &what);
            check_households(&hh, &out["households"], &what)
        }
        "livestock.slaughter_in_hunger" => {
            let mut hh = fx.households(&a["households"]);
            let (killed, meat) =
                livestock::slaughter_in_hunger(&mut hh, &vecf(&a["need"]), &vecf(&a["food_price"]), cfg);
            check(&killed, &out["killed"], &what);
            check(&meat, &out["meat"], &what);
            check_households(&hh, &out["households"], &what)
        }
        "livestock.distress_sales" => {
            let mut hh = fx.households(&a["households"]);
            let sales =
                livestock::distress_sales(&mut hh, &vecf(&a["short"]), &vecf(&a["spare"]), &vecf(&a["worth"]), cfg);
            check(&sales.sold, &out["sold"], &what);
            check(&sales.price, &out["price"], &what);
            check_households(&hh, &out["households"], &what)
        }
        "livestock.shear" => {
            check(&livestock::shear(&fx.households(&a["households"]), &vecf(&a["worth"])), &out["returned"], &what)
        }

        // lords
        "lords.cart_away" => {
            let mut l = lords(&a["lords"]);
            check(&lords::cart_away(&mut l, cfg), &out["returned"], &what);
            check_lords(&l, &out["lords"], &what)
        }
        "lords.offer" => check(&lords::offer(&lords(&a["lords"]), cfg), &out["returned"], &what),
        "lords.local_spending" => check(
            &lords::local_spending(&lords(&a["lords"]), cfg, a["in_town"].as_bool().unwrap()),
            &out["returned"],
            &what,
        ),
        "lords.pay_servants" => {
            let mut l = lords(&a["lords"]);
            let paid = lords::pay_servants(
                &mut l,
                &veci(&a["market"]),
                &vecu(&a["households_location"]),
                &vecf(&a["members"]),
                cfg,
            );
            check(&paid, &out["returned"], &what);
            check_lords(&l, &out["lords"], &what)
        }
        "lords.sell_in_town" => {
            let mut l = lords(&a["lords"]);
            let (mut dg, mut dc) = (vecf(&a["dealers_grain"]), vecf(&a["dealers_cash"]));
            let promised = vecf(&a["promised"]);
            let sold = lords::sell_in_town(
                &mut l,
                &vecf(&a["carted"]),
                &veci(&a["market"]),
                &mut dg,
                &mut dc,
                &vecf(&a["town_price"]),
                Some(&promised),
            );
            check(&sold, &out["returned"], &what);
            check_lords(&l, &out["lords"], &what);
            check(&dg, &out["dealers_grain"], &what);
            check(&dc, &out["dealers_cash"], &what)
        }
        "lords.charity" => {
            let mut c = cfg.clone();
            c.lord.charity_in_famine = a["charity"].as_bool().unwrap();
            let mut l = lords(&a["lords"]);
            let given = lords::charity(&mut l, &vecf(&a["gap"]), &vecu(&a["location"]), &vecb(&a["famine"]), &c);
            check(&given, &out["returned"], &what);
            check_lords(&l, &out["lords"], &what)
        }
        "lords.collect_state_tax" => {
            let mut c = cfg.clone();
            c.state.remit_in_famine = a["remit"].as_bool().unwrap();
            let mut hh = fx.households(&a["households"]);
            let mut l = lords(&a["lords"]);
            let (living, keep) = (vecb(&a["living"]), vecf(&a["keep"]));
            let take = lords::collect_state_tax(
                &mut hh,
                &mut l,
                &vecf(&a["wage"]),
                &vecf(&a["rent_per_plot"]),
                &vecf(&a["food_price"]),
                &vecb(&a["famine"]),
                &c,
                Some(&living),
                Some(&keep),
            );
            check(&take.coins, &out["coins"], &what);
            check(&take.grain, &out["grain"], &what);
            check_households(&hh, &out["households"], &what);
            check_lords(&l, &out["lords"], &what)
        }
        "lords.requisition" => {
            let mut hh = fx.households(&a["households"]);
            let mut l = lords(&a["lords"]);
            let mut fg = vecf(&a["farm_grain"]);
            let (taken, from_barn) = lords::requisition(&mut hh, &mut l, &mut fg, &vecf(&a["share"]));
            check(&taken, &out["taken"], &what);
            check(&from_barn, &out["from_barn"], &what);
            check_households(&hh, &out["households"], &what);
            check_lords(&l, &out["lords"], &what);
            check(&fg, &out["farm_grain_after"], &what)
        }

        // council
        "council.levy_grain" => {
            let mut c = councils(&a["councils"]);
            let mut granary = vecf(&a["granary"]);
            let taken = council::levy_grain(
                &mut c,
                &vecf(&a["produced"]),
                &mut granary,
                &vecf(&a["need"]),
                &vecb(&a["famine"]),
                cfg,
            );
            check(&taken, &out["returned"], &what);
            check_councils(&c, &out["councils"], &what);
            check(&granary, &out["granary_after"], &what)
        }
        "council.give_relief" => {
            let mut c = councils(&a["councils"]);
            let given =
                council::give_relief(&mut c, &vecf(&a["bought"]), &vecf(&a["need"]), &vecu(&a["location"]), cfg);
            check(&given, &out["returned"], &what);
            check_councils(&c, &out["councils"], &what)
        }
        "council.cash_relief" => {
            let mut c = councils(&a["councils"]);
            let mut money = vecf(&a["money"]);
            let given = council::cash_relief(
                &mut c,
                &mut money,
                &vecf(&a["shortfall"]),
                &vecu(&a["location"]),
                &vecf(&a["keep"]),
                cfg,
            );
            check(&given, &out["returned"], &what);
            check_councils(&c, &out["councils"], &what);
            check(&money, &out["money"], &what)
        }
        "council.collect_taxes" => {
            let mut c = councils(&a["councils"]);
            let mut income = vecf(&a["income"]);
            let taxes = council::collect_taxes(
                &mut income,
                &vecu(&a["location"]),
                &mut c,
                &vecf(&a["target"]),
                &vecb(&a["famine"]),
                cfg,
            );
            check(&taxes, &out["returned"], &what);
            check_councils(&c, &out["councils"], &what);
            check(&income, &out["income_after"], &what)
        }
        "council.pay_staff" => {
            let pop = fx.population(&a["population"]);
            let mut c = councils(&a["councils"]);
            let (income, paid) =
                council::pay_staff(&pop, &mut c, num(&a["job"]) as i16, &vecf(&a["pay"]), n_of(&a["n_households"]));
            check(&income, &out["income"], &what);
            check(&paid, &out["paid"], &what);
            check_councils(&c, &out["councils"], &what)
        }
        "council.check_formation" => {
            let mut c = councils(&a["councils"]);
            c.months_ready = vec![num(&a["months_ready"]) as i64; n];
            let new = council::check_formation(&mut c, &vecf(&a["people"]), cfg);
            check_bools(&new, &out["returned"], &what);
            check_councils(&c, &out["councils"], &what)
        }

        // environment
        "environment.size_woods" => {
            let richness = vecf(&a["richness"]);
            let (capacity, stock) = environment::size_woods(&vecf(&a["people"]), cfg, Some(&richness));
            let returned = out["returned"].as_array().unwrap();
            check(&capacity, &returned[0], &what);
            check(&stock, &returned[1], &what)
        }
        "environment.woods_reach" => {
            check(&environment::woods_reach(&vecf(&a["woods"]), &vecf(&a["normal"]), cfg), &out["returned"], &what)
        }
        "environment.grow_woods" => {
            let mut woods = vecf(&a["woods"]);
            let lost =
                environment::grow_woods(&mut woods, &vecf(&a["capacity"]), &vecf(&a["cut"]), &vecf(&a["burned"]), cfg);
            check(&lost, &out["returned"], &what);
            check(&woods, &out["woods"], &what)
        }
        "environment.soil_target" => {
            let (people, land) = (vecf(&a["people"]), vecf(&a["land"]));
            let got: Vec<f64> = (0..people.len()).map(|i| environment::soil_target(people[i], land[i], cfg)).collect();
            check(&got, &out["returned"], &what)
        }
        "environment.tire_soil" => {
            let mut soil = vecf(&a["soil"]);
            environment::tire_soil(&mut soil, &vecf(&a["people"]), &vecf(&a["land"]), cfg);
            check(&soil, &out["soil"], &what)
        }

        // town
        "town.price" => check(
            &town::price(&town(&a["town"]), num(&a["month_of_year"]) as i64, &vecf(&a["farm_weather"]), cfg),
            &out["returned"],
            &what,
        ),
        "town.merchants" => {
            let m = town::merchants(
                &vecf(&a["village_price"]),
                &vecf(&a["town_price"]),
                &vecf(&a["need"]),
                cfg,
                &vecf(&a["transport"]),
            );
            check(&m.exports, &out["exports"], &what);
            check(&m.imports, &out["imports"], &what)
        }
        "town.lose_coins" => {
            let mut money = vecf(&a["money"]);
            let mut t = town(&a["town"]);
            check_one(town::lose_coins(&mut money, &mut t, cfg), &out["returned"], &what);
            check(&money, &out["money"], &what);
            check_one(t.coins_lost, &out["town"]["coins_lost"], &what)
        }

        // migration
        "migration.appeal" => check(
            &migration::appeal(
                &vecf(&a["prospects"]),
                &memory(&a["memory"]),
                &vecf(&a["people"]),
                &vecf(&a["land"]),
                &vecf(&a["lord_take"]),
                cfg,
            ),
            &out["returned"],
            &what,
        ),
        "migration.remember" => {
            let mut m = memory(&a["memory"]);
            let mult = vecf(&a["mortality_mult"]);
            migration::remember(
                &mut m,
                &vecf(&a["ration"]),
                &vecf(&a["deaths"]),
                &vecf(&a["people"]),
                &vecf(&a["plundered"]),
                cfg,
                Some(&mult),
            );
            let p = &out["memory"];
            check(&m.hunger, &p["hunger"], &what);
            check(&m.danger, &p["danger"], &what);
            check(&m.danger_now, &p["danger_now"], &what)
        }
        "migration.remember_leaving" => {
            let mut m = memory(&a["memory"]);
            migration::remember_leaving(&mut m, &vecf(&a["left"]), &vecf(&a["people"]));
            check(&m.leaving, &out["memory"]["leaving"], &what)
        }

        // events
        "events.chance_now" => {
            let spec = cfg.events.iter().find(|e| e.name == s(&a["name"])).unwrap();
            check_one(events::chance_now(spec, a["repeated"].as_bool().unwrap()), &out["returned"], &what)
        }
        "events.modifiers" | "events.production_outlook" => {
            let active: Vec<ActiveEvent> = a["active"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| ActiveEvent {
                    spec: n_of(&e["spec"]),
                    location: n_of(&e["location"]),
                    months_left: num(&e["months_left"]) as i64,
                    effects: e["effects"]
                        .as_object()
                        .unwrap()
                        .iter()
                        .map(|(k, v)| (events::effect_named(k).unwrap(), num(v)))
                        .collect(),
                })
                .collect();
            if function == "events.modifiers" {
                let names: Vec<String> = cfg.businesses.iter().map(|b| b.name.clone()).collect();
                let m = events::modifiers(&active, cfg, n, &names);
                let p = &out["returned"];
                check_grid(&m.production_mult, &p["production_mult"], &what);
                for (got, key) in [
                    (&m.health_delta, "health_delta"),
                    (&m.mortality_mult, "mortality_mult"),
                    (&m.fertility_mult, "fertility_mult"),
                    (&m.granary_loss, "granary_loss"),
                    (&m.heating_mult, "heating_mult"),
                    (&m.debt_cancel, "debt_cancel"),
                    (&m.requisition, "requisition"),
                    (&m.debase, "debase"),
                    (&m.woods_burned, "woods_burned"),
                    (&m.wool_price, "wool_price"),
                ] {
                    check(got, &p[key], &format!("{what} {key}"));
                }
            } else {
                let got = events::production_outlook(&active, cfg, n, n_of(&a["months"]), s(&a["business"]));
                check_grid(&got, &out["returned"], &what)
            }
        }

        // households
        "households.sizes" => {
            let pop = fx.population(&a["population"]);
            check(&households::sizes(&pop, n_of(&a["n_households"])), &out["returned"], &what)
        }
        "households.pass_on_land_and_grain" => {
            let mut hh = fx.households(&a["households"]);
            let mut reserve = vecf(&a["reserve"]);
            let got =
                households::pass_on_land_and_grain(&mut hh, &vecf(&a["size"]), &mut reserve, &vecb(&a["council"]));
            check(&got, &out["returned"], &what);
            check_households(&hh, &out["households"], &what);
            check(&reserve, &out["reserve_after"], &what)
        }
        "households.pass_on_savings" => {
            let mut hh = fx.households(&a["households"]);
            let mut treasury = vecf(&a["treasury"]);
            households::pass_on_savings(&mut hh, &vecf(&a["size"]), &mut treasury, &vecb(&a["council"]));
            check_households(&hh, &out["households"], &what);
            check(&treasury, &out["treasury_after"], &what)
        }

        // metrics
        "metrics.poorest_fifth_ration" => check_one(
            metrics::poorest_fifth_ration(
                &vecf(&a["money"]),
                &vecf(&a["size"]),
                &vecf(&a["bought"]),
                &vecf(&a["need"]),
            ),
            &out["returned"],
            &what,
        ),
        "metrics.age_groups" => {
            let (c, w, e) = metrics::age_groups(&fx.population(&a["population"]), cfg);
            check(&[c as f64, w as f64, e as f64], &out["returned"], &what)
        }
        "metrics.average_health" => {
            check_one(metrics::average_health(&fx.population(&a["population"])), &out["returned"], &what)
        }

        // healthcare
        "healthcare.treat" => {
            let mut pop = fx.population(&a["population"]);
            let (care, treated) = healthcare::treat(&mut pop, &vecf(&a["healers"]), &vecf(&a["risk"]), cfg);
            check(&care, &out["care"], &what);
            check(&treated, &out["treated"], &what);
            let p = population(&out["population"]);
            assert_eq!(pop.count, p.count, "{what}: counts");
            check(&pop.health, &out["population"]["health"], &what)
        }

        // towns
        "towns.gets" => check(&towns::gets(&geo), &out["returned"], &what),
        "towns.store_markup" => check(
            &towns::store_markup(&vecf(&a["markup"]), &vecf(&a["store"]), &vecf(&a["need"]), cfg),
            &out["returned"],
            &what,
        ),
        "towns.from_neighbours" => check(&towns::from_neighbours(&vecf(&a["previous"]), &geo), &out["returned"], &what),
        "towns.delivered_price" => check(
            &towns::delivered_price(&vecf(&a["village_price"]), &geo, &vecf(&a["weights"])),
            &out["returned"],
            &what,
        ),
        "towns.carts" => {
            let got = towns::carts(
                &grid(&a["prices"]),
                &geo,
                &grid(&a["wanted"]),
                &grid(&a["stock"]),
                &vecf(&a["dealers_cash"]),
                &vecf(&a["weights"]),
                n_of(&a["food"]),
                cfg,
            );
            let p = &out["returned"];
            check(&got.grain_in, &p["grain_in"], &what);
            check(&got.grain_held, &p["grain_held"], &what);
            check(&got.grain_out, &p["grain_out"], &what);
            assert_eq!(got.src, vecf(&p["src"]).iter().map(|&x| x as usize).collect::<Vec<_>>(), "{what}: src");
            assert_eq!(got.dst, vecf(&p["dst"]).iter().map(|&x| x as usize).collect::<Vec<_>>(), "{what}: dst");
            check_grid(&got.goods, &p["goods"], &what)
        }
        "towns.settle" => {
            let cart = carts(&a["cart"], cfg.products.len());
            let (mut stock, mut cash) = (vecf(&a["stock"]), vecf(&a["cash"]));
            let mut demand = vec![0.0; n];
            towns::settle(
                &cart,
                n_of(&a["product"]),
                &mut stock,
                &mut cash,
                &mut demand,
                &vecf(&a["offer"]),
                &vecf(&a["sold"]),
                &vecf(&a["spent"]),
                &vecf(&a["asked"]),
            );
            check(&stock, &out["stock"], &what);
            check(&cash, &out["cash"], &what);
            check(&demand, &out["demand"], &what)
        }
        "towns.between_towns" => {
            let (mut dg, mut dc) = (vecf(&a["dealers_grain"]), vecf(&a["dealers_cash"]));
            let moved = towns::between_towns(&mut dg, &mut dc, &vecf(&a["price"]), &vecf(&a["need"]), &geo, cfg);
            check(&moved, &out["returned"], &what);
            check(&dg, &out["dealers_grain"], &what);
            check(&dc, &out["dealers_cash"], &what)
        }
        "towns.border_trade" => {
            let (mut dg, mut dc) = (vecf(&a["dealers_grain"]), vecf(&a["dealers_cash"]));
            let outside = vecf(&a["outside"]);
            let moved =
                towns::border_trade(&mut dg, &mut dc, &vecf(&a["town_price"]), &outside, &vecf(&a["need"]), &geo, cfg);
            check(&moved, &out["returned"], &what);
            check(&dg, &out["dealers_grain"], &what);
            check(&dc, &out["dealers_cash"], &what);
            check_one(towns::outside_coins(&moved, &outside, cfg), &out["coins"], &what)
        }
        "towns.dealers_payout" => {
            let mut dc = vecf(&a["dealers_cash"]);
            let paid = towns::dealers_payout(&mut dc, &vecf(&a["need"]), &vecf(&a["price"]), cfg);
            check(&paid, &out["returned"], &what);
            check(&dc, &out["dealers_cash"], &what)
        }
        other => panic!("{}: no Rust check for {other}", fx.name),
    }
}

#[test]
fn every_deterministic_function_matches_python_on_every_state() {
    let mut checked = 0;
    for name in STATES {
        let fx = load(name);
        for function in fx.cases.keys() {
            for (args, out) in fx.of(function) {
                run_case(&fx, function, &args, &out);
                checked += 1;
            }
        }
    }
    eprintln!("{checked} fixture cases match Python");
    assert!(checked > 800, "only {checked} cases checked");
}

#[test]
fn maps_match_python_for_every_exported_region() {
    let path = fixtures_dir().join("geography.json");
    let data: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    for (name, entry) in data["maps"].as_object().unwrap() {
        let config: Config = serde_json::from_value(entry["config"].clone()).unwrap();
        let rust = geography::build(&config).unwrap();
        let python = geography(&entry["geography"]);
        let what = format!("map {name}");
        assert_eq!(rust.terrain, python.terrain, "{what}: terrain");
        assert_eq!(rust.market, python.market, "{what}: market");
        assert_eq!(rust.town, python.town, "{what}: towns");
        assert_eq!(rust.border, python.border, "{what}: border");
        assert_eq!(rust.on_map, python.on_map, "{what}: on a map");
        for (got, want, field) in [
            (&rust.x, &python.x, "x"),
            (&rust.y, &python.y, "y"),
            (&rust.transport, &python.transport, "transport"),
            (&rust.pasture, &python.pasture, "pasture"),
            (&rust.woods, &python.woods, "woods"),
            (&rust.forage, &python.forage, "forage"),
            (&rust.arable, &python.arable, "arable"),
            (&rust.to_border, &python.to_border, "to_border"),
            (&rust.routes.cost, &python.routes.cost, "route costs"),
            (&rust.routes.water, &python.routes.water, "route water"),
        ] {
            assert_eq!(got.len(), want.len(), "{what}: {field} lengths");
            for i in 0..got.len() {
                assert!(same(got[i], want[i]), "{what}: {field}[{i}] Rust {} against Python {}", got[i], want[i]);
            }
        }
        assert_eq!(rust.routes.a, python.routes.a, "{what}: routes (a)");
        assert_eq!(rust.routes.b, python.routes.b, "{what}: routes (b)");
    }
    let z = vecf(&data["normal_share"]["z"]);
    let got: Vec<f64> = z.iter().map(|&x| geography::normal_share(x)).collect();
    check(&got, &data["normal_share"]["returned"], "normal_share");
}
