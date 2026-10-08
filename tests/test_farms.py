from dataclasses import replace

import numpy as np

from econ_sim import farms, households, rules
from econ_sim.config import Config, LandConfig, VillageConfig
from econ_sim.households import Households
from econ_sim.population import Population
from econ_sim.rng import RandomStreams
from econ_sim.simulation import Simulation
from econ_sim.world import create_world

CONFIG = Config()
FARM = CONFIG.farming


def workers(jobs, household, location=None):
    n = len(jobs)
    return Population(
        count=np.ones(n, dtype=np.int64),
        age_months=np.full(n, 30 * 12),
        female=np.zeros(n, dtype=bool),
        health=np.full(n, 100.0),
        skill=np.ones(n),
        location=np.zeros(n) if location is None else location,
        household=household,
        job=jobs,
        married=np.ones(n, dtype=bool),
    )


def test_harvest_goes_to_workers_by_work_and_to_landholders_by_land():
    # Household 0 farms and holds no land; household 1 holds all the land and
    # nobody in it farms; household 2 is a smith's family.
    pop = workers([FARM, 3, 3], [0, 1, 2])
    hh = Households(np.zeros(3), land=np.array([0.0, 10.0, 0.0]))
    shares = farms.income_shares(pop, hh, np.array([10.0]), CONFIG)
    a = CONFIG.food.labor_share
    assert np.allclose(shares, [a, 1 - a, 0.0])
    farm_grain = np.array([100.0])
    received = farms.share_harvest(hh, farm_grain, np.array([20.0]), shares)
    assert np.allclose(received, [80 * a, 80 * (1 - a), 0.0])
    assert np.isclose(farm_grain[0], 20.0) and np.allclose(hh.grain, received)


def test_with_nobody_holding_land_the_workers_get_it_all():
    pop = workers([FARM, FARM], [0, 1])
    hh = Households(np.zeros(2))
    assert np.allclose(farms.income_shares(pop, hh, np.array([10.0]), CONFIG), [0.5, 0.5])


def test_stock_to_keep_is_what_a_full_ration_needs():
    need = np.array([10.0, 10.0])
    outlook = np.array([[0.0] * 12, [20.0] * 12])
    keep = rules.stock_to_keep(need, outlook, CONFIG)
    # Just enough to keep a full ration; a little less forces belt-tightening.
    assert np.allclose(rules.plan_ration(keep, need, outlook, CONFIG), 1.0)
    assert (rules.plan_ration(keep * 0.97, need, outlook, CONFIG) < 1.0).all()
    assert np.isclose(keep[1], 10.0)  # harvests cover the future: only this month's need


def test_families_eat_from_their_store_sell_their_spare_and_buy_what_they_lack():
    hh = Households(np.zeros(3), grain=np.array([0.0, 30.0, 500.0]))
    need = np.full(3, 10.0)
    no_harvest = np.zeros((1, 12))
    plan = farms.plan_family_food(hh, need, no_harvest, np.ones(3), np.ones(1), CONFIG)
    assert plan.own[0] == 0 and plan.want[0] == 10.0  # nothing in store: buy it all
    assert 0 < plan.own[1] < 10 and plan.spare[1] == 0  # a short store is spread out
    assert plan.own[2] == 10.0 and plan.want[2] == 0  # plenty: eat fully
    assert plan.spare[2] > 0 and 500 - plan.spare[2] > 12 * 10 * 0.85  # but keep a year's food


def test_the_farm_store_sells_first_then_families_by_what_they_offered():
    sold_by_family, by_farms = farms.sellers_share(
        np.array([50.0]), np.array([30.0, 90.0]), np.array([20.0]), np.zeros(2, dtype=np.int32)
    )
    assert by_farms[0] == 20.0 and np.allclose(sold_by_family, [7.5, 22.5])


