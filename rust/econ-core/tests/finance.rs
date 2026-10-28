//! The towns' moneylenders and an interest rate that moves (`finance.rs`).

use econ_core::checks::broken_invariants;
use econ_core::config::{configs_dir, Config, ScheduledEvent};
use econ_core::credit;
use econ_core::finance::Lenders;
use econ_core::households::Households;
use econ_core::Simulation;

fn region(name: &str) -> Config {
    Config::from_file(configs_dir().join("regions").join(format!("{name}.json"))).unwrap()
}

fn drought(config: &mut Config, month: i64) {
    config.scheduled_events.push(ScheduledEvent { event: "drought".into(), month, village: None, near: None });
}

/// Lenders of one town (place 0) serving it and a village (place 1).
fn two_places() -> Lenders {
    let mut lenders = Lenders::none(2);
    lenders.town = vec![0, 0];
    lenders.cash = vec![100.0, 0.0];
    lenders.capital = vec![100.0, 0.0];
    lenders
}

#[test]
fn without_finance_every_debt_grows_at_the_usual_rate_and_only_neighbours_lend() {
    let mut config = region("city3");
    config.months = 24;
    config.finance.enabled = false;
    drought(&mut config, 4);
    let mut sim = Simulation::new(config).unwrap();
    sim.run();
    // (the region's rate is a mean over people: the same up to rounding)
    assert!(sim.records.iter().all(|r| (r.interest_rate - sim.config.credit.interest).abs() < 1e-12));
    assert!(sim.world.interest.iter().all(|&r| r == sim.config.credit.interest));
    assert!(sim.records.iter().all(|r| r.lenders_coins == 0.0 && r.lenders_lent == 0.0));
    assert!(!sim.chronicle.entries.iter().any(|e| e.kind.starts_with("credit")));
}

#[test]
fn the_towns_lenders_start_with_part_of_the_richest_townsfolks_coins_and_no_coin_is_made() {
    let config = region("city3");
    let mut off = config.clone();
    off.finance.enabled = false;
    let (on, off) = (Simulation::new(config).unwrap(), Simulation::new(off).unwrap());
    let w = &on.world;
    assert!((w.money() - off.world.money()).abs() <= 1e-9 * w.money(), "coins are moved, not made");
    for t in (0..w.n_locations()).filter(|&v| w.geo.town[v]) {
        assert!(w.lenders.cash[t] > 0.0 && w.lenders.capital[t] == w.lenders.cash[t]);
        let before: f64 = (0..off.world.households.len())
            .filter(|&h| off.world.households.location[h] as usize == t)
            .map(|h| off.world.households.money[h])
            .sum();
        let after: f64 = (0..w.households.len())
            .filter(|&h| w.households.location[h] as usize == t)
            .map(|h| w.households.money[h])
            .sum();
        assert!((before - after - w.lenders.cash[t]).abs() < 1e-9 * before);
        assert!(w.lenders.cash[t] < sim_share(&on.config) * before, "only the richest fifth put coins in");
    }
    // every village lends from its market town; nothing is owed yet
    for v in 0..w.n_locations() {
        assert_eq!(w.lenders.town[v], w.geo.market[v]);
    }
    assert!(w.lenders.claims.iter().all(|&c| c == 0.0));
}

fn sim_share(config: &Config) -> f64 {
    config.finance.capital_share
}

#[test]
fn the_rate_is_the_usual_one_when_borrowers_ask_for_the_usual_share_of_what_is_on_offer() {
    let mut config = region("city3");
    config.finance.rate_months = 1.0; // (no averaging: the rate follows this month)
    let cfg = config.finance.clone();
    let mut lenders = Lenders::none(4);
    let mut rate = vec![0.0; 4];
    let offered = [1000.0, 1000.0, 1000.0, 0.0];
    let asked = [cfg.usual_share * 1000.0, 4.0 * cfg.usual_share * 1000.0, 0.0, 5.0];
    lenders.move_rates(&mut rate, &asked, &offered, &config);
    assert!((rate[0] - config.credit.interest).abs() < 1e-12);
    let doubled = (config.credit.interest * 4f64.powf(cfg.rate_response)).min(cfg.highest_rate);
    assert!((rate[1] - doubled).abs() < 1e-12, "four times the usual share: {}", rate[1]);
    assert_eq!(rate[2], cfg.lowest_rate, "nobody asks: coins go begging");
    assert_eq!(rate[3], cfg.highest_rate, "nothing on offer");
}

