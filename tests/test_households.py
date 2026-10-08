from dataclasses import replace

import numpy as np

from econ_sim import economy, households, rules
from econ_sim.config import Config, VillageConfig
from econ_sim.population import NO_JOB, Population
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
    heads = pop.female & (pop.age_years >= 18) & (pop.age_years <= 64)
    assert set(pop.household[heads]) == set(range(len(world.households)))


def test_children_live_with_a_woman_old_enough_to_be_their_mother():
    world, _ = village()
    pop = world.population
    heads = pop.female & pop.married & (pop.age_years >= 18) & (pop.age_years <= 64)
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


def families(members, money=None):
    """Households from (household, age, female, married) tuples."""
    household, age, female, married = (np.array(x) for x in zip(*members))
    n = len(members)
    pop = Population(
        count=np.ones(n, dtype=np.int64),
        age_months=age * 12,
        female=female,
        health=np.full(n, 100.0),
        skill=np.ones(n),
        location=np.zeros(n),
        household=household,
        married=married,
    )
    n_households = household.max() + 1
    return pop, households.Households(np.zeros(n_households), money)


def sure_weddings(config):
    return replace(config, demography=replace(config.demography, marriage_chance=1.0))


def test_some_young_people_start_out_single():
    world, _ = village()
    pop = world.population
    young_women = pop.female & (pop.age_years >= 18) & (pop.age_years <= 22)
    assert 0.2 < (~pop.married[young_women]).mean() < 0.9
    assert pop.married[pop.age_years >= 40].all()
    assert not pop.married[pop.age_years < 18].any()


def test_the_first_to_marry_stays_as_heir_and_the_rest_leave():
    config = sure_weddings(Config())
    pop, hh = families(
        [
            (0, 50, False, True), (0, 48, True, True), (0, 22, False, False), (0, 21, False, False),
            (1, 52, False, True), (1, 47, True, True), (1, 20, True, False), (1, 19, True, False),
        ],
        money=np.array([80.0, 80.0]),
    )
    weddings = households.marry(pop, hh, config, np.random.default_rng(0))
    assert weddings == 2
    # One couple stays with the groom's family, the other with the bride's.
    assert len(hh) == 2
    sons, daughters = np.flatnonzero(pop.age_years < 23)[:2], np.flatnonzero(pop.age_years < 23)[2:]
    assert sorted(pop.household[sons]) == [0, 1] and sorted(pop.household[daughters]) == [0, 1]
    assert pop.married.all()
    # A third wedding into either family would set up a new household.
    pop.append(families([(0, 20, False, False), (1, 18, True, False)])[0])
    assert households.marry(pop, hh, config, np.random.default_rng(1)) == 1
    assert len(hh) == 3 and (pop.household[-2:] == 2).all()


def test_brothers_and_sisters_of_an_orphaned_family_do_not_all_stay():
    config = sure_weddings(Config())
    # Parents gone: the eldest brother married in; his younger brother leaves.
    pop, hh = families(
        [(0, 30, False, True), (0, 28, True, True), (0, 24, False, False), (1, 45, True, True), (1, 20, True, False)]
    )
    pop.married[3] = True
    households.marry(pop, hh, config, np.random.default_rng(0))
    assert pop.household[2] == pop.household[4] == 1  # with the bride's family


def test_whoever_leaves_takes_their_share_of_the_savings():
    config = sure_weddings(Config())
    pop, hh = families(
        [(0, 40, True, True), (0, 42, False, True), (0, 20, True, False), (0, 2, True, False),
         (1, 70, True, True), (1, 68, False, True), (1, 40, True, True), (1, 41, False, True), (1, 21, False, False)],
        money=np.array([40.0, 100.0]),
    )
    households.marry(pop, hh, config, np.random.default_rng(0))
    # The groom's family already has its heir couple; the bride's has room.
    assert pop.household[8] == 0
    assert np.isclose(hh.money[1], 100.0 - 100.0 / 5) and np.isclose(hh.money[0], 40.0 + 100.0 / 5)
    assert np.isclose(hh.money.sum(), 140.0)


def test_nobody_marries_when_no_one_is_willing():
    config = sure_weddings(Config())
    pop, hh = families([(0, 50, True, True), (0, 20, True, False), (1, 50, True, True), (1, 22, False, False)])
    assert households.marry(pop, hh, config, np.random.default_rng(0), willingness=np.zeros(1)) == 0
    assert households.marry(pop, hh, config, np.random.default_rng(0), willingness=np.ones(1)) == 1


def test_families_in_a_long_run_stay_family_sized():
    from econ_sim.simulation import Simulation

    sim = Simulation(Config(seed=3, months=12 * 15))
    sim.run()
    size = households.sizes(sim.world.population, len(sim.world.households))
    assert 3.5 < size[size > 0].mean() < 6.5
    assert sum(r.weddings for r in sim.records) > 50