def test_land_is_spread_with_some_families_landless():
    world = create_world(Config(seed=1), RandomStreams(1))
    hh = world.households
    assert np.isclose(hh.land.sum(), world.land[0])
    landless = (hh.land == 0).mean()
    assert 0.15 < landless < 0.45
    holders = hh.land[hh.land > 0]
    assert holders.max() > 4 * np.median(holders)  # a few big farms
    # Food in store starts with the families, by need and land.
    assert np.isclose(hh.grain.sum(), world.granary[0]) and world.farm_grain[0] == 0
    assert (hh.grain > 0).all()


def test_a_vacant_holding_goes_to_a_landless_family():
    hh = Households(np.zeros(4), land=np.array([5.0, 3.0, 0.0, 0.0]), grain=np.array([10.0, 0, 0, 0]))
    size = np.array([0, 3, 2, 4])
    reserve = np.zeros(1)
    to_reserve = households.pass_on_land_and_grain(hh, size, reserve, np.array([True]))
    assert hh.land.tolist() == [0.0, 3.0, 0.0, 5.0]  # the newest landless family
    assert to_reserve[0] == reserve[0] == 10.0 and hh.grain[0] == 0


def test_without_a_council_grain_of_the_dead_goes_to_the_neighbours():
    hh = Households(np.zeros(3), grain=np.array([12.0, 0.0, 0.0]))
    households.pass_on_land_and_grain(hh, np.array([0, 1, 2]), np.zeros(1), np.array([False]))
    assert np.allclose(hh.grain, [0.0, 4.0, 8.0])


def test_heirs_keep_the_land_unless_it_is_split():
    def wed(partible):
        config = replace(Config(), demography=replace(Config().demography, marriage_chance=1.0),
                         land=LandConfig(partible=partible))
        pop = workers([FARM] * 5, [0, 0, 0, 1, 1])
        pop.age_months[:] = np.array([50, 48, 22, 50, 20]) * 12
        pop.female[:] = [False, True, False, False, True]
        pop.married[:] = [True, True, False, True, False]
        hh = Households(np.zeros(2), land=np.array([6.0, 4.0]), grain=np.array([30.0, 20.0]))
        households.marry(pop, hh, config, np.random.default_rng(0))
        return pop, hh

    pop, hh = wed(partible=False)
    # The groom is his family's first to marry: the bride moves in with her share of grain, no land.
    assert pop.household[4] == 0 and hh.land.tolist() == [6.0, 4.0]
    assert np.isclose(hh.grain[0], 30.0 + 20.0 / 2)
    pop, hh = wed(partible=True)
    assert np.isclose(hh.land[0], 6.0 + 4.0 / 2) and np.isclose(hh.land.sum(), 10.0)


def test_most_food_is_eaten_from_families_own_stores():
    records = Simulation(Config(seed=2, months=36)).run()
    eaten = sum(r.food_eaten for r in records)
    own = sum(r.own_food for r in records)
    assert 0.6 < own / eaten < 0.95
    assert sum(r.grain_sold for r in records) > 0


def test_in_a_drought_the_landless_go_hungrier_than_the_village():
    from econ_sim.config import CouncilConfig, ScheduledEvent

    config = Config(seed=0, months=24, council=CouncilConfig(enabled=False),
                    scheduled_events=(ScheduledEvent("drought", 4),))
    worst_village, worst_landless = [], []
    for seed in range(4):
        records = Simulation(replace(config, seed=seed)).run()
        worst_village.append(min(r.ration for r in records))
        worst_landless.append(min(r.landless_ration for r in records))
    assert np.mean(worst_landless) < np.mean(worst_village) - 0.05


def test_two_villages_each_share_their_own_land():
    config = Config(seed=1, villages=(VillageConfig(name="A"), VillageConfig(name="B", land=200)))
    world = create_world(config, RandomStreams(1))
    hh = world.households
    for v, land in enumerate([350.0, 200.0]):
        assert np.isclose(hh.land[hh.location == v].sum(), land)
