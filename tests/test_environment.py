from dataclasses import replace

import numpy as np

from econ_sim import environment, events
from econ_sim.config import Config, EnvironmentConfig, EventSpec, ScheduledEvent
from econ_sim.rng import RandomStreams
from econ_sim.simulation import Simulation
from econ_sim.world import create_world

CONFIG = Config()
ENV = CONFIG.environment


def test_woods_start_in_balance_with_the_village():
    capacity, woods = environment.size_woods(np.array([1000.0]), CONFIG)
    yearly_need = 1000 * 12 * np.mean(CONFIG.needs.firewood)
    assert np.isclose(capacity[0], ENV.woods_years * yearly_need)
    regrowth = ENV.regrowth * woods * (1 - woods / capacity)
    assert np.isclose(regrowth[0], yearly_need)
    assert woods[0] > capacity[0] / 2  # on the side where cutting less lets them grow back


def test_woods_hold_steady_under_balanced_cutting_and_regrow_after_a_fire():
    capacity, start = environment.size_woods(np.array([1000.0]), CONFIG)
    monthly_cut = ENV.regrowth / 12 * start * (1 - start / capacity)
    woods = start.copy()
    for _ in range(120):
        environment.grow_woods(woods, capacity, monthly_cut, np.zeros(1), CONFIG)
    assert np.isclose(woods[0], start[0], rtol=1e-6)
    lost = environment.grow_woods(woods, capacity, monthly_cut, np.array([0.2]), CONFIG)
    assert np.isclose(lost[0], 0.2 * start[0])
    burned = woods.copy()
    for _ in range(12 * 15):
        environment.grow_woods(woods, capacity, monthly_cut, np.zeros(1), CONFIG)
    assert burned[0] < woods[0] and woods[0] > 0.9 * start[0]  # mostly grown back in 15 years


def test_overcut_woods_thin_and_never_vanish_entirely():
    capacity, start = environment.size_woods(np.array([1000.0]), CONFIG)
    woods = start.copy()
    for _ in range(12 * 200):
        environment.grow_woods(woods, capacity, 3.0 * ENV.regrowth / 12 * start * (1 - start / capacity),
                               np.zeros(1), CONFIG)
    assert np.isclose(woods[0], 0.01 * capacity[0])


def test_thin_woods_make_wood_harder_to_get():
    normal = np.array([100.0, 100.0, 100.0])
    reach = environment.woods_reach(np.array([100.0, 25.0, 200.0]), normal, CONFIG)
    assert np.allclose(reach, [1.0, 0.25 ** ENV.reach_exponent, 1.5 ** ENV.reach_exponent])
    off = replace(CONFIG, environment=EnvironmentConfig(enabled=False))
    assert (environment.woods_reach(np.array([1.0]), np.array([100.0]), off) == 1).all()


def test_crowded_soil_tires_and_rested_soil_recovers():
    land = np.array([100.0, 100.0, 100.0])
    people = land * ENV.people_per_plot * np.array([1.0, 1.5, 0.5])
    target = environment.soil_target(people, land, CONFIG)
    assert np.isclose(target[0], 1.0)
    assert np.isclose(target[1], 1 - 0.5 * ENV.overcropping)
    assert np.isclose(target[2], min(1 + 0.5 * ENV.overcropping, ENV.fertility_range[1]))
    soil = np.ones(3)
    for _ in range(12):
        environment.tire_soil(soil, people, land, CONFIG)
    # A year closes `soil_recovery` of the gap.
    assert np.allclose(soil - 1.0, ENV.soil_recovery * (target - 1.0))


def test_droughts_come_in_runs_but_no_more_often_in_the_long_run():
    spec = next(s for s in CONFIG.events if s.name == "drought")
    after_none = events.chance_now(spec, repeated=False)
    assert events.chance_now(spec, repeated=True) == spec.repeat_chance > spec.chance > after_none
    # Long-run share of years with a drought, as a two-state chain.
    p = after_none / (1 - spec.repeat_chance + after_none)
    assert np.isclose(p, spec.chance)
    plain = EventSpec("x", "location", 0.2, {})
    assert events.chance_now(plain, repeated=True) == 0.2


def test_a_drought_makes_the_next_years_drought_likelier_in_a_run():
    def second_droughts(scheduled):
        count = 0
        for seed in range(60):
            records = Simulation(Config(seed=seed, months=16, scheduled_events=scheduled)).run()
            count += "drought" in records[15].events  # April of year 2
        return count

    assert second_droughts((ScheduledEvent("drought", 4),)) > 2 * max(second_droughts(()), 1)


def test_a_forest_fire_burns_part_of_the_woods():
    base = Simulation(Config(seed=0, months=12, random_events=False)).run()
    fire = Simulation(Config(seed=0, months=12, random_events=False,
                             scheduled_events=(ScheduledEvent("forest_fire", 6),))).run()
    assert sum(r.woods_burned for r in fire) > 0 and sum(r.woods_burned for r in base) == 0
    assert fire[-1].woods < base[-1].woods


def test_the_village_starts_with_normal_soil_and_woods():
    world = create_world(Config(seed=1), RandomStreams(1))
    assert (world.soil == 1).all() and (world.woods == world.woods_normal).all()
    assert (world.woods < world.woods_capacity).all()
