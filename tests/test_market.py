from dataclasses import replace

import numpy as np

from econ_sim import market
from econ_sim.config import Config, CouncilConfig, ScheduledEvent, VillageConfig
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


def test_markup_rises_with_shortage_and_falls_with_glut():
    new = market.adjust_markup(np.ones(3), np.array([110.0, 100.0, 80.0]), np.array([100.0, 100.0, 100.0]), CONFIG)
    assert new[0] > 1.0 and new[1] == 1.0 and new[2] < 1.0


def test_markup_moves_are_capped_and_bounded():
    cap = CONFIG.trade.max_markup_change
    low, high = CONFIG.trade.markup_range
    new = market.adjust_markup(np.ones(2), np.array([1000.0, 0.0]), np.array([1.0, 1000.0]), CONFIG)
    assert np.allclose(new, [1 + cap, 1 - cap])
    assert market.adjust_markup(np.array([high]), np.array([10.0]), np.array([1.0]), CONFIG)[0] == high
    assert market.adjust_markup(np.array([low]), np.array([0.0]), np.array([1.0]), CONFIG)[0] == low


def test_markup_rises_when_nothing_is_on_offer():
    assert market.adjust_markup(np.ones(1), np.array([10.0]), np.array([0.0]), CONFIG)[0] > 1.0


def test_grain_price_follows_king_davenant():
    normal = CONFIG.food.normal_cover
    markup = np.ones(3)
    for _ in range(100):  # let it settle
        markup = market.grain_markup(markup, np.array([normal, 0.9 * normal, 0.8 * normal]), CONFIG)
    assert np.isclose(markup[0], 1.0)
    assert 1.25 < markup[1] < 1.35  # 10% short: about 30% dearer
    assert 1.65 < markup[2] < 1.85  # 20% short: about 80% dearer


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
    cash = np.array([[100.0, 0.0, 0.0, 0.0]])
    income, paid = market.pay_wages(cash, pop, 3, CONFIG)
    assert np.isclose(paid[0, 0], 100.0 * CONFIG.money.wage_payout)
    assert np.isclose(income[1], 2 * income[0]) and income[2] == 0.0
    assert np.isclose(income.sum() + cash.sum(), 100.0)


def test_each_business_pays_its_own_workers():
    pop = people(skill=[1.0, 1.0], health=[100.0, 100.0], job=[0, 2], household=[0, 1])
    income, _ = market.pay_wages(np.array([[100.0, 0.0, 10.0, 0.0]]), pop, 2, CONFIG)
    assert np.isclose(income[0], 90.0) and np.isclose(income[1], 9.0)


def test_businesses_set_money_aside_before_paying_wages():
    pop = people(skill=[1.0], health=[100.0], job=[0], household=[0])
    cash = np.array([[100.0, 0.0, 0.0, 0.0]])
    income, _ = market.pay_wages(cash, pop, 1, CONFIG, keep=np.array([[60.0, 0.0, 0.0, 0.0]]))
    assert np.isclose(income[0], 40.0 * CONFIG.money.wage_payout)


def test_weak_workers_earn_less():
    pop = people(skill=[1.0, 1.0], health=[100.0, 0.0], job=[0, 0], household=[0, 1])
    income, _ = market.pay_wages(np.array([[100.0, 0.0, 0.0, 0.0]]), pop, 2, CONFIG)
    assert income[1] < income[0]


def test_savings_of_families_with_nobody_left_go_to_neighbours():
    hh = Households(location=[0, 0, 0, 1], money=[30.0, 10.0, 20.0, 5.0])
    pass_on_savings(hh, size=np.array([0, 2, 3, 0]))
    assert np.allclose(hh.money, [0.0, 25.0, 35.0, 5.0])  # village 1 has no heirs: money stays


def test_money_is_never_created_or_destroyed():
    config = Config(seed=3, months=36, villages=(VillageConfig(name="A"), VillageConfig(name="B", population=300, land=100)))
    sim = Simulation(replace(config, scheduled_events=(ScheduledEvent("drought", 4),)))
    total = sim.world.money
    for _ in range(36):
        r = sim.step()
        assert np.isclose(r.savings + r.business_cash + r.treasury + r.lord_purse + r.state_purse, total)


def test_drought_raises_food_price_and_hits_the_poorest_hardest():
    # Without a council's relief, the market alone decides who eats.
    result = compare(Config(seed=0, months=24, council=CouncilConfig(enabled=False)), (ScheduledEvent("drought", 4),), runs=5)
    price_gap = series(result.scenario, "food_price").max(axis=1) - series(result.baseline, "food_price").max(axis=1)
    # (a run whose baseline also has a bad year can hit the same ceiling)
    assert (price_gap >= 0).all() and (price_gap > 0).mean() >= 0.6
    poorest = series(result.scenario, "poorest_fifth_ration").min(axis=1)
    village = series(result.scenario, "ration").min(axis=1)
    # On average: with wages paid in grain, the poorest in coins and grain are
    # not always the hungriest (a craftsman with a few coins can be worse off).
    assert poorest.mean() < village.mean()


def test_firewood_is_stocked_up_before_winter():
    assert np.allclose(market.seasonal_buffer((1.0,) * 12), 0.0)
    buffer = market.seasonal_buffer(CONFIG.needs.firewood)
    assert buffer.argmax() == 10  # most is needed going into November
    assert buffer[3] == 0.0  # by April the winter is over


def test_families_keep_warm_through_ordinary_winters():
    config = replace(CONFIG, seed=2, months=60, random_events=False)
    records = Simulation(config).run()
    # The poorest gather most of their fuel and go a little cold.
    assert min(r.warmth for r in records) > 0.85


def test_families_who_cannot_buy_firewood_gather_some():
    poor = replace(CONFIG, seed=2, months=24, random_events=False, money=replace(CONFIG.money, initial_savings_months=0.0))
    for record in Simulation(poor).run():
        assert record.warmth >= CONFIG.needs.gathering - 1e-9


def test_customary_wage_follows_the_money_there_is():
    little = replace(CONFIG, seed=2, months=120, money=replace(CONFIG.money, initial_savings_months=0.5))
    sim = Simulation(little)
    start = sim.world.wage_level.copy()
    sim.run()
    assert (sim.world.wage_level < start).all()


def test_no_heating_needs_no_buffer():
    assert np.allclose(market.seasonal_buffer((0.0,) * 12), 0.0)
