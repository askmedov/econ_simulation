"""Golden fixtures for the Rust engine: inputs and outputs of the model's
deterministic functions, which Rust must match to a relative 1e-9.

    python scripts/export_fixtures.py     # rewrite rust/fixtures/*.json

Inputs come from real states: Python runs a few settings for some months
(a village at harvest, a village in a famine with debts, a hamlet, a region
with towns and a city in a shearing month and later), and each function is
called on that state the way `Simulation.step` calls it. Each file holds the
setting's whole config and, per function, its cases: the arguments, what it
returned, and what it changed.
"""

from __future__ import annotations

import copy
import dataclasses
import json
import math
import sys
from dataclasses import replace
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

from econ_sim import (  # noqa: E402
    council, credit, economy, environment, events, farms, geography, healthcare, households, livestock, lords, market,
    metrics, migration, rules, town, towns, work,
)
from econ_sim.config import Config, ScheduledEvent, VillageConfig  # noqa: E402
from econ_sim.geography import RegionSettings, region  # noqa: E402
from econ_sim.simulation import Simulation  # noqa: E402
from econ_sim.world import ActiveEvent  # noqa: E402

FIXTURES = ROOT / "rust" / "fixtures"


def enc(x):
    """Plain JSON data: arrays as lists, infinities and not-a-numbers as strings."""
    if isinstance(x, np.ndarray):
        return [enc(v) for v in x.tolist()] if x.ndim else enc(x.item())
    if isinstance(x, (np.floating, float)):
        x = float(x)
        if math.isnan(x):
            return "nan"
        if math.isinf(x):
            return "inf" if x > 0 else "-inf"
        return x
    if isinstance(x, (np.integer,)):
        return int(x)
    if isinstance(x, (np.bool_,)):
        return bool(x)
    if isinstance(x, (list, tuple)):
        return [enc(v) for v in x]
    if isinstance(x, dict):
        return {str(k): enc(v) for k, v in x.items()}
    if dataclasses.is_dataclass(x) and not isinstance(x, type):
        return {f.name: enc(getattr(x, f.name)) for f in dataclasses.fields(x) if not f.name.startswith("_")}
    return x


class Recorder:
    """Cases by function. The state's people and families are written once
    (`state`); arguments that are them say "$population" or "$households"."""

    def __init__(self, population, households):
        self.cases: dict[str, list] = {}
        self.shared = {id(population): "$population", id(households): "$households"}
        self.state = {"population": enc(population), "households": enc(households)}

    def add(self, name: str, args: dict, out: dict) -> None:
        args = {k: self.shared.get(id(v), v) if not isinstance(v, (int, float, bool, str)) else v for k, v in args.items()}
        self.cases.setdefault(name, []).append({"args": enc(args), "out": enc(out)})


def settle_state(config: Config, months: int):
    sim = Simulation(config)
    sim.run(months)
    return sim


def by_village(values, loc, n):
    return np.bincount(loc, weights=values, minlength=n).astype(np.float64)


