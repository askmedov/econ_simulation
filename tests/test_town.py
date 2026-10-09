from dataclasses import replace

import numpy as np

from econ_sim import town
from econ_sim.config import Config, ScheduledEvent, TownConfig
from econ_sim.simulation import Simulation
from econ_sim.town import Town

CONFIG = Config()


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


def test_a_debased_coinage_raises_prices_in_coins():
    config = Config(seed=1, months=48, random_events=False)
    debased = replace(config, scheduled_events=(ScheduledEvent("debasement", 3),))
    before = Simulation(config).run()
    after = Simulation(debased).run()
    assert np.isclose(after[-1].town_price, before[-1].town_price / (1 - 0.25))
    late = slice(36, 48)
    assert np.mean([r.food_price for r in after[late]]) > np.mean([r.food_price for r in before[late]])
