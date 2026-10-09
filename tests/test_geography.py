from dataclasses import replace

import numpy as np
import pytest

from econ_sim import events, geography, livestock, migration
from econ_sim.__main__ import _parse_event
from econ_sim.checks import broken_invariants
from econ_sim.config import Config, EventSpec, MapConfig, ScheduledEvent, VillageConfig
from econ_sim.geography import Geography, RegionSettings, region
from econ_sim.households import Households
from econ_sim.migration import Memory
from econ_sim.rng import RandomStreams
from econ_sim.simulation import Simulation
from econ_sim.town import merchants
from econ_sim.world import create_world

CONFIG = Config()


def on_map(villages, roads=(), rivers=(), towns=((0.0, 0.0),), **config):
    """A config with villages at given (x, y[, terrain]) and a map."""
    made = tuple(
        VillageConfig(name=f"v{i}", population=200, land=70.0, x=v[0], y=v[1], terrain=v[2] if len(v) > 2 else "plain")
        for i, v in enumerate(villages)
    )
    return replace(CONFIG, villages=made, map=MapConfig(enabled=True, towns=towns, roads=roads, rivers=rivers), **config)


def test_a_region_is_laid_out_the_same_way_from_the_same_settings():
    settings = RegionSettings(villages=60, seed=3)
    first, again, other = region(settings), region(settings), region(replace(settings, seed=4))
    assert first == again and first != other
    assert len(first.villages) == 60 and len(first.map.towns) == round(60 / 25)
    kinds = {v.terrain for v in first.villages}
    assert {"plain", "upland", "valley", "forest"} <= kinds
    assert first.map.enabled and first.map.rivers


def test_every_village_can_reach_a_market_town():
    config = region(RegionSettings(villages=80, seed=5))
    geo = geography.build(config)
    assert np.isfinite(geo.transport).all()
    assert ((geo.market >= 0) & (geo.market < len(config.map.towns))).all()
    assert (geo.transport >= config.map.handling).all()


def test_carting_costs_more_the_farther_and_less_by_river_and_more_over_hills():
    roads = ((0, 3), (0, 1), (1, 2))  # the town is place 3, after the three villages
    config = on_map([(5, 0), (10, 0), (20, 0)], roads=roads)
    cfg = config.map
    transport, market = geography.carriage(config)
    assert np.allclose(transport, cfg.handling + cfg.road_per_km * np.array([5, 10, 20]))
    assert (market == 0).all()
    by_river = geography.carriage(replace(config, map=replace(cfg, rivers=((2, 3),))))[0]
    assert np.isclose(by_river[2], cfg.handling + cfg.river_per_km * 20) and by_river[2] < transport[2]
    hills = on_map([(5, 0, "upland"), (10, 0), (20, 0)], roads=roads)
    assert geography.carriage(hills)[0][0] > transport[0]


def test_a_village_no_road_reaches_never_trades():
    config = on_map([(5, 0), (50, 50)], roads=((0, 2),))
    transport, market = geography.carriage(config)
    assert np.isfinite(transport[0]) and np.isinf(transport[1]) and market[1] == -1
    carts = merchants(np.array([0.1, 0.1]), np.ones(2), np.full(2, 100.0), config, transport)
    assert carts.exports[0] > 0 and carts.exports[1] == 0
    carts = merchants(np.array([9.0, 9.0]), np.ones(2), np.full(2, 100.0), config, transport)
    assert carts.imports[0] > 0 and carts.imports[1] == 0


def test_a_remote_village_starts_with_cheaper_grain_at_the_margin_of_the_town_price():
    config = on_map([(2, 0), (30, 0)], roads=((0, 2), (0, 1)))
    world = create_world(config, RandomStreams(1))
    t = world.geo.transport
    price = world.food_price
    assert price[1] < price[0]
    assert np.allclose(price / (1.0 - t), world.town.level, rtol=0.02)


def test_the_weather_is_shared_by_neighbours_and_keeps_its_usual_chances():
    geo = geography.build(on_map([(0, 0), (2, 0), (2000, 0)]))
    rng = np.random.default_rng(0)
    draws = np.array([geography.weather(geo, 250.0, rng) for _ in range(3000)])
    assert np.corrcoef(draws[:, 0], draws[:, 1])[0, 1] > 0.99
    assert abs(np.corrcoef(draws[:, 0], draws[:, 2])[0, 1]) < 0.1
    assert abs((draws[:, 0] < 0.1).mean() - 0.1) < 0.02


def roll(world, specs, seed):
    world.active_events.clear()
    world.last_started.clear()
    weather = events.Weather(world.geo, 250.0, np.random.default_rng([seed, 1]))
    started = events.start_location_events(world, specs, np.random.default_rng(seed), weather)
    return {(e.spec.name, e.location) for e in started}


def test_droughts_and_good_years_fall_on_neighbours_together():
    world = create_world(on_map([(0, 0), (2, 0), (4, 0), (3000, 0)]), RandomStreams(0))
    weather = dict(scope="location", months=None, group="weather", reach="weather", effects={"production_mult": 0.6})
    drought = EventSpec("drought", chance=0.3, **weather)
    good = EventSpec("good_weather", chance=0.3, end="good", **{**weather, "effects": {"production_mult": 1.25}})
    apart = together = 0
    for seed in range(300):
        hit = roll(world, (good, drought), seed)
        for name in ("drought", "good_weather"):
            near = [(name, v) in hit for v in range(3)]
            together += all(near) or not any(near)
            apart += ((name, 0) in hit) != ((name, 3) in hit)
        # Never a drought next door to a good year.
        assert not ({("drought", 0), ("good_weather", 1)} <= hit or {("good_weather", 0), ("drought", 1)} <= hit)
    assert together >= 0.97 * 600
    assert apart > 100  # far away, the weather is another matter