def record_state(rec: Recorder, sim: Simulation) -> None:
    """Call each deterministic function on this state, as the month does."""
    w, cfg = sim.world, sim.config
    pop, hh, n, geo = w.population, w.households, w.n_locations, w.geo
    n_hh = len(hh)
    food, farm = w.food, cfg.farming
    loc = hh.location
    rng = np.random.default_rng(7)  # (only to make up a few inputs that the state doesn't hold)

    # rules
    rec.add("rules.season_factors", {}, {"returned": rules.season_factors(cfg)})
    rec.add("rules.monthly_chance", {"annual": [0.0, 0.1, 0.28, 0.5, 0.99]},
            {"returned": rules.monthly_chance(np.array([0.0, 0.1, 0.28, 0.5, 0.99]))})
    rec.add("rules.food_need", {"population": pop}, {"returned": rules.food_need(pop, cfg)})
    rec.add("rules.spouse_alive", {"population": pop}, {"returned": rules.spouse_alive(pop)})
    need_rows = rules.food_need(pop, cfg)
    need = rules.by_location(need_rows, pop, n)
    outlook = np.abs(rng.normal(1.0, 0.5, size=(n, 11))) * need[:, None]
    stock = w.granary
    for most in (1.0, 2.0):
        rec.add("rules.plan_ration", {"stock": stock, "need": need, "outlook": outlook, "most": most},
                {"returned": rules.plan_ration(stock, need, outlook, cfg, most)})
        rec.add("rules.plan_ration_realistically", {"stock": stock * 0.3, "need": need, "outlook": outlook, "most": most},
                {"returned": rules.plan_ration_realistically(stock * 0.3, need, outlook, cfg, most=most)})
    rec.add("rules.stock_to_keep", {"need": need, "outlook": outlook}, {"returned": rules.stock_to_keep(need, outlook, cfg)})
    shares = np.linspace(0.0, 1.3, 14)
    rec.add("rules.target_health", {"food_share": shares}, {"returned": rules.target_health(shares, cfg)})
    rec.add("rules.expected_work_factor", {"food_share": shares}, {"returned": rules.expected_work_factor(shares, cfg)})
    family_share = np.clip(rng.normal(0.9, 0.2, size=len(pop)), 0.0, 1.0)
    delta = rng.normal(-1.0, 3.0, size=len(pop))
    p = copy.deepcopy(pop)
    rules.update_health(p, family_share, delta, cfg)
    rec.add("rules.update_health", {"population": pop, "food_share": family_share, "health_delta": delta},
            {"health": p.health})
    mult = np.abs(rng.normal(1.2, 0.4, size=n))
    care = np.where(rng.random(len(pop)) < 0.1, cfg.healthcare.treatment_mortality, 1.0)
    rec.add("rules.death_chance", {"population": pop, "mortality_mult": mult, "care": care},
            {"returned": rules.death_chance(pop, mult, cfg, care)})
    pay, price = np.abs(rng.normal(1.5, 0.5, n)), np.abs(rng.normal(1.0, 0.3, n))
    workers_v = np.where(rng.random(n) < 0.2, 0.0, np.abs(rng.normal(500, 100, n)))
    rec.add("rules.wage_cover", {"average_pay": pay, "food_price": price, "need": need, "workers": workers_v},
            {"returned": rules.wage_cover(pay, price, need, workers_v)})
    covers = np.linspace(0.5, 2.0, 9)
    rec.add("rules.marriage_factor", {"cover": covers}, {"returned": rules.marriage_factor(covers, cfg)})
    ev = np.abs(rng.normal(1.0, 0.2, size=(n, 11)))
    rec.add("rules.harvest_outlook", {"normal_capacity": need, "event_outlook": ev, "month_of_year": w.month_of_year},
            {"returned": rules.harvest_outlook(need, ev, w.month_of_year, cfg)})

    # economy
    force = economy.workforce(pop, n, cfg)
    workers = economy.headcount(pop, n, cfg, force)
    rec.add("economy.headcount", {"population": pop}, {"returned": workers})
    labor_now = economy.labor(pop, n, cfg, force=force)
    full = economy.labor(pop, n, cfg, at_full_health=True, force=force)
    rec.add("economy.labor", {"population": pop}, {"returned": labor_now, "full": full})
    rec.add("economy.tool_factor", {"tools": w.tools, "workers": workers},
            {"returned": economy.tool_factor(w.tools, workers, cfg)})
    herd = by_village(hh.animals, loc, n)
    animal_factor = livestock.farm_factor(herd, w.land, cfg)
    farm_mult = animal_factor * w.soil
    making = np.abs(rng.normal(1.0, 0.15, size=w.cash.shape))
    rec.add("economy.capacity", {"labor": labor_now, "tools": w.tools, "workers": workers, "land": w.land,
                                 "production_mult": making, "farm_mult": farm_mult, "sown": w.sown},
            {"returned": economy.capacity(labor_now, w.tools, workers, w.land, making, cfg, farm_mult, w.sown)})
    rec.add("economy.farm_land", {"land": w.land, "tools": w.tools, "workers": workers, "farm_mult": farm_mult},
            {"returned": economy.farm_land(w.land, w.tools, workers, cfg, farm_mult)})
    planned = economy.capacity(labor_now, w.tools, workers, w.land, making, cfg, farm_mult) * 1.3
    supplies = w.supplies.copy()
    made = economy.limit_by_supplies(planned, supplies, cfg)
    rec.add("economy.limit_by_supplies", {"planned": planned, "supplies": w.supplies}, {"returned": made, "supplies": supplies})
    per_worker = economy.productivity(pop, w.tools, workers, w.land, cfg, farm_mult, force=force)
    rec.add("economy.productivity", {"population": pop, "tools": w.tools, "workers": workers, "land": w.land,
                                     "farm_mult": farm_mult}, {"returned": per_worker})
    rec.add("economy.fair_prices", {"wage": w.wage_level, "per_worker": per_worker, "prices": w.prices},
            {"returned": economy.fair_prices(w.wage_level, per_worker, w.prices, cfg)})
    rec.add("economy.potential_pay", {"prices": w.prices, "per_worker": per_worker},
            {"returned": economy.potential_pay(w.prices, per_worker, cfg)})
    work_needed = np.abs(rng.normal(1.0, 0.5, size=workers.shape)) * workers
    rec.add("economy.wanted_workers", {"work_needed": work_needed, "workers": workers},
            {"returned": economy.wanted_workers(work_needed, workers, cfg)})

    # work
    for month in (w.month_of_year, cfg.work.harvest_months[0]):
        help_ = work.harvest_help(pop, n, n_hh, month, cfg, force)
        rec.add("work.harvest_help", {"population": pop, "n_households": n_hh, "month_of_year": month},
                {"farm": help_.farm, "kept": help_.kept, "households": help_.households})
    at_harvest = work.harvest_help(pop, n, n_hh, cfg.work.harvest_months[0], cfg, force).farm
    boost = work.harvest_boost(full[:, farm], at_harvest, cfg)
    rec.add("work.harvest_boost", {"farm_effort": full[:, farm], "help_effort": at_harvest}, {"returned": boost})
    rec.add("work.outlook_boost", {"boost": boost, "month_of_year": w.month_of_year, "months": 11},
            {"returned": work.outlook_boost(boost, w.month_of_year, 11, cfg)})
    for month in (w.month_of_year, 3):
        rec.add("work.home_cloth", {"population": pop, "n_households": n_hh, "month_of_year": month},
                {"returned": work.home_cloth(pop, n_hh, month, cfg)})
    members = market.by_household(pop.count.astype(np.float64), pop, n_hh)
    family_food = market.by_household(need_rows, pop, n_hh)
    short = family_food * rng.normal(0.3, 0.6, n_hh)
    spare = np.maximum(hh.grain - 2.0 * family_food, 0.0) * rng.random(n_hh)
    h2 = copy.deepcopy(hh)
    moved = work.kin_help(h2, members > 0, short, spare, cfg)
    rec.add("work.kin_help", {"households": hh, "alive": members > 0, "short": short, "spare": spare},
            {"returned": moved, "grain": h2.grain})

    # farms
    help_now = work.harvest_help(pop, n, n_hh, w.month_of_year, cfg, force)
    owners = livestock.owners_part(animal_factor)
    shares_hh, to_lord = farms.income_shares(pop, hh, w.land, cfg, owners, w.lord.land, help_now.households, force)
    rec.add("farms.income_shares", {"population": pop, "households": hh, "village_land": w.land, "animals_part": owners,
                                    "lord_land": w.lord.land, "extra_effort": help_now.households},
            {"shares": shares_hh, "to_lord": to_lord})
    farm_grain = np.abs(rng.normal(1.0, 0.5, n)) * need * 2
    keep_tools = np.abs(rng.normal(5.0, 2.0, n))
    h2, fg = copy.deepcopy(hh), farm_grain.copy()
    received, rent = farms.share_harvest(h2, fg, keep_tools, shares_hh, to_lord)
    rec.add("farms.share_harvest", {"households": hh, "farm_grain": farm_grain, "keep_for_tools": keep_tools,
                                    "shares": shares_hh, "to_lord": to_lord},
            {"received": received, "rent": rent, "farm_grain": fg, "grain": h2.grain})
    after = np.where(w.council.formed, 1.0 - cfg.council.grain_levy, 1.0)
    plan = farms.plan_family_food(hh, family_food, outlook, shares_hh, after, cfg, w.markup[:, food])
    rec.add("farms.plan_family_food", {"households": hh, "need": family_food, "village_outlook": outlook,
                                       "shares": shares_hh, "after_deductions": after, "markup": w.markup[:, food]},
            {"own": plan.own, "spare": plan.spare, "want": plan.want})
    sold_v = by_village(plan.spare, loc, n) * rng.random(n) + farm_grain * rng.random(n)
    lord_offer = lords.offer(w.lord, cfg)
    brought = np.abs(rng.normal(10.0, 5.0, n))
    s = farms.sellers_share(sold_v, plan.spare, farm_grain, loc, lord_offer, brought)
    rec.add("farms.sellers_share", {"sold": sold_v, "offered_by_families": plan.spare, "farm_offer": farm_grain,
                                    "location": loc, "lord_offer": lord_offer, "imports": brought},
            {"families": s.families, "farms": s.farms, "lord": s.lord, "imports": s.imports})
    for m in range(1, 13):
        rec.add("farms.seed_plan", {"month_of_year": m}, {"returned": farms.seed_plan(m, cfg)})
        rec.add("farms.seed_gathered", {"month_of_year": m}, {"returned": farms.seed_gathered(m, cfg)})
    farmed = economy.farm_land(w.land, w.tools, workers, cfg, farm_mult)
    needed = farms.seed_needed(farmed, cfg)
    produced = np.abs(rng.normal(1.0, 0.5, n)) * need * 2
    seed0 = needed * rng.random(n) * 0.5
    for m in cfg.food.seed_months:
        fg, sd = farm_grain.copy(), seed0.copy()
        kept = farms.keep_seed(fg, sd, needed, produced, m, cfg)
        rec.add("farms.keep_seed", {"farm_grain": farm_grain, "seed": seed0, "needed": needed, "harvest": produced,
                                    "month_of_year": m}, {"returned": kept, "farm_grain": fg, "seed": sd})
    rec.add("farms.seed_outlook", {"needed": needed, "month_of_year": w.month_of_year, "months": 11},
            {"returned": farms.seed_outlook(needed, w.month_of_year, 11, cfg)})
    sown = np.clip(rng.normal(0.9, 0.1, n), 0.0, 1.0)
    for m in (1, 7, 10, 12):
        rec.add("farms.sown_outlook", {"sown": sown, "month_of_year": m, "months": 11},
                {"returned": farms.sown_outlook(sown, m, 11, cfg)})
    h2, sd = copy.deepcopy(hh), seed0.copy()
    sown_out, given = farms.sow(h2, sd, needed, family_food)
    rec.add("farms.sow", {"households": hh, "seed": seed0, "needed": needed, "need": family_food},
            {"sown": sown_out, "given": given, "grain": h2.grain, "seed_after": sd})

    # market
    price_hh = w.food_price[loc]
    essentials = plan.want * price_hh + members * 0.3 * w.prices[loc, cfg.product_for("heating")]
    money = hh.money.copy()
    pot = market.share_with_neighbours(money, essentials, loc, n, cfg)
    rec.add("market.share_with_neighbours", {"money": hh.money, "cost": essentials, "location": loc, "n": n},
            {"returned": pot, "money": money})
    offer = by_village(plan.want, loc, n) * rng.random(n) * 1.5
    money = hh.money.copy()
    sale = market.buy(money, plan.want, w.food_price, offer, loc)
    rec.add("market.buy", {"money": hh.money, "want": plan.want, "price": w.food_price, "offer": offer, "location": loc},
            {"bought": sale.bought, "spent": sale.spent, "sold": sale.sold, "demand": sale.demand, "money": money})
    demand = np.abs(rng.normal(1.0, 0.5, w.stock.shape)) * np.where(rng.random(w.stock.shape) < 0.1, 0.0, 1.0)
    supply = np.abs(rng.normal(1.0, 0.5, w.stock.shape)) * np.where(rng.random(w.stock.shape) < 0.1, 0.0, 1.0)
    rec.add("market.adjust_markup", {"markup": w.markup, "demand": demand, "supply": supply},
            {"returned": market.adjust_markup(w.markup, demand, supply, cfg)})
    rec.add("market.seasonal_buffer", {"profile": cfg.needs.firewood}, {"returned": market.seasonal_buffer(cfg.needs.firewood)})
    cover = np.abs(rng.normal(1.2, 0.4, n))
    rec.add("market.grain_markup", {"markup": w.markup[:, food], "cover": cover},
            {"returned": market.grain_markup(w.markup[:, food], cover, cfg)})
    keep = np.abs(rng.normal(1.0, 1.0, w.cash.shape))
    cash = w.cash.copy()
    income, payout = market.pay_wages(cash, pop, n_hh, cfg, keep=keep, force=force)
    rec.add("market.pay_wages", {"cash": w.cash, "population": pop, "n_households": n_hh, "keep": keep},
            {"income": income, "payout": payout, "cash": cash})

    # credit
    worth = cfg.livestock.value_months * w.wage_level
    normal_gross = economy.capacity(full, w.tools, workers, w.land, np.ones_like(full), cfg, farm_mult)[:, farm]
    yearly_net = np.maximum(12.0 * normal_gross - needed, 0.0)
    land_part = (1.0 - cfg.food.labor_share) * (1.0 - owners)
    normal_food_price = w.food_price / np.maximum(w.markup[:, food], 1e-9)
    rent_per_plot = np.divide(land_part * yearly_net, w.land, out=np.zeros(n), where=w.land > 0) * normal_food_price
    plot_price = credit.land_price(rent_per_plot, cfg)
    rec.add("credit.land_price", {"rent_per_plot": rent_per_plot}, {"returned": plot_price})
    rec.add("credit.collateral", {"households": hh, "plot_price": plot_price, "animal_price": worth},
            {"returned": credit.collateral(hh, plot_price, worth)})
    capacity = np.where(rng.random(n_hh) < 0.3, 0.0, np.abs(rng.normal(5.0, 4.0, n_hh)))
    totals = np.where(rng.random(n) < 0.2, 0.0, np.abs(rng.normal(30.0, 20.0, n)))
    rec.add("credit.biggest_first", {"capacity": capacity, "totals": totals, "location": loc},
            {"returned": credit._biggest_first(capacity, totals, loc)})
    rec.add("credit.pro_rata", {"weights": capacity, "totals": totals, "location": loc},
            {"returned": credit._pro_rata(capacity, totals, loc)})
    coin_target = np.maximum(3.0 * family_food - hh.grain, 0.0) * price_hh
    food_cost = plan.want * price_hh
    h_debt = copy.deepcopy(hh)
    # give some families debts and claims to work on, balanced in each village
    owed = np.where(rng.random(n_hh) < 0.25, np.abs(rng.normal(20.0, 15.0, n_hh)), 0.0)
    claims = np.where((owed == 0) & (rng.random(n_hh) < 0.3), rng.random(n_hh), 0.0)
    claims = credit._pro_rata(claims, by_village(owed, loc, n), loc)
    h_debt.debt = hh.debt + owed
    h_debt.lent = hh.lent + claims
    h2 = copy.deepcopy(h_debt)
    interest = credit.accrue_interest(h2, cfg)
    rec.add("credit.accrue_interest", {"households": h_debt}, {"returned": interest, "households": h2})
    personal = cfg.credit.personal_months * w.wage_level[loc]
    worth_hh = credit.collateral(h_debt, plot_price, worth)
    h2 = copy.deepcopy(h_debt)
    lent = credit.borrow(h2, np.maximum(food_cost * 3 - h2.money, 0.0), h2.money - coin_target, worth_hh, personal, cfg)
    rec.add("credit.borrow", {"households": h_debt, "short": np.maximum(food_cost * 3 - h_debt.money, 0.0),
                              "spare": h_debt.money - coin_target, "worth": worth_hh, "personal": personal, "n": n},
            {"returned": lent, "households": h2})
    inc = np.abs(rng.normal(2.0, 1.0, n_hh))
    h2 = copy.deepcopy(h_debt)
    repaid = credit.repay(h2, inc, cfg, spare=h2.money - coin_target)
    rec.add("credit.repay", {"households": h_debt, "income": inc, "spare": h_debt.money - coin_target, "n": n},
            {"returned": repaid, "households": h2})
    h2 = copy.deepcopy(h_debt)
    rations = credit.repay_in_grain(h2, family_food, w.food_price, cfg)
    rec.add("credit.repay_in_grain", {"households": h_debt, "need": family_food, "food_price": w.food_price},
            {"returned": rations, "households": h2})
    h3 = copy.deepcopy(h_debt)
    h3.debt = h3.debt * 8.0
    h3.lent = h3.lent * 8.0
    h2 = copy.deepcopy(h3)
    f = credit.foreclose(h2, plot_price, worth, cfg)
    rec.add("credit.foreclose", {"households": h3, "plot_price": plot_price, "animal_price": worth},
            {"animals": f.animals, "land": f.land, "value": f.value, "households": h2})
    limit = cfg.credit.loan_to_value * credit.collateral(h3, plot_price, worth) + personal
    h2 = copy.deepcopy(h3)
    lost = credit.default(h2, limit * 0.1, cfg)
    rec.add("credit.default", {"households": h3, "limit": limit * 0.1, "n": n}, {"returned": lost, "households": h2})
    h2 = copy.deepcopy(h_debt)
    ls = credit.sell_land(h2, np.maximum(food_cost * 5 - h2.money, 0.0), h2.money - coin_target, plot_price, cfg)
    rec.add("credit.sell_land", {"households": h_debt, "short": np.maximum(food_cost * 5 - h_debt.money, 0.0),
                                 "spare": h_debt.money - coin_target, "worth": plot_price},
            {"plots": ls.plots, "price": ls.price, "households": h2})
    share = np.where(rng.random(n) < 0.5, 0.0, rng.random(n))
    h2 = copy.deepcopy(h_debt)
    c = credit.cancel_debts(h2, share)
    rec.add("credit.cancel_debts", {"households": h_debt, "share": share}, {"returned": c, "households": h2})
    gone = rng.random(n_hh) < 0.1
    h2 = copy.deepcopy(h_debt)
    credit.write_off(h2, gone)
    rec.add("credit.write_off", {"households": h_debt, "gone": gone, "n": n}, {"households": h2})

    # livestock
    rec.add("livestock.farm_factor", {"animals": herd, "land": w.land}, {"returned": animal_factor})
    rec.add("livestock.owners_part", {"factor": animal_factor}, {"returned": owners})
    weather = np.clip(rng.normal(0.9, 0.2, n), 0.3, 1.3)
    heating_mult = np.where(rng.random(n) < 0.3, 1.5, 1.0)
    h2 = copy.deepcopy(hh)
    gd = livestock.grow_and_die(h2, weather, heating_mult, cfg)
    rec.add("livestock.grow_and_die", {"households": hh, "farm_weather": weather, "heating_mult": heating_mult},
            {"returned": gd, "animals": h2.animals})
    h2 = copy.deepcopy(hh)
    culled, meat = livestock.winter_cull(h2, w.land * 0.2, cfg, geo.pasture)
    rec.add("livestock.winter_cull", {"households": hh, "village_land": w.land * 0.2, "pasture": geo.pasture},
            {"culled": culled, "meat": meat, "households": h2})
    h_poor = copy.deepcopy(hh)
    h_poor.grain = hh.grain * rng.random(n_hh) * 0.2
    h_poor.money = hh.money * rng.random(n_hh) * 0.2
    h2 = copy.deepcopy(h_poor)
    killed, meat = livestock.slaughter_in_hunger(h2, family_food, w.food_price, cfg)
    rec.add("livestock.slaughter_in_hunger", {"households": h_poor, "need": family_food, "food_price": w.food_price},
            {"killed": killed, "meat": meat, "households": h2})
    h2 = copy.deepcopy(h_poor)
    sales = livestock.distress_sales(h2, np.maximum(essentials * 3 - h2.money, 0.0), h2.money - coin_target * 0.2, worth, cfg)
    rec.add("livestock.distress_sales", {"households": h_poor, "short": np.maximum(essentials * 3 - h_poor.money, 0.0),
                                         "spare": h_poor.money - coin_target * 0.2, "worth": worth},
            {"sold": sales.sold, "price": sales.price, "households": h2})
    wool_worth = np.abs(rng.normal(2.0, 0.5, n))
    rec.add("livestock.shear", {"households": hh, "worth": wool_worth}, {"returned": livestock.shear(hh, wool_worth)})

    # lords
    l2 = copy.deepcopy(w.lord)
    l2.barn = l2.barn + np.abs(rng.normal(50.0, 20.0, n))
    l2.purse = l2.purse + np.abs(rng.normal(30.0, 10.0, n))
    l3 = copy.deepcopy(l2)
    away = lords.cart_away(l3, cfg)
    rec.add("lords.cart_away", {"lords": l2}, {"returned": away, "lords": l3})
    rec.add("lords.offer", {"lords": l2}, {"returned": lords.offer(l2, cfg)})
    for in_town in (False, True):
        rec.add("lords.local_spending", {"lords": l2, "in_town": in_town},
                {"returned": lords.local_spending(l2, cfg, in_town)})
    l3 = copy.deepcopy(l2)
    paid = lords.pay_servants(l3, geo.market, loc, members, cfg)
    rec.add("lords.pay_servants", {"lords": l2, "market": geo.market, "households_location": loc, "members": members},
            {"returned": paid, "lords": l3})
    l3 = copy.deepcopy(l2)
    dg, dc = w.dealers_grain.copy(), w.dealers_cash.copy()
    promised = np.abs(rng.normal(10.0, 10.0, n)) * geo.town
    carted = away * cfg.lord.to_town
    sold_t = lords.sell_in_town(l3, carted, geo.market, dg, dc, w.food_price, promised)
    rec.add("lords.sell_in_town", {"lords": l2, "carted": carted, "market": geo.market, "dealers_grain": w.dealers_grain,
                                   "dealers_cash": w.dealers_cash, "town_price": w.food_price, "promised": promised},
            {"returned": sold_t, "lords": l3, "dealers_grain": dg, "dealers_cash": dc})
    famine = rng.random(n) < 0.5
    gap = family_food * rng.random(n_hh) * 0.3
    for charity in (False, True):
        c2 = replace(cfg, lord=replace(cfg.lord, charity_in_famine=charity))
        l3 = copy.deepcopy(l2)
        given = lords.charity(l3, gap, loc, famine, c2)
        rec.add("lords.charity", {"lords": l2, "gap": gap, "location": loc, "famine": famine, "charity": charity},
                {"returned": given, "lords": l3})
    for remit in (False, True):
        c2 = replace(cfg, state=replace(cfg.state, remit_in_famine=remit))
        h2, l3 = copy.deepcopy(hh), copy.deepcopy(l2)
        take = lords.collect_state_tax(h2, l3, w.wage_level, rent_per_plot, w.food_price, famine, c2, members > 0,
                                       keep=coin_target)
        rec.add("lords.collect_state_tax", {"households": hh, "lords": l2, "wage": w.wage_level,
                                            "rent_per_plot": rent_per_plot, "food_price": w.food_price, "famine": famine,
                                            "living": members > 0, "keep": coin_target, "remit": remit},
                {"coins": take.coins, "grain": take.grain, "households": h2, "lords": l3})
    share = np.where(rng.random(n) < 0.5, 0.0, rng.uniform(0.1, 0.4, n))
    h2, l3, fg = copy.deepcopy(hh), copy.deepcopy(l2), farm_grain.copy()
    taken, from_barn = lords.requisition(h2, l3, fg, share)
    rec.add("lords.requisition", {"households": hh, "lords": l2, "farm_grain": farm_grain, "share": share},
            {"taken": taken, "from_barn": from_barn, "households": h2, "lords": l3, "farm_grain_after": fg})

    # council
    c2 = copy.deepcopy(w.council)
    fg = farm_grain.copy()
    taken = council.levy_grain(c2, produced, fg, need, famine, cfg)
    rec.add("council.levy_grain", {"councils": w.council, "produced": produced, "granary": farm_grain, "need": need,
                                   "famine": famine}, {"returned": taken, "councils": c2, "granary_after": fg})
    c2 = copy.deepcopy(w.council)
    given = council.give_relief(c2, plan.own, family_food, loc, cfg)
    rec.add("council.give_relief", {"councils": w.council, "bought": plan.own, "need": family_food, "location": loc},
            {"returned": given, "councils": c2})
    c2 = copy.deepcopy(w.council)
    money = hh.money.copy()
    shortfall = np.maximum(essentials * 2 - money, 0.0)
    keep_v = np.abs(rng.normal(20.0, 10.0, n))
    given = council.cash_relief(c2, money, shortfall, loc, keep_v, cfg)
    rec.add("council.cash_relief", {"councils": w.council, "money": hh.money, "shortfall": shortfall, "location": loc,
                                    "keep": keep_v}, {"returned": given, "councils": c2, "money": money})
    c2 = copy.deepcopy(w.council)
    inc = income.copy()
    target = np.abs(rng.normal(150.0, 80.0, n))
    taxes = council.collect_taxes(inc, loc, c2, target, famine, cfg)
    rec.add("council.collect_taxes", {"income": income, "location": loc, "councils": w.council, "target": target,
                                      "famine": famine}, {"returned": taxes, "councils": c2, "income_after": inc})
    for job in (council.official_job(cfg), healthcare.healer_job(cfg)):
        c2 = copy.deepcopy(w.council)
        pay_v = np.abs(rng.normal(1.5, 0.3, n))
        inc, paid = council.pay_staff(pop, c2, job, pay_v, n_hh)
        rec.add("council.pay_staff", {"population": pop, "councils": w.council, "job": job, "pay": pay_v,
                                      "n_households": n_hh}, {"income": inc, "paid": paid, "councils": c2})
    people = rules.by_location(pop.count.astype(np.float64), pop, n)
    c2 = copy.deepcopy(w.council)
    c2.formed = np.zeros(n, dtype=bool)
    c3 = copy.deepcopy(c2)
    c3.months_ready = np.full(n, 5)
    new = council.check_formation(c3, people, cfg)
    rec.add("council.check_formation", {"councils": c2, "months_ready": 5, "people": people},
            {"returned": new, "councils": c3})

    # environment
    sizes_v = np.array([v.population for v in cfg.villages], dtype=np.float64)
    rec.add("environment.size_woods", {"people": sizes_v, "richness": geo.woods},
            {"returned": environment.size_woods(sizes_v, cfg, geo.woods)})
    woods = w.woods * rng.uniform(0.3, 2.0, n)
    rec.add("environment.woods_reach", {"woods": woods, "normal": w.woods_normal},
            {"returned": environment.woods_reach(woods, w.woods_normal, cfg)})
    cut = np.abs(rng.normal(1.0, 0.5, n)) * people * 0.3
    burned = np.where(rng.random(n) < 0.3, 0.03, 0.0)
    wd = woods.copy()
    lost = environment.grow_woods(wd, w.woods_capacity, cut, burned, cfg)
    rec.add("environment.grow_woods", {"woods": woods, "capacity": w.woods_capacity, "cut": cut, "burned": burned},
            {"returned": lost, "woods": wd})
    usual_land = geo.usual_land(w.land)
    rec.add("environment.soil_target", {"people": people, "land": usual_land},
            {"returned": environment.soil_target(people, usual_land, cfg)})
    soil = w.soil * rng.uniform(0.8, 1.1, n)
    sl = soil.copy()
    environment.tire_soil(sl, people, usual_land, cfg)
    rec.add("environment.tire_soil", {"soil": soil, "people": people, "land": usual_land}, {"soil": sl})

    # town
    farm_weather = np.clip(rng.normal(0.9, 0.2, n), 0.1, 1.3)
    rec.add("town.price", {"town": w.town, "month_of_year": w.month_of_year, "farm_weather": farm_weather},
            {"returned": town.price(w.town, w.month_of_year, farm_weather, cfg)})
    tp = town.price(w.town, w.month_of_year, farm_weather, cfg)
    vp = tp * rng.uniform(0.5, 1.5, n)
    m = town.merchants(vp, tp, need, cfg, geo.transport)
    rec.add("town.merchants", {"village_price": vp, "town_price": tp, "need": need, "transport": geo.transport},
            {"exports": m.exports, "imports": m.imports})
    t2, money = copy.deepcopy(w.town), hh.money.copy()
    lost = town.lose_coins(money, t2, cfg)
    rec.add("town.lose_coins", {"money": hh.money, "town": w.town}, {"returned": lost, "money": money, "town": t2})

    # migration
    rec.add("migration.appeal", {"prospects": cover, "memory": w.memory, "people": people, "land": usual_land,
                                 "lord_take": to_lord}, {"returned": migration.appeal(cover, w.memory, people, usual_land,
                                                                                      to_lord, cfg)})
    m2 = copy.deepcopy(w.memory)
    ration = np.clip(rng.normal(0.95, 0.1, n), 0.0, 1.0)
    died = np.round(people * rng.uniform(0.0, 0.02, n))
    plundered = np.where(rng.random(n) < 0.2, 0.2, 0.0)
    migration.remember(m2, ration, died, people, plundered, cfg, mult)
    rec.add("migration.remember", {"memory": w.memory, "ration": ration, "deaths": died, "people": people,
                                   "plundered": plundered, "mortality_mult": mult}, {"memory": m2})
    m3 = copy.deepcopy(m2)
    left = np.round(people * rng.uniform(0.0, 0.01, n))
    migration.remember_leaving(m3, left, people)
    rec.add("migration.remember_leaving", {"memory": m2, "left": left, "people": people}, {"memory": m3})

    # events
    for spec in cfg.events:
        for repeated in (False, True):
            rec.add("events.chance_now", {"name": spec.name, "repeated": repeated},
                    {"returned": events.chance_now(spec, repeated)})
    w2 = copy.deepcopy(w)
    for i, spec in enumerate(cfg.events):
        if spec.scope != "location":
            continue
        place = i % n
        effects = {k: (v if not isinstance(v, tuple) else float(np.mean(v))) for k, v in spec.effects.items()}
        w2.active_events.append(ActiveEvent(spec=spec, location=place, months_left=1 + i % 7, effects=effects))
    names = tuple(b.name for b in cfg.businesses)
    mods = events.modifiers(w2, names)
    active = [{"spec": cfg.events.index(e.spec), "location": e.location, "months_left": e.months_left,
               "effects": e.effects} for e in w2.active_events]
    rec.add("events.modifiers", {"active": active}, {"returned": mods})
    rec.add("events.production_outlook", {"active": active, "months": 11, "business": names[farm]},
            {"returned": events.production_outlook(w2, 11, names[farm])})

    # households
    size = households.sizes(pop, n_hh)
    rec.add("households.sizes", {"population": pop, "n_households": n_hh}, {"returned": size})
    gone_size = np.where(rng.random(n_hh) < 0.1, 0, size)
    h2, reserve = copy.deepcopy(hh), w.council.reserve.copy()
    to_reserve = households.pass_on_land_and_grain(h2, gone_size, reserve, w.council.formed)
    rec.add("households.pass_on_land_and_grain", {"households": hh, "size": gone_size, "reserve": w.council.reserve,
                                                  "council": w.council.formed},
            {"returned": to_reserve, "households": h2, "reserve_after": reserve})
    no_council = np.zeros(n, dtype=bool)
    h2, reserve = copy.deepcopy(hh), w.council.reserve.copy()
    to_reserve = households.pass_on_land_and_grain(h2, gone_size, reserve, no_council)
    rec.add("households.pass_on_land_and_grain", {"households": hh, "size": gone_size, "reserve": w.council.reserve,
                                                  "council": no_council},
            {"returned": to_reserve, "households": h2, "reserve_after": reserve})
    for formed in (w.council.formed, no_council):
        h2, treasury = copy.deepcopy(hh), w.council.treasury.copy()
        households.pass_on_savings(h2, gone_size, treasury, formed)
        rec.add("households.pass_on_savings", {"households": hh, "size": gone_size, "treasury": w.council.treasury,
                                               "council": formed}, {"households": h2, "treasury_after": treasury})

    # metrics
    wealth = hh.money + hh.grain * w.food_price[loc]
    eaten = family_food * np.clip(rng.normal(0.95, 0.1, n_hh), 0.0, 1.0)
    rec.add("metrics.poorest_fifth_ration", {"money": wealth, "size": size, "bought": eaten, "need": family_food},
            {"returned": metrics.poorest_fifth_ration(wealth, size, eaten, family_food)})
    rec.add("metrics.age_groups", {"population": pop}, {"returned": list(metrics.age_groups(pop, cfg))})
    rec.add("metrics.average_health", {"population": pop}, {"returned": metrics.average_health(pop)})

    # healthcare
    risk = rules.death_chance(pop, mult, cfg)
    healers = np.where(w.council.formed, np.round(people * 0.005), 0.0)
    p2 = copy.deepcopy(pop)
    care, treated = healthcare.treat(p2, healers, risk, cfg)
    rec.add("healthcare.treat", {"population": pop, "healers": healers, "risk": risk},
            {"care": care, "treated": treated, "population": p2})

    # towns (on a map with towns)
    if geo.has_towns:
        rec.add("towns.gets", {}, {"returned": towns.gets(geo)})
        rec.add("towns.store_markup", {"markup": w.markup[:, food], "store": w.dealers_grain, "need": need},
                {"returned": towns.store_markup(w.markup[:, food], w.dealers_grain, need, cfg)})
        rec.add("towns.from_neighbours", {"previous": w.food_price}, {"returned": towns.from_neighbours(w.food_price, geo)})
        fair = w.food_price * rng.uniform(0.8, 1.2, n)
        rec.add("towns.delivered_price", {"village_price": fair, "weights": people},
                {"returned": towns.delivered_price(fair, geo, people)})
        wanted = w.orders.copy()
        wanted[:, food] = need
        in_store = w.stock.copy()
        in_store[:, food] = w.dealers_grain
        prices = w.prices * rng.uniform(0.7, 1.3, w.prices.shape)
        cart = towns.carts(prices, geo, wanted, in_store, w.dealers_cash, need, food, cfg)
        rec.add("towns.carts", {"prices": prices, "wanted": wanted, "stock": in_store, "dealers_cash": w.dealers_cash,
                                "weights": need, "food": food}, {"returned": cart})
        for product in range(len(cfg.products)):
            if product == food:
                continue
            offer = w.stock[:, product] + cart.incoming(n)[:, product]
            stock_p = w.stock[:, product].copy()
            cash_p = w.cash[:, economy.seller_of(cfg)[product]].copy()
            demand_p = np.zeros(n)
            sold_p = offer * rng.uniform(0.0, 1.0, n)
            spent_p = sold_p * w.prices[:, product]
            asked_p = sold_p * rng.uniform(1.0, 1.5, n)
            towns.settle(cart, product, stock_p, cash_p, demand_p, offer, sold_p, spent_p, asked_p)
            rec.add("towns.settle", {"cart": cart, "product": product, "stock": w.stock[:, product],
                                     "cash": w.cash[:, economy.seller_of(cfg)[product]], "offer": offer, "sold": sold_p,
                                     "spent": spent_p, "asked": asked_p},
                    {"stock": stock_p, "cash": cash_p, "demand": demand_p})
        for scale in (0.6, 1.0, 1.6):
            dg, dc = w.dealers_grain.copy(), w.dealers_cash.copy()
            price = w.food_price * np.where(geo.town, rng.uniform(0.7, 1.4, n), 1.0)
            moved = towns.between_towns(dg, dc, price, need, geo, cfg)
            rec.add("towns.between_towns", {"dealers_grain": w.dealers_grain, "dealers_cash": w.dealers_cash,
                                            "price": price, "need": need},
                    {"returned": moved, "dealers_grain": dg, "dealers_cash": dc})
            dg, dc = w.dealers_grain.copy(), w.dealers_cash.copy()
            outside = tp * scale
            moved = towns.border_trade(dg, dc, w.food_price, outside, need, geo, cfg)
            rec.add("towns.border_trade", {"dealers_grain": w.dealers_grain, "dealers_cash": w.dealers_cash,
                                           "town_price": w.food_price, "outside": outside, "need": need},
                    {"returned": moved, "dealers_grain": dg, "dealers_cash": dc,
                     "coins": towns.outside_coins(moved, outside, cfg)})
        dc = w.dealers_cash.copy() * 3
        paid = towns.dealers_payout(dc, need, w.food_price, cfg)
        rec.add("towns.dealers_payout", {"dealers_cash": w.dealers_cash * 3, "need": need, "price": w.food_price},
                {"returned": paid, "dealers_cash": dc})


