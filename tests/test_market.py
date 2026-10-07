from dataclasses import replace

import numpy as np

from econ_sim import market
from econ_sim.config import Config, ScheduledEvent, VillageConfig
from econ_sim.households import Households, pass_on_savings
from econ_sim.population import Population
from econ_sim.scenarios import compare, series
from econ_sim.simulation import Simulation

CONFIG = Config()


def test_families_get_what_they_want_when_there_is_plenty():
    money = np.array([10.0, 10.0])
    sale = market.buy(money, np.array([3.0, 2.0]), np.array([1.0]), np.array([100.0]), np.array([0, 0]))
    assert sale.bought.tolist() == [3.0, 2.0]
    assert money.tolist() == [7.0, 8.0]
    assert sale.sold.tolist() == [5.0] and sale.demand.tolist() == [5.0]


def test_scarce_goods_are_shared_in_proportion_to_what_was_asked():
    money = np.array([10.0, 10.0])
    sale = market.buy(money, np.array([3.0, 1.0]), np.array([1.0]), np.array([2.0]), np.array([0, 0]))
    assert np.allclose(sale.bought, [1.5, 0.5])
    assert np.allclose(money, [8.5, 9.5])


def test_families_cannot_spend_more_than_they_have():
    money = np.array([1.0])
    sale = market.buy(money, np.array([5.0]), np.array([2.0]), np.array([100.0]), np.array([0]))
    assert np.isclose(sale.bought[0], 0.5) and np.isclose(money[0], 0.0)


def test_each_village_has_its_own_market():
    money = np.array([10.0, 10.0])
    sale = market.buy(money, np.array([4.0, 4.0]), np.array([1.0, 2.0]), np.array([2.0, 10.0]), np.array([0, 1]))
    assert np.allclose(sale.bought, [2.0, 4.0])
    assert np.allclose(money, [8.0, 2.0])


def test_price_rises_with_shortage_and_falls_with_glut():
    price = np.ones(3)
    new = market.adjust_price(price, demand=np.array([110.0, 100.0, 80.0]), supply=np.array([100.0, 100.0, 100.0]), config=CONFIG)
    assert new[0] > 1.0 and new[1] == 1.0 and new[2] < 1.0


def test_price_moves_are_capped():
    cap = CONFIG.money.max_price_change
    new = market.adjust_price(np.ones(2), np.array([1000.0, 0.0]), np.array([1.0, 1000.0]), CONFIG)
    assert np.allclose(new, [1 + cap, 1 - cap])


def test_price_rises_when_nothing_is_on_offer():
    new = market.adjust_price(np.ones(1), np.array([10.0]), np.array([0.0]), CONFIG)
    assert new[0] > 1.0


def test_sharing_moves_money_from_comfortable_to_short_families():
    money = np.array([100.0, 0.0, 5.0])
    cost = np.array([10.0, 10.0, 10.0])
    total = money.sum()
    market.share_with_neighbours(money, cost, np.zeros(3, dtype=int), 1, CONFIG)
    assert np.isclose(money.sum(), total)
    assert money[0] < 100.0 and money[1] > 0.0 and money[2] > 5.0
    assert money[1] <= 10.0 and money[2] <= 10.0  # only up to this month's food


def test_no_sharing_when_nobody_has_much():
    money = np.array([15.0, 0.0])
    market.share_with_neighbours(money, np.array([10.0, 10.0]), np.zeros(2, dtype=int), 1, CONFIG)
    assert money.tolist() == [15.0, 0.0]


def people(skill, health, job, household):
    n = len(skill)
    return Population(
        count=np.ones(n), age_months=np.full(n, 30 * 12), female=np.zeros(n, bool), health=health,
        skill=skill, location=np.zeros(n), household=household, job=job,
    )


def test_wages_go_to_workers_by_skill_and_health():
    pop = people(skill=[1.0, 2.0, 1.0], health=[100.0, 100.0, 100.0], job=[0, 0, -1], household=[0, 1, 2])
    cash = np.array([100.0])
    income, paid = market.pay_wages(cash, pop.job == 0, pop, 3, CONFIG)
    assert np.isclose(paid[0], 100.0 * CONFIG.money.wage_payout)
    assert np.isclose(income[1], 2 * income[0]) and income[2] == 0.0
    assert np.isclose(income.sum() + cash[0], 100.0)


def test_weak_workers_earn_less():
    pop = people(skill=[1.0, 1.0], health=[100.0, 0.0], job=[0, 0], household=[0, 1])
    income, _ = market.pay_wages(np.array([100.0]), pop.job == 0, pop, 2, CONFIG)
    assert income[1] < income[0]


def test_savings_of_families_with_nobody_left_go_to_neighbours():
    hh = Households(location=[0, 0, 0, 1], money=[30.0, 10.0, 20.0, 5.0])
    pass_on_savings(hh, size=np.array([0, 2, 3, 0]))
    assert np.allclose(hh.money, [0.0, 25.0, 35.0, 5.0])  # village 1 has no heirs: money stays


def test_money_is_never_created_or_destroyed():
    config = Config(seed=3, months=36, villages=(VillageConfig(name="A"), VillageConfig(name="B", population=300, land=100)))
    sim = Simulation(replace(config, scheduled_events=(ScheduledEvent("drought", 4),)))
    total = sim.world.households.money.sum() + sim.world.farm_cash.sum()
    for _ in range(36):
        r = sim.step()
        assert np.isclose(r.savings + r.business_cash, total)


def test_drought_raises_food_price_and_hits_the_poorest_hardest():
    result = compare(Config(seed=0, months=24), (ScheduledEvent("drought", 4),), runs=5)
    price_gap = series(result.scenario, "food_price").max(axis=1) - series(result.baseline, "food_price").max(axis=1)
    assert (price_gap > 0).all()
    poorest = series(result.scenario, "poorest_fifth_ration").min(axis=1)
    village = series(result.scenario, "ration").min(axis=1)
    assert (poorest <= village + 1e-9).all() and (poorest < village).any()
