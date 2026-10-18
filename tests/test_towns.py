from dataclasses import replace

import numpy as np

from econ_sim import geography, lords, migration, towns
from econ_sim.checks import broken_invariants
from econ_sim.config import Config, MapConfig, VillageConfig
from econ_sim.economy import headcount
from econ_sim.geography import Geography, RegionSettings, region
from econ_sim.households import Households
from econ_sim.rng import RandomStreams
from econ_sim.simulation import Simulation
from econ_sim.world import create_world

CONFIG = Config()


def little_region(border=False, **config):
    """Two villages (5 and 15 km away) and their market town."""
    places = (
        VillageConfig(name="near", population=300, land=105.0, x=5.0),
        VillageConfig(name="far", population=300, land=105.0, x=15.0),
        VillageConfig(name="town", population=60, land=0.0, initial_food_months=0.0, kind="town", border=border),
    )
    world_map = MapConfig(enabled=True, roads=((0, 2), (0, 1)))
    return replace(CONFIG, villages=places, map=world_map, **config)


def geo(prices_town=True):
    return Geography(
        x=np.array([5.0, 15.0, 0.0]), y=np.zeros(3), terrain=np.zeros(3, dtype=int),
        transport=np.array([0.2, 0.3, 0.0]), market=np.array([2, 2, 2]), pasture=np.ones(3), woods=np.ones(3),
        forage=np.ones(3), on_map=True, town=np.array([False, False, True]), border=np.array([False, False, prices_town]),
    )


def test_a_region_has_market_towns_with_people_but_no_farmland():
    config = region(RegionSettings(villages=50, seed=2))
    town = [v for v in config.villages if v.kind == "town"]
    village = [v for v in config.villages if v.kind == "village"]
    assert len(town) == 2 and all(t.land == 0 and t.population >= 300 for t in town)
    share = sum(t.population for t in town) / sum(v.population for v in config.villages)
    assert 0.1 < share < 0.15
    assert sum(t.border for t in town) == 1 and not any(v.border for v in village)


def test_a_town_needs_a_map():
    try:
        geography.build(replace(CONFIG, villages=(VillageConfig(kind="town", land=0.0),)))
    except ValueError:
        return
    raise AssertionError("a town without a map was accepted")


def test_townsfolk_work_in_crafts_more_skilfully_and_buy_grain_at_the_cost_of_bringing_it_in():
    config = little_region()
    world = create_world(config, RandomStreams(1))
    jobs = headcount(world.population, 3, config)
    assert jobs[2, config.farming] == 0 and jobs[2].sum() > 0
    assert (world.skill[2, 1:] == config.town.craft_skill).all() and (world.skill[:2] == 1).all()
    gets = towns.gets(world.geo)
    assert np.isclose(world.food_price[2], np.mean(world.food_price[:2] / gets[:2]), rtol=0.01)
    assert world.dealers_grain[2] > 0 and world.dealers_cash[2] > 0 and world.dealers_grain[:2].sum() == 0
    assert world.unhealthy[2] == config.town.mortality


def test_merchants_carry_where_the_gap_beats_the_carting_and_more_the_wider_it_is():
    g = geo()
    wanted = np.full((3, 4), 100.0)
    stock = np.full((3, 4), 1000.0)
    even = np.ones((3, 4))
    cart = towns.carts(even, g, wanted, stock, np.full(3, 1e6), np.ones(3), 0, CONFIG)
    assert cart.goods.sum() == 0 and cart.grain_in.sum() == 0 and cart.grain_out.sum() == 0
    # Grain cheap in the near village, a little dear in the far one, very dear in the town.
    prices = np.ones((3, 4))
    prices[:, 0] = [0.5, 1.5, 1.3]
    cart = towns.carts(prices, g, wanted, stock, np.full(3, 1e6), np.ones(3), 0, CONFIG)
    full = CONFIG.map.capacity * 100.0
    assert np.isclose(cart.grain_out[0], full) and cart.grain_out[1] == 0
    assert cart.grain_in[0] == 0 and cart.grain_in[1] == 0  # 1.5 < 1.3 x 1.3: not worth it
    prices[1, 0] = 1.3 * (1 + 0.3) * 1.05  # half way up the ramp
    cart = towns.carts(prices, g, wanted, stock, np.full(3, 1e6), np.ones(3), 0, CONFIG)
    assert np.isclose(cart.grain_in[1], 0.5 * full) and np.isclose(cart.grain_held[2], 0.5 * full)


def test_a_town_shares_out_what_it_has_and_its_dealers_buy_only_what_they_can_pay_for():
    g = geo()
    wanted = np.full((3, 4), 100.0)
    prices = np.ones((3, 4))
    prices[:2, 2] = 5.0  # cloth is dear in both villages
    stock = np.zeros((3, 4))
    stock[2, 2] = 10.0  # but the town has only a little
    cart = towns.carts(prices, g, wanted, stock, np.zeros(3), np.ones(3), 0, CONFIG)
    assert np.isclose(cart.incoming(3)[:2, 2].sum(), 10.0)
    prices[:2, 0] = 0.1  # grain cheap in both villages, but the dealers have no coins
    cart = towns.carts(prices, g, wanted, stock, np.zeros(3), np.ones(3), 0, CONFIG)
    assert cart.grain_out.sum() == 0