# A small region with three towns (one a city) for the states: big enough
# for routes between towns, small enough to keep the files small.
SMALL_REGION = RegionSettings(villages=16, mean_size=150, towns=3, seed=3)

SETTINGS = {
    # (config, months to run before taking the state)
    "village_harvest": (replace(Config(), seed=3), 8),
    "village_famine": (replace(Config(), seed=5, scheduled_events=(ScheduledEvent("drought", 4), ScheduledEvent("drought", 16))), 20),
    "hamlet": (replace(Config(), seed=11, villages=(VillageConfig(population=20, land=7),)), 30),
    "city_june": (replace(region(SMALL_REGION, Config()), seed=2), 5),
    "city_later": (replace(region(SMALL_REGION, Config()), seed=4,
                           scheduled_events=(ScheduledEvent("drought", 4, near=(5.0, 5.0, 6.0)),)), 16),
}


def geography_file() -> dict:
    """The map built from each exported region (and the small region)."""
    sys.path.insert(0, str(ROOT / "scripts"))
    from export_config import REGIONS

    maps = {name: region(settings, Config()) for name, settings in REGIONS.items()}
    maps["small_region"] = region(SMALL_REGION, Config())
    z = np.linspace(-6.0, 6.0, 49)
    return {
        "maps": {name: {"config": json.loads(json.dumps(dataclasses.asdict(config))),
                        "geography": enc(geography.build(config))} for name, config in maps.items()},
        "normal_share": {"z": enc(z), "returned": enc(geography.normal_share(z))},
    }


def main() -> int:
    FIXTURES.mkdir(parents=True, exist_ok=True)
    path = FIXTURES / "geography.json"
    path.write_text(json.dumps(geography_file(), separators=(",", ":")) + "\n")
    print(f"{path.name}: {path.stat().st_size // 1024} KB")
    for name, (config, months) in SETTINGS.items():
        sim = settle_state(config, months)
        rec = Recorder(sim.world.population, sim.world.households)
        record_state(rec, sim)
        data = {"config": json.loads(json.dumps(dataclasses.asdict(config))), "month": sim.world.month,
                "state": rec.state, "cases": rec.cases}
        path = FIXTURES / f"{name}.json"
        path.write_text(json.dumps(data, separators=(",", ":")) + "\n")
        print(f"{path.name}: {sum(len(v) for v in rec.cases.values())} cases of {len(rec.cases)} functions, "
              f"{path.stat().st_size // 1024} KB")
    return 0


if __name__ == "__main__":
    sys.exit(main())
