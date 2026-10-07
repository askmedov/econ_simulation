from dataclasses import replace

import numpy as np

from econ_sim import economy
from econ_sim.config import Config
from econ_sim.population import NO_JOB, Population

CONFIG = Config()
FARM, WOOD, WEAVE, SMITH = (CONFIG.business_index(b) for b in ("farming", "woodcutting", "weaving", "smithing"))


def workers(jobs, health=100.0, age=30):
    n = len(jobs)
    return Population(
        count=np.ones(n), age_months=np.full(n, age * 12), female=np.zeros(n, bool),
        health=np.full(n, health, dtype=float), skill=np.ones(n), location=np.zeros(n), job=jobs,
    )


def test_headcount_and_labor_by_business():
    pop = workers([FARM, FARM, WEAVE, NO_JOB])
    assert economy.headcount(pop, 1, CONFIG).tolist() == [[2.0, 0.0, 1.0, 0.0]]
    weak = workers([FARM], health=0.0)
    assert economy.labor(weak, 1, CONFIG)[0, FARM] == CONFIG.health.work_at_zero_health
    assert economy.labor(weak, 1, CONFIG, at_full_health=True)[0, FARM] == 1.0


def test_more_farmers_on_the_same_land_each_grow_less():
    land, mult = np.array([50.0]), np.ones((1, 4))
    one = np.array([[50.0, 0, 0, 0]])
    two = np.array([[100.0, 0, 0, 0]])
    a = economy.capacity(one, one, one, land, mult, CONFIG)[0, FARM]
    b = economy.capacity(two, two, two, land, mult, CONFIG)[0, FARM]
    assert a < b < 2 * a


def test_other_trades_make_the_same_per_worker():
    land, mult = np.array([50.0]), np.ones((1, 4))
    one = np.array([[0, 10.0, 0, 0]])
    two = np.array([[0, 20.0, 0, 0]])
    assert np.isclose(economy.capacity(two, two, two, land, mult, CONFIG)[0, WOOD],
                      2 * economy.capacity(one, one, one, land, mult, CONFIG)[0, WOOD])


def test_tools_raise_output():
    land, mult, labor = np.array([50.0]), np.ones((1, 4)), np.array([[0, 10.0, 0, 0]])
    without = economy.capacity(labor, np.zeros((1, 4)), labor, land, mult, CONFIG)[0, WOOD]
    with_tools = economy.capacity(labor, labor, labor, land, mult, CONFIG)[0, WOOD]
    assert np.isclose(with_tools, without * (1 + CONFIG.businesses[WOOD].tool_boost))


def test_events_scale_one_business():
    land, labor = np.array([50.0]), np.array([[50.0, 10.0, 0, 0]])
    mult = np.ones((1, 4))
    mult[0, FARM] = 0.6
    hit = economy.capacity(labor, labor, labor, land, mult, CONFIG)
    normal = economy.capacity(labor, labor, labor, land, np.ones((1, 4)), CONFIG)
    assert np.isclose(hit[0, FARM], 0.6 * normal[0, FARM]) and hit[0, WOOD] == normal[0, WOOD]


def test_smiths_need_firewood():
    supplies = np.zeros((1, 4, 4))
    fuel = CONFIG.product_index("firewood")
    per_tool = dict(CONFIG.businesses[SMITH].inputs)["firewood"]
    supplies[0, SMITH, fuel] = 3 * per_tool
    made = economy.limit_by_supplies(np.array([[0, 0, 0, 10.0]]), supplies, CONFIG)
    assert np.isclose(made[0, SMITH], 3.0) and np.isclose(supplies[0, SMITH, fuel], 0.0)


def test_fair_price_is_wage_over_output_plus_supplies():
    per_worker = np.array([[2.0, 8.0, 2.0, 1.0]])
    prices = np.zeros((1, 4))
    for _ in range(4):
        prices = economy.fair_prices(np.array([2.0]), per_worker, prices, CONFIG)
    fuel = CONFIG.product_index("firewood")
    assert np.isclose(prices[0, CONFIG.product_index("food")], 1.0)
    assert np.isclose(prices[0, fuel], 0.25)
    assert np.isclose(prices[0, CONFIG.product_index("tools")], 2.0 + 2.0 * 0.25)


def test_necessities_get_workers_before_comforts():
    work_needed = np.array([[80.0, 10.0, 50.0, 5.0]])
    wanted = economy.wanted_workers(work_needed, np.array([[50.0, 20.0, 20.0, 10.0]]), CONFIG)
    assert np.isclose(wanted.sum(), 100.0)
    assert np.allclose(wanted[0, [FARM, WOOD, SMITH]], [80.0, 10.0, 5.0])
    assert np.isclose(wanted[0, WEAVE], 5.0)


def test_necessities_share_out_workers_when_short():
    wanted = economy.wanted_workers(np.array([[150.0, 50.0, 30.0, 0.0]]), np.array([[50.0, 30.0, 10.0, 10.0]]), CONFIG)
    assert np.allclose(wanted, [[75.0, 25.0, 0.0, 0.0]])


def test_workers_move_from_crowded_trades_to_short_ones():
    pop = workers([FARM] * 90 + [WEAVE] * 10)
    wanted = np.array([[60.0, 0.0, 40.0, 0.0]])
    config = replace(CONFIG, trade=replace(CONFIG.trade, hiring_rate=1.0))
    moved = economy.move_workers(pop, wanted, config, np.random.default_rng(0))
    assert moved > 0 and pop.size == 100
    counts = economy.headcount(pop, 1, config)[0]
    assert counts[FARM] < 90 and counts[WEAVE] > 10 and counts[WOOD] == 0