def test_goods_carted_in_are_sold_for_the_business_that_made_them():
    g = geo()
    cart = towns.Carts(np.zeros(3), np.zeros(3), np.zeros(3), src=np.array([2]), dst=np.array([0]),
                       goods=np.array([[0.0, 0.0, 30.0, 0.0]]))
    stock, cash, demand = np.array([10.0, 0.0, 70.0]), np.zeros(3), np.zeros(3)
    offer = stock + cart.incoming(3)[:, 2]
    sold, spent, asked = np.array([20.0, 0.0, 0.0]), np.array([40.0, 0.0, 0.0]), np.array([25.0, 0.0, 0.0])
    towns.settle(cart, 2, stock, cash, demand, offer, sold, spent, asked)
    # A quarter of what was on offer in the village was its own, three quarters the town's.
    assert np.allclose(cash, [10.0, 0.0, 30.0]) and np.allclose(demand, [6.25, 0.0, 18.75])
    assert np.allclose(stock, [5.0, 0.0, 70.0 + 15.0])  # the town's unsold cloth came back


def test_a_town_marks_up_its_grain_as_its_dealers_store_runs_low():
    need = np.full(2, 100.0)
    usual = CONFIG.town.dealers_months * need
    markup = np.ones(2)
    for _ in range(30):
        markup = towns.store_markup(markup, np.array([usual[0], usual[1] / 4]), need, CONFIG)
    assert np.isclose(markup[0], 1.0) and np.isclose(markup[1], 4 ** CONFIG.town.store_elasticity, rtol=0.01)


def test_only_a_border_town_trades_with_the_world_beyond():
    need = np.array([0.0, 0.0, 100.0])
    for border in (True, False):
        grain, cash = np.array([0.0, 0.0, 500.0]), np.array([0.0, 0.0, 1e5])
        moved = towns.border_trade(grain, cash, np.array([1.0, 1.0, 3.0]), np.ones(3), need, geo(border), CONFIG)
        assert (moved[2] > 0) == border and moved[:2].sum() == 0
    grain, cash = np.array([0.0, 0.0, 500.0]), np.zeros(3)
    moved = towns.border_trade(grain, cash, np.array([1.0, 1.0, 0.3]), np.ones(3), need, geo(True), CONFIG)
    assert moved[2] < 0 and grain[2] >= need[2] and cash[2] > 0  # sold beyond, keeping a month


def test_the_lord_sells_his_grain_in_town_and_pays_its_people_for_their_service():
    lord = lords.Lords(land=np.zeros(3), barn=np.zeros(3), purse=np.array([100.0, 0.0, 0.0]), state_purse=np.zeros(3))
    grain, cash = np.zeros(3), np.array([0.0, 0.0, 50.0])
    sold = lords.sell_in_town(lord, np.array([40.0, 0.0, 0.0]), np.array([2, 2, 2]), grain, cash, np.full(3, 2.0))
    assert np.isclose(sold[0], 25.0) and np.isclose(grain[2], 25.0) and np.isclose(cash[2], 0.0)
    assert np.isclose(lord.purse[0], 150.0) and np.isclose(lord.barn[0], 15.0)  # what they couldn't pay for stays
    paid = lords.pay_servants(lord, np.array([2, 2, 2]), np.array([0, 2, 2]), np.array([4.0, 1.0, 3.0]), CONFIG)
    wages = 150.0 * CONFIG.lord.spends_in_town * CONFIG.lord.servants
    assert paid[0] == 0 and np.allclose(paid[1:], [wages / 4, 3 * wages / 4])
    assert np.isclose(lord.purse[0], 150.0 - wages)


def test_some_who_leave_a_village_go_to_its_market_town_and_lodge_with_any_family():
    config = replace(CONFIG, migration=replace(CONFIG.migration, stay_in_region=0.0, to_town=1.0))
    dest = migration._destinations(np.zeros(50, dtype=int), np.array([0.5, 1.0, 1.0]), np.full(3, 100.0), config,
                                   np.random.default_rng(0), geo())
    assert (dest == 2).all()
    living = np.ones(2, dtype=bool)
    hh = Households(np.array([0, 2]), land=np.zeros(2))
    assert migration._can_host(hh, living, geo()).tolist() == [False, True]


def test_a_region_with_towns_keeps_the_books():
    config = replace(region(RegionSettings(villages=10, mean_size=200, seed=4)), months=36, seed=2)
    sim = Simulation(config)
    money, people = sim.world.money, sim.world.population.size
    for _ in range(36):
        r = sim.step()
        people += r.births - r.deaths - r.emigrants + r.immigrants
        assert people == r.population
        assert broken_invariants(sim.world) == []
        assert np.isclose(sim.world.money, money)
    records = sim.records
    assert all(r.town_people > 0 for r in records)
    assert sum(r.grain_exported for r in records) > 0  # villages fed the town
    assert np.mean([r.town_ration for r in records]) > 0.9


def test_a_town_lives_on_its_villages_grain():
    config = replace(little_region(), months=12, seed=3, random_events=False)
    sim = Simulation(config)
    sim.run()
    town_need = sum(r.food_needed for r in sim.records) * 60 / 660
    carted = sum(r.grain_exported for r in sim.records)
    assert carted > 0.5 * town_need
    assert min(r.town_ration for r in sim.records) > 0.9
