from dataclasses import replace

import numpy as np

from econ_sim import economy, households, rules
from econ_sim.config import Config, VillageConfig
from econ_sim.population import NO_JOB
from econ_sim.rng import RandomStreams
from econ_sim.world import create_world


def village(population=1000, villages=1, seed=0):
    config = Config(seed=seed, villages=tuple(VillageConfig(population=population) for _ in range(villages)))
    return create_world(config, RandomStreams(seed)), config


def test_everyone_belongs_to_a_household_of_their_village():
    world, _ = village(villages=2)
    pop = world.population
    assert (pop.household >= 0).all()
    assert (world.households.location[pop.household] == pop.location).all()


def test_households_are_family_sized():
    world, _ = village()
    size = households.sizes(world.population, len(world.households))
    assert 3.5 < size.mean() < 6.5
    assert size.min() >= 1


def test_every_household_starts_with_a_woman_of_family_age():
    world, _ = village()
    pop = world.population
    heads = pop.female & (pop.age_years >= 18) & (pop.age_years <= 49)
    assert set(pop.household[heads]) == set(range(len(world.households)))


def test_children_live_with_a_woman_old_enough_to_be_their_mother():
    world, _ = village()
    pop = world.population
    heads = pop.female & (pop.age_years >= 18) & (pop.age_years <= 49)
    head_age = dict(zip(pop.household[heads], pop.age_years[heads]))
    children = np.flatnonzero(pop.age_years < 18)
    ok = [18 <= head_age[pop.household[c]] - pop.age_years[c] <= 45 for c in children]
    assert np.mean(ok) > 0.9


def test_a_village_without_women_of_family_age_lives_together():
    world, _ = village(population=3, seed=5)
    assert len(world.households) >= 1
    assert (world.population.household >= 0).all()


def test_newborns_join_their_mothers_household():
    world, config = village()
    pop = world.population
    sure = replace(config, demography=replace(config.demography, annual_birth_chance=1.0))
    before = len(pop)
    rules.births(pop, np.ones(1), sure, np.random.default_rng(0), 1)
    babies = np.arange(before, len(pop))
    assert len(babies) > 0
    fertile = pop.female & (pop.age_years >= 16) & (pop.age_years <= 45)
    assert set(pop.household[babies]) <= set(pop.household[fertile])


def test_working_age_people_have_jobs_and_others_do_not():
    world, config = village()
    pop = world.population
    working = rules.is_working_age(pop, config)
    assert (pop.job[working] != NO_JOB).all()
    assert (pop.job[~working] == NO_JOB).all()


def test_jobs_follow_people_as_they_age():
    world, config = village()
    pop = world.population
    wanted = np.array([[0.0, 0.0, 10.0, 0.0]])  # only weaving is short of workers
    pop.age_months[:] = 14 * 12 + 11  # everyone about to turn 15
    economy.assign_new_workers(pop, wanted, config)
    assert (pop.job == NO_JOB).all()
    rules.grow_older(pop)
    economy.assign_new_workers(pop, wanted, config)
    assert (pop.job == config.business_index("weaving")).all()
    pop.age_months[:] = 60 * 12  # everyone retires
    economy.assign_new_workers(pop, wanted, config)
    assert (pop.job == NO_JOB).all()
