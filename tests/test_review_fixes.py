"""Regression tests for bugs found in a review of Phase 2b."""

from dataclasses import replace

import numpy as np
import pytest

from econ_sim import credit, farms, households, lords
from econ_sim.config import Config, DemographyConfig, LandConfig, VillageConfig
from econ_sim.households import Households
from econ_sim.lords import Lords
from econ_sim.population import Population
from econ_sim.simulation import Simulation

CONFIG = Config()
FARM = CONFIG.farming


def people(ages, household, female, married):
    n = len(ages)
    return Population(
        count=np.ones(n, dtype=np.int64),
        age_months=np.asarray(ages) * 12,
        female=np.asarray(female),
        health=np.full(n, 100.0),
        skill=np.ones(n),
        location=np.zeros(n, dtype=np.int32),
        household=np.asarray(household),
        job=np.full(n, FARM),
        married=np.asarray(married),
    )


def test_seed_about_to_be_picked_is_known_before_it_is_picked():
    farm_grain, seed = np.array([100.0]), np.array([10.0])
    due = farms.seed_due(farm_grain, seed, np.array([60.0]), np.array([100.0]), 9, CONFIG)
    assert farm_grain[0] == 100.0 and seed[0] == 10.0  # nothing picked yet
    kept = farms.keep_seed(farm_grain, seed, np.array([60.0]), np.array([100.0]), 9, CONFIG)
    assert np.allclose(due, kept) and kept[0] > 0


def test_in_a_drought_the_harvest_months_count_as_famine():
    # The seed picked at harvest is not food: a drought harvest is a famine.
    from econ_sim.config import CouncilConfig, ScheduledEvent

    flagged = 0
    for seed in range(4):
        config = Config(seed=seed, months=10, random_events=False, council=CouncilConfig(enabled=False),
                        scheduled_events=(ScheduledEvent("drought", 4),))
        records = Simulation(config).run()
        flagged += sum(r.food_cover < 1.0 for r in records if r.month in (8, 9, 10))
    assert flagged >= 6


@pytest.mark.parametrize("month, gathered", [(7, 0.0), (11, 0.0), (12, 0.0)])
def test_no_seed_in_store_outside_the_harvest(month, gathered):
    assert farms.seed_gathered(month, CONFIG) == gathered
    sim = Simulation(Config(seed=1, months=1, start_month=month))
    sim.step()
    assert farms.seed_gathered(month, CONFIG) == 0.0
    assert sim.world.seed[0] <= sim.records[0].food_to_seed + 1e-9


def test_savings_of_the_dead_stay_in_their_village():
    hh = Households(np.array([0, 0]), money=np.array([10.0, 5.0]))
    treasury = np.zeros(3)  # villages 1 and 2 have no households
    households.pass_on_savings(hh, np.array([0, 2]), treasury, np.array([True, True, True]))
    assert treasury.tolist() == [10.0, 0.0, 0.0]


def test_a_last_member_who_marries_out_takes_the_family_with_them():
    config = replace(CONFIG, demography=replace(DemographyConfig(), marriage_chance=1.0),
                     land=LandConfig(partible=False))
    # Family 0: an orphan daughter of 19, alone, with land, an ox and a debt.
    # Family 1: parents and their son of 22, the heir: she moves in with them.
    pop = people([19, 50, 47, 22], [0, 1, 1, 1], female=[True, False, True, False],
                 married=[False, True, True, False])
    pop.couple[:] = [-1, 0, 0, -1]
    hh = Households(np.zeros(3), money=np.array([12.0, 30.0, 0.0]), land=np.array([4.0, 6.0, 0.0]),
                    animals=np.array([1.0, 2.0, 0.0]), debt=np.array([7.0, 0.0, 0.0]), lent=np.array([0.0, 2.0, 5.0]),
                    kin=np.array([-1, 0, 0]))
    assert households.marry(pop, hh, config, np.random.default_rng(0)) == 1
    assert pop.household[0] == 1
    assert hh.land[0] == hh.animals[0] == hh.debt[0] == hh.money[0] == 0
    assert hh.land[1] == 10.0 and hh.animals[1] == 3.0 and hh.debt[1] == 7.0 and hh.money[1] == 42.0
    assert hh.land.sum() == 10.0 and hh.debt.sum() == 7.0 and hh.lent.sum() == 7.0
    assert hh.kin.tolist() == [-1, -1, 1]  # her kin are now her husband's family's


def test_borrowers_pay_off_debt_from_coins_they_can_spare():
    hh = Households(np.zeros(2), money=np.array([50.0, 0.0]), debt=np.array([30.0, 0.0]), lent=np.array([0.0, 30.0]))
    credit.repay(hh, np.array([10.0, 0.0]), CONFIG, spare=np.array([20.0, 0.0]))
    paid = 30.0 - hh.debt[0]
    assert np.isclose(paid, CONFIG.credit.repay_share * 10.0 + 20.0)
    assert np.isclose(hh.debt.sum(), hh.lent.sum())


def test_households_with_nobody_left_pay_no_state_tax():
    hh = Households(np.zeros(2), money=np.array([10.0, 10.0]))
    take = lords.collect_state_tax(hh, Lords.none(1), np.array([4.0]), np.zeros(1), np.ones(1), np.array([False]),
                                   CONFIG, living=np.array([True, False]))
    assert hh.money[1] == 10.0 and np.isclose(take.coins[0], CONFIG.state.hearth_tax_months * 4.0)


def test_a_village_needs_people():
    with pytest.raises(ValueError):
        Simulation(replace(Config(), villages=(VillageConfig(), VillageConfig(name="empty", population=0))))
