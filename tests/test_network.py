from dataclasses import replace

import numpy as np

from econ_sim import events, geography, livestock, towns
from econ_sim.checks import broken_invariants
from econ_sim.config import Config, MapConfig, ScheduledEvent, VillageConfig
from econ_sim.geography import Geography, RegionSettings, Routes, region
from econ_sim.households import Households
from econ_sim.rng import RandomStreams
from econ_sim.simulation import Simulation
from econ_sim.world import create_world

CONFIG = Config()


def two_towns(river=False, border=True, **config):
    """A village by each of two towns 20 km apart, joined by a road (or a river)."""
    places = (
        VillageConfig(name="west village", population=300, land=105.0, x=0.0),
        VillageConfig(name="east village", population=300, land=105.0, x=24.0),
        VillageConfig(name="west town", population=60, land=0.0, initial_food_months=0.0, x=2.0, kind="town", border=border),
        VillageConfig(name="east town", population=60, land=0.0, initial_food_months=0.0, x=22.0, kind="town"),
    )
    legs = ((2, 3),)
    world_map = MapConfig(enabled=True, roads=((0, 2), (1, 3)) + (() if river else legs), rivers=legs if river else ())
    return replace(CONFIG, villages=places, map=world_map, **config)


def geo_of(cost=0.3, water=0.0):
    return Geography(
        x=np.zeros(4), y=np.zeros(4), terrain=np.zeros(4, dtype=int), transport=np.array([0.2, 0.2, 0.0, 0.0]),
        market=np.array([2, 3, 2, 3]), pasture=np.ones(4), woods=np.ones(4), forage=np.ones(4), on_map=True,
        town=np.array([False, False, True, True]), border=np.array([False, False, True, False]),
        routes=Routes(np.array([2]), np.array([3]), np.array([cost]), np.array([water])),
    )


def test_routes_join_neighbouring_towns_by_the_cheapest_way_and_know_how_much_is_by_water():
    config = two_towns()
    geo = geography.build(config)
    r = geo.routes
    assert r.a.tolist() == [2] and r.b.tolist() == [3]
    assert np.isclose(r.cost[0], config.map.wholesale_handling + config.map.road_per_km * 20) and r.water[0] == 0
    by_river = geography.build(two_towns(river=True)).routes
    assert np.isclose(by_river.cost[0], config.map.wholesale_handling + config.map.river_per_km * 20)
    assert by_river.water[0] == 1
    assert towns.water_factor(geography.build(two_towns(river=True)), config)[0] == config.map.water_capacity


def test_each_town_only_looks_to_its_nearest_towns():
    a = np.array([0, 1, 2, 3])
    b = np.array([1, 2, 3, 4])
    cost = np.ones(4)
    r = geography.town_routes(5, a, b, cost, np.zeros(4, dtype=bool), np.arange(5), 0.1, nearest=1)
    assert sorted(zip(r.a.tolist(), r.b.tolist())) == [(0, 1), (1, 2), (2, 3), (3, 4)]  # no long hops
    assert np.allclose(r.cost, 1.1)


def test_grain_goes_to_the_dearer_town_when_the_gap_beats_the_route_and_is_paid_for():
    need = np.array([0.0, 0.0, 100.0, 100.0])
    grain, cash = np.array([0.0, 0.0, 500.0, 0.0]), np.array([0.0, 0.0, 0.0, 1e5])
    before = cash.sum()
    moved = towns.between_towns(grain, cash, np.array([1.0, 1.0, 1.0, 1.25]), need, geo_of(0.3), CONFIG)
    assert not moved.any()  # 25% dearer, but the route costs 30%
    need[1] = 200.0  # the east town's market: itself and a village
    moved = towns.between_towns(grain, cash, np.array([1.0, 1.0, 1.0, 2.0]), need, geo_of(0.3), CONFIG)
    full = CONFIG.map.town_capacity * 300.0
    assert np.isclose(moved[3], full) and np.isclose(moved[2], -full)
    assert np.isclose(cash[2], full * 1.0) and np.isclose(cash.sum(), before)  # at the seller's price


def test_a_town_sends_only_what_it_can_spare_and_a_river_carries_more():
    need = np.array([0.0, 0.0, 100.0, 100.0])
    grain, cash = np.array([0.0, 0.0, 110.0, 0.0]), np.array([0.0, 0.0, 0.0, 1e5])
    moved = towns.between_towns(grain, cash, np.array([1.0, 1.0, 1.0, 2.0]), need, geo_of(0.3), CONFIG)
    assert np.isclose(moved[3], 110.0 - CONFIG.town.keep_months * 100.0)
    grain = np.array([0.0, 0.0, 1e4, 0.0])
    moved = towns.between_towns(grain, cash, np.array([1.0, 1.0, 1.0, 2.0]), need, geo_of(0.3, water=1.0), CONFIG)
    assert np.isclose(moved[3], CONFIG.map.town_capacity * CONFIG.map.water_capacity * 100.0)


