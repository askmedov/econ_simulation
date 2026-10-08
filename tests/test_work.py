from dataclasses import replace

import numpy as np

from econ_sim import farms, households, work
from econ_sim.config import Config, DemographyConfig, WorkConfig
from econ_sim.households import Households
from econ_sim.population import NO_JOB, Population
from econ_sim.rng import RandomStreams
from econ_sim.simulation import Simulation
from econ_sim.world import create_world

CONFIG = Config()
FARM = CONFIG.farming
WEAVE = next(i for i, b in enumerate(CONFIG.businesses) if b.name == "weaving")


def people(ages, jobs, household, female=None):
    n = len(ages)
    return Population(
        count=np.ones(n, dtype=np.int64),
        age_months=np.asarray(ages) * 12,
        female=np.zeros(n, dtype=bool) if female is None else np.asarray(female),
        health=np.full(n, 100.0),
        skill=np.ones(n),
        location=np.zeros(n, dtype=np.int32),
        household=np.asarray(household),
        job=np.asarray(jobs),
        married=np.zeros(n, dtype=bool),
    )


def test_at_harvest_crafts_children_and_the_old_help_in_the_fields():
    # A farmer, a weaver, a child of 12, a child of 5 and a man of 65.
    pop = people([30, 30, 12, 5, 65], [FARM, WEAVE, NO_JOB, NO_JOB, NO_JOB], [0, 1, 1, 2, 2])
    cfg = CONFIG.work
    helping = work.harvest_help(pop, 1, 3, 8, CONFIG)
    assert np.isclose(helping.kept[WEAVE], 1 - cfg.craft_help) and helping.kept[FARM] == 1.0
    expected = cfg.craft_help + 2 * cfg.helper_effort
    assert np.isclose(helping.farm[0], expected)
    assert np.allclose(helping.households, [0.0, cfg.craft_help + cfg.helper_effort, cfg.helper_effort])
    # Outside the harvest nobody leaves their trade.
    off = work.harvest_help(pop, 1, 3, 2, CONFIG)
    assert off.farm[0] == 0 and (off.kept == 1).all()


def test_helpers_earn_a_share_of_the_harvest():
    pop = people([30, 12], [FARM, NO_JOB], [0, 1])
    hh = Households(np.zeros(2))
    helping = work.harvest_help(pop, 1, 2, 9, CONFIG)
    shares, _ = farms.income_shares(pop, hh, np.array([10.0]), CONFIG, extra_effort=helping.households)
    assert shares[1] > 0 and np.isclose(shares[1] / shares[0], CONFIG.work.helper_effort)


def test_the_outlook_counts_the_help_in_the_harvest_months_only():
    boost = work.harvest_boost(np.array([100.0]), np.array([20.0]), CONFIG)
    assert np.isclose(boost[0], 1.2 ** CONFIG.food.labor_share)
    ahead = work.outlook_boost(boost, 6, 12, CONFIG)  # in June: next month is July
    months = (6 + np.arange(12)) % 12 + 1
    harvest = np.isin(months, CONFIG.work.harvest_months)
    assert np.allclose(ahead[0, harvest], boost[0]) and np.allclose(ahead[0, ~harvest], 1.0)
    assert harvest[0]  # July


def test_women_spin_and_weave_at_home_outside_the_harvest():
    pop = people([30, 30, 70, 10], [FARM, FARM, NO_JOB, NO_JOB], [0, 1, 1, 1], female=[True, False, True, True])
    cloth = work.home_cloth(pop, 2, 1, CONFIG)
    assert np.allclose(cloth, [CONFIG.work.home_cloth, 0.0])  # women of working age only
    assert (work.home_cloth(pop, 2, 8, CONFIG) == 0).all()


def test_kin_give_grain_both_ways_up_to_their_spare_and_the_need():
    # 0 is the parents' family; 1 and 2 were founded by its sons; 3 is not kin.
    hh = Households(np.zeros(4), grain=np.array([100.0, 0.0, 50.0, 0.0]), kin=np.array([-1, 0, 0, -1]))
    alive = np.ones(4, dtype=bool)
    short = np.array([0.0, 30.0, 0.0, 40.0])
    spare = np.array([40.0, 0.0, 20.0, 0.0])
    moved = work.kin_help(hh, alive, short, spare, CONFIG)
    share = CONFIG.work.kin_share
    assert np.isclose(moved[1], share * 40.0) and np.isclose(moved[0], -share * 40.0)
    assert moved[2] == 0 and moved[3] == 0  # a sibling's family isn't linked; strangers get nothing
    # Children help their parents too, but never beyond what they lack.
    hh = Households(np.zeros(2), grain=np.array([0.0, 100.0]), kin=np.array([-1, 0]))
    moved = work.kin_help(hh, np.ones(2, dtype=bool), np.array([5.0, 0.0]), np.array([0.0, 80.0]), CONFIG)
    assert np.allclose(moved, [5.0, -5.0]) and np.allclose(hh.grain, [5.0, 95.0])


def test_no_help_from_a_family_that_is_gone_or_when_switched_off():
    hh = Households(np.zeros(2), grain=np.array([100.0, 0.0]), kin=np.array([-1, 0]))
    gone = np.array([False, True])
    assert (work.kin_help(hh, gone, np.array([0.0, 10.0]), np.array([50.0, 0.0]), CONFIG) == 0).all()
    off = replace(CONFIG, work=WorkConfig(enabled=False))
    assert (work.kin_help(hh, np.ones(2, dtype=bool), np.array([0.0, 10.0]), np.array([50.0, 0.0]), off) == 0).all()


def test_a_new_household_is_kin_to_the_grooms_family():
    config = replace(CONFIG, demography=replace(DemographyConfig(), marriage_chance=1.0))
    # Both families already have parents and an heir couple at home, so a
    # younger son of family 0 and a younger daughter of family 1 set up a
    # home of their own.
    pop = people([50, 48, 25, 22, 20, 50, 47, 24, 23, 19], [FARM] * 10, [0, 0, 0, 0, 0, 1, 1, 1, 1, 1],
                 female=[False, True, False, True, False, False, True, False, True, True])
    pop.married[:] = [True, True, True, True, False, True, True, True, True, False]
    pop.couple[:] = [0, 0, 1, 1, -1, 2, 2, 3, 3, -1]
    hh = Households(np.zeros(2))
    assert households.marry(pop, hh, config, np.random.default_rng(0)) == 1
    assert pop.household[4] == pop.household[9] == 2 and len(hh) == 3
    assert hh.kin.tolist() == [-1, -1, 0]


def test_families_start_linked_to_a_family_a_generation_older():
    world = create_world(Config(seed=1), RandomStreams(1))
    hh = world.households
    linked = hh.kin >= 0
    assert 0.2 < linked.mean() < 0.95
    oldest = np.full(len(hh), -1)
    np.maximum.at(oldest, world.population.household, world.population.age_years.astype(int))
    gap = oldest[hh.kin[linked]] - oldest[linked]
    assert (gap >= 18).all() and (gap <= 45).all()
    assert (hh.location[hh.kin[linked]] == hh.location[linked]).all()


def test_household_work_shows_in_the_records():
    records = Simulation(Config(seed=2, months=24)).run()
    assert sum(r.homespun for r in records) > 0
    assert all(r.harvest_help > 0 for r in records if r.month in CONFIG.work.harvest_months)
    assert all(r.harvest_help == 0 for r in records if r.month not in CONFIG.work.harvest_months)
    off = Simulation(Config(seed=2, months=24, work=WorkConfig(enabled=False))).run()
    assert sum(r.homespun + r.harvest_help + r.kin_help for r in off) == 0