def test_a_volcano_strikes_every_village_at_once():
    world = create_world(on_map([(0, 0), (500, 0), (1000, 0)]), RandomStreams(0))
    cold = EventSpec("cold_years", scope="location", chance=0.5, reach="region", effects={"production_mult": 0.7})
    counts = [len(roll(world, (cold,), seed)) for seed in range(100)]
    assert set(counts) == {0, 3}


def test_a_scheduled_event_can_strike_one_part_of_the_map():
    config = on_map([(0, 0), (3, 0), (20, 0)], roads=((0, 3), (1, 3), (2, 3)), months=1,
                    scheduled_events=(ScheduledEvent("drought", 1, near=(1.0, 0.0, 5.0)),))
    sim = Simulation(config)
    sim.step()
    struck = {e.location for e in sim.world.active_events if e.spec.name == "drought"}
    assert struck == {0, 1}


def test_an_event_near_a_place_needs_a_map():
    with pytest.raises(ValueError):
        Simulation(replace(CONFIG, scheduled_events=(ScheduledEvent("drought", 1, near=(0.0, 0.0, 5.0)),)))
    assert _parse_event("drought@4@20,10,8") == ScheduledEvent("drought", 4, near=(20.0, 10.0, 8.0))


def test_terrain_gives_woods_pasture_and_famine_foods():
    world = create_world(on_map([(0, 0), (1, 0, "forest"), (2, 0, "upland")]), RandomStreams(0))
    forest = CONFIG.map.terrains[CONFIG.map.terrain_index("forest")]
    upland = CONFIG.map.terrains[CONFIG.map.terrain_index("upland")]
    assert np.isclose(world.woods_capacity[1] / world.woods_capacity[0], forest.woods)
    assert world.geo.forage[1] == forest.forage
    hh = Households(np.array([0, 2]), land=np.ones(2), animals=np.full(2, 50.0))
    culled, _ = livestock.winter_cull(hh, np.array([10.0, 0.0, 10.0]), CONFIG, world.geo.pasture)
    room = CONFIG.livestock.winter_capacity_per_plot * 10.0
    assert np.isclose(culled[0], 50.0 - room) and np.isclose(culled[2], 50.0 - upland.pasture * room)


def test_terrain_must_be_known():
    with pytest.raises(ValueError):
        geography.build(replace(CONFIG, villages=(VillageConfig(terrain="moon"),)))


def test_people_who_move_go_mostly_to_villages_nearby():
    geo = Geography(
        x=np.array([0.0, 5.0, 100.0]), y=np.zeros(3), terrain=np.zeros(3, dtype=int), transport=np.zeros(3),
        market=np.zeros(3, dtype=int), pasture=np.ones(3), woods=np.ones(3), forage=np.ones(3), on_map=True,
    )
    config = replace(CONFIG, migration=replace(CONFIG.migration, stay_in_region=1.0))
    dest = migration._destinations(np.zeros(2000, dtype=int), np.array([0.5, 1.0, 1.0]), np.full(3, 100.0), config,
                                   np.random.default_rng(0), geo)
    assert (dest == 1).sum() > 10 * (dest == 2).sum() > 0
    anywhere = migration._destinations(np.zeros(2000, dtype=int), np.array([0.5, 1.0, 1.0]), np.full(3, 100.0), config,
                                       np.random.default_rng(0))
    assert abs((anywhere == 1).mean() - 0.5) < 0.05


def test_a_region_keeps_the_books_and_each_village_its_record():
    config = replace(region(RegionSettings(villages=8, mean_size=150, seed=2)), months=36, seed=3)
    sim = Simulation(config)
    money = sim.world.money
    people = sim.world.population.size
    for _ in range(36):
        r = sim.step()
        people += r.births - r.deaths - r.emigrants + r.immigrants
        assert people == r.population
        assert broken_invariants(sim.world) == []
        assert np.isclose(sim.world.money, money)
    places = sim.place_series()
    assert places["population"].shape == (36, 8)
    assert places["population"].sum(axis=1).tolist() == [r.population for r in sim.records]
    assert places["deaths"].sum() == sum(r.deaths for r in sim.records)


def test_without_a_map_villages_keep_their_own_weather():
    config = replace(CONFIG, villages=(VillageConfig(), VillageConfig()))
    assert not create_world(config, RandomStreams(0)).geo.on_map
    weather = dict(scope="location", months=None, reach="weather", effects={"production_mult": 0.6})
    world = create_world(config, RandomStreams(0))
    differ = 0
    for seed in range(100):
        world.active_events.clear()
        world.last_started.clear()
        started = events.start_location_events(world, (EventSpec("drought", chance=0.5, **weather),),
                                               np.random.default_rng(seed))
        differ += len(started) == 1
    assert differ > 30


def test_memory_is_per_village_on_a_map_too():
    config = replace(region(RegionSettings(villages=5, mean_size=100, seed=1)), months=2)
    sim = Simulation(config)
    sim.run()
    assert isinstance(sim.world.memory, Memory) and len(sim.world.memory.hunger) == 5