def test_cloth_goes_between_towns_on_consignment():
    geo = geo_of(0.3)
    prices = np.ones((4, 4))
    prices[3, 2] = 3.0  # cloth is dear in the east town
    stock = np.full((4, 4), 1000.0)
    cart = towns.carts(prices, geo, np.full((4, 4), 100.0), stock, np.full(4, 1e6), np.ones(4), 0, CONFIG)
    between = (cart.src == 2) & (cart.dst == 3)
    assert cart.goods[between, 2].sum() > 0 and cart.goods[between][:, [0, 1, 3]].sum() == 0


def test_with_several_towns_the_river_town_is_a_city():
    one = region(RegionSettings(villages=25))
    assert sum(v.kind == "town" for v in one.villages) == 1 and not any(v.name == "City" for v in one.villages)
    many = region(RegionSettings(villages=100))
    city = next(v for v in many.villages if v.name == "City")
    assert city.border and city.population == max(v.population for v in many.villages if v.kind == "town")


def test_hill_villages_hold_less_farmland_for_their_people_and_count_it_as_usual():
    config = region(RegionSettings(villages=60, seed=2))
    per_person = {}
    for v in config.villages:
        if v.kind == "village":
            per_person.setdefault(v.terrain, []).append(v.land / v.population)
    arable = {t.name: t.arable for t in config.map.terrains}
    assert np.isclose(np.mean(per_person["upland"]) / np.mean(per_person["plain"]), arable["upland"], rtol=0.02)
    geo = geography.build(config)
    hill = geo.terrain == config.map.terrain_index("upland")
    assert np.allclose(geo.usual_land(np.ones(len(geo.x)))[hill], 1 / arable["upland"])


def test_herds_are_shorn_and_hill_villages_start_stocked():
    hh = Households(np.array([0, 1]), animals=np.array([10.0, 20.0]))
    assert np.allclose(livestock.shear(hh, np.array([0.5, 2.0])), [5.0, 40.0])
    land = np.array([100.0, 100.0])
    hh = Households(np.array([0, 1]), land=np.array([1.0, 1.0]))
    livestock.place_herds(hh, land, CONFIG, np.random.default_rng(0), np.array([1.0, 3.5]))
    assert np.isclose(hh.animals[1] / hh.animals[0], 3.5)


def test_wool_is_sold_beyond_the_border_in_june_and_a_slump_halves_it():
    config = two_towns(months=6, start_month=1, random_events=False)
    sim = Simulation(config)
    money = sim.world.money
    sim.run()
    wool = [r.wool_sold for r in sim.records]
    assert wool[5] > 0 and sum(wool[:5]) == 0
    assert np.isclose(sim.world.money, money)  # the coins came from beyond (the town's purse)
    slump = Simulation(replace(config, scheduled_events=(ScheduledEvent("wool_slump", 1),)))
    slump.run()
    assert np.isclose(slump.records[5].wool_sold / wool[5], 0.4, rtol=0.2)
    no_border = Simulation(two_towns(border=False, months=6, random_events=False))
    no_border.run()
    assert no_border.records[5].wool_sold == 0


def test_a_wool_slump_is_never_rolled_without_a_map():
    spec = next(e for e in CONFIG.events if e.name == "wool_slump")
    assert set(spec.effects) <= set(events.MAP_ONLY_EFFECTS)
    world = create_world(CONFIG, RandomStreams(0))
    sure = replace(spec, chance=1.0)
    assert events.start_location_events(world, (sure,), np.random.default_rng(0)) == []


def test_a_region_with_a_city_keeps_the_books_and_its_towns_trade():
    config = replace(region(RegionSettings(villages=40, mean_size=150, towns=3, seed=5)), months=36, seed=1)
    sim = Simulation(config)
    money, people = sim.world.money, sim.world.population.size
    for _ in range(36):
        r = sim.step()
        people += r.births - r.deaths - r.emigrants + r.immigrants
        assert people == r.population
        assert broken_invariants(sim.world) == []
        assert np.isclose(sim.world.money, money)
    places = sim.place_series()
    assert places["sent"].shape == places["wool"].shape == (36, len(config.villages))
    assert places["wool"].sum() == sum(r.wool_sold for r in sim.records) > 0