#[test]
fn the_towns_lenders_lend_what_neighbours_cannot_as_far_as_their_budget_goes() {
    let config = region("city3");
    let mut hh = Households::with_locations(vec![0, 1, 1, 1]);
    hh.money = vec![0.0, 0.0, 0.0, 10.0];
    let short = [0.0, 30.0, 30.0, 0.0];
    let spare = [0.0, 0.0, 0.0, 10.0];
    let worth = [0.0, 1000.0, 1000.0, 0.0];
    let personal = [0.0; 4];
    let mut lenders = two_places();
    let borrowing =
        credit::borrow_from(&mut hh, &short, &spare, &worth, &personal, 2, &config, None, Some(&mut lenders));
    let local = config.credit.lend_share * 10.0;
    let budget = config.finance.lend_share * 100.0;
    assert!((borrowing.lent[1] - (local + budget)).abs() < 1e-9, "{:?}", borrowing.lent);
    assert!((borrowing.by_town[1] - budget).abs() < 1e-9);
    assert!((lenders.claims[1] - budget).abs() < 1e-9 && (lenders.cash[0] - (100.0 - budget)).abs() < 1e-9);
    // debts balance the neighbours' claims and the lenders'
    let debts: f64 = hh.debt.iter().sum();
    assert!((debts - hh.lent.iter().sum::<f64>() - lenders.claims[1]).abs() < 1e-9);
}

#[test]
fn the_towns_lenders_take_no_land_and_share_losses_by_their_claims() {
    let config = region("city3");
    // one village (place 1): a debtor owing 40, half to a neighbour, half to the town
    let mut hh = Households::with_locations(vec![1, 1]);
    hh.debt = vec![40.0, 0.0];
    hh.lent = vec![0.0, 20.0];
    hh.land = vec![1.0, 0.0];
    let mut lenders = two_places();
    lenders.claims = vec![0.0, 20.0];
    let plot = [0.0, 30.0];
    let f = credit::foreclose(&mut hh, &plot, &[0.0, 0.0], &config, Some(&lenders));
    // the debt passes 90% of the plot's worth: the neighbour's half of it is settled with land
    assert!((f.value[1] - 15.0).abs() < 1e-9, "{:?}", f.value);
    assert!((hh.land[1] - 0.5).abs() < 1e-9 && (hh.land[0] - 0.5).abs() < 1e-9);
    assert!((hh.debt[0] - 25.0).abs() < 1e-9 && (hh.lent[1] - 5.0).abs() < 1e-9);
    // a write-off is shared by the claims: the lenders hold 20 of 25
    let lost = credit::default(&mut hh, &[0.0, 0.0], 2, &config, Some(&mut lenders));
    assert!((lost[1] - 25.0).abs() < 1e-9);
    assert!(hh.lent[1].abs() < 1e-9 && lenders.claims[1].abs() < 1e-9);
    assert!((lenders.flows.lost - 20.0).abs() < 1e-9);
}

#[test]
fn the_towns_lenders_spend_their_profits_in_their_town() {
    let config = region("city3");
    let mut lenders = two_places();
    lenders.cash[0] = 120.0;
    lenders.claims[1] = 30.0; // they own 150 against 100 they started with
    let (earned, spent) = lenders.spend(&[0, 0, 1], &[2.0, 3.0, 4.0], &config);
    let profit = config.finance.spend_share * 50.0;
    assert!((spent[0] - profit).abs() < 1e-9);
    assert!((earned[0] - 0.4 * profit).abs() < 1e-9 && (earned[1] - 0.6 * profit).abs() < 1e-9 && earned[2] == 0.0);
    assert!((lenders.cash[0] - (120.0 - profit)).abs() < 1e-9);
}

#[test]
fn in_a_drought_credit_grows_dear_the_towns_lenders_lend_and_the_books_balance() {
    let mut config = region("city3");
    config.months = 30;
    drought(&mut config, 4);
    let mut sim = Simulation::new(config).unwrap();
    let money = sim.world.money();
    for _ in 0..sim.config.months {
        let r = sim.step();
        let broken = broken_invariants(&sim.world);
        assert!(broken.is_empty(), "month {}: {broken:?}", r.month_number);
        assert!((sim.world.money() - money).abs() <= 1e-9 * money, "month {}: coins", r.month_number);
    }
    let usual = sim.records[..3].iter().map(|r| r.interest_rate).sum::<f64>() / 3.0;
    let dearest = sim.records.iter().map(|r| r.interest_rate).fold(0.0, f64::max);
    assert!(dearest > 1.4 * usual, "the rate rose from {usual:.3} to {dearest:.3}");
    assert!(sim.records.iter().map(|r| r.lenders_lent).sum::<f64>() > 0.0);
    assert!(sim.chronicle.entries.iter().any(|e| e.kind == "credit dear"));
}
