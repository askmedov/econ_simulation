from dataclasses import replace

import numpy as np

from econ_sim import migration, town
from econ_sim.config import Config, MigrationConfig, ScheduledEvent, TownConfig
from econ_sim.households import Households
from econ_sim.population import NO_JOB, Population
from econ_sim.simulation import Simulation
from econ_sim.town import Town

CONFIG = Config()


def people(ages, household, married=None, female=None):
    n = len(ages)
    return Population(
        count=np.ones(n, dtype=np.int64),
        age_months=np.asarray(ages) * 12,
        female=np.zeros(n, dtype=bool) if female is None else np.asarray(female),
        health=np.full(n, 100.0),
        skill=np.ones(n),
        location=np.zeros(n, dtype=np.int32),
        household=np.asarray(household),
        job=np.full(n, NO_JOB),
        married=np.zeros(n, dtype=bool) if married is None else np.asarray(married),
    )


def test_the_town_price_is_dearest_before_harvest_and_in_a_dearth():
    t = Town.at(2.0)
    normal = np.ones(1)
    june = town.price(t, CONFIG.town.dearest_month, normal, CONFIG)[0]
    december = town.price(t, CONFIG.town.dearest_month + 6, normal, CONFIG)[0]
    assert np.isclose(june, 2.0 * (1 + CONFIG.town.seasonal)) and np.isclose(december, 2.0 * (1 - CONFIG.town.seasonal))
    dearth = town.price(t, CONFIG.town.dearest_month, np.array([0.5]), CONFIG)[0]
    assert np.isclose(dearth, june * (1 + CONFIG.town.regional_share))


def test_merchants_carry_grain_only_across_the_transport_gap():
    need = np.full(3, 100.0)
    town_price = np.full(3, 1.0)
    carried = town.merchants(np.array([0.5, 1.0, 2.0]), town_price, need, CONFIG)
    cart = CONFIG.town.capacity * 100.0
    assert carried.exports.tolist() == [cart, 0.0, 0.0]
    assert carried.imports.tolist() == [0.0, 0.0, cart]
    off = town.merchants(np.array([0.5, 1.0, 2.0]), town_price, need, replace(CONFIG, town=TownConfig(enabled=False)))
    assert not off.exports.any() and not off.imports.any()


def test_coins_are_lost_a_little_each_year():
    money = np.full(4, 100.0)
    t = Town.at(1.0)
    for _ in range(12):
        town.lose_coins(money, t, CONFIG)
    assert np.allclose(money, 100.0 * (1 - CONFIG.town.coins_lost_a_year))
    assert np.isclose(t.coins_lost + money.sum(), 400.0)


def test_young_singles_leave_with_their_share_of_coins_and_grain():
    config = replace(CONFIG, migration=MigrationConfig(leave_chance=1.0))
    pop = people([40, 38, 20, 10], [0, 0, 0, 0], married=[True, True, False, False], female=[False, True, False, False])
    hh = Households(np.zeros(1), money=np.array([40.0]), grain=np.array([80.0]))
    moves = migration.leave(pop, hh, np.array([1.5]), np.ones(1), config, np.random.default_rng(0))
    assert moves.left[0] == 1 and pop.size == 3 and pop.count[pop.age_years == 20].sum() == 0  # dropped at month end
    assert np.isclose(moves.coins, 10.0) and np.isclose(moves.grain, 20.0)
    assert np.isclose(hh.money[0], 30.0) and np.isclose(hh.grain[0], 60.0)


def test_starving_families_flee_together():
    config = replace(CONFIG, migration=MigrationConfig(leave_chance=0.0, flee_chance=1.0))
    pop = people([40, 38, 12, 50], [0, 0, 0, 1], married=[True, True, False, True])
    hh = Households(np.zeros(2), grain=np.array([5.0, 50.0]), land=np.array([3.0, 3.0]))
    moves = migration.leave(pop, hh, np.array([1.0]), np.array([0.3, 0.9]), config, np.random.default_rng(0))
    assert moves.left[0] == 3 and pop.count[pop.household == 0].sum() == 0 and pop.size == 1
    assert hh.grain[0] == 0 and hh.land[0] == 3.0  # the land stays behind


def test_servants_come_to_landholders_when_hands_are_short():
    config = replace(CONFIG, migration=MigrationConfig(arrive_rate=1000.0))
    pop = people([40, 40], [0, 1], married=[True, True])
    hh = Households(np.zeros(2), land=np.array([0.0, 5.0]))
    arrived = migration.arrive(pop, hh, np.array([1.0]), config, np.random.default_rng(0))
    assert arrived[0] == 0  # a wage barely feeds a family: nobody comes
    arrived = migration.arrive(pop, hh, np.array([2.0]), config, np.random.default_rng(0))
    assert arrived[0] > 0 and pop.size == 2 + arrived[0]
    newcomers = pop.household[2:]
    assert (newcomers == 1).all() and (pop.age_years[2:] < 30).all()


def test_a_debased_coinage_raises_prices_in_coins():
    config = Config(seed=1, months=48, random_events=False)
    debased = replace(config, scheduled_events=(ScheduledEvent("debasement", 3),))
    before = Simulation(config).run()
    after = Simulation(debased).run()
    assert np.isclose(after[-1].town_price, before[-1].town_price / (1 - 0.25))
    late = slice(36, 48)
    assert np.mean([r.food_price for r in after[late]]) > np.mean([r.food_price for r in before[late]])


def test_a_wage_that_buys_plenty_draws_only_so_many_newcomers():
    cfg = CONFIG.migration
    pop = people([40] * 100, list(range(100)), married=[True] * 100)
    hh = Households(np.zeros(100), land=np.ones(100))
    at_cap = cfg.welcome_cover + cfg.max_pull
    capped = migration.arrive(pop.select(np.arange(100)), hh, np.array([at_cap]), CONFIG, np.random.default_rng(3))
    beyond = migration.arrive(pop.select(np.arange(100)), hh, np.array([at_cap + 5.0]), CONFIG, np.random.default_rng(3))
    assert capped[0] == beyond[0]
