from dataclasses import replace

import numpy as np

from econ_sim import council
from econ_sim.config import Config, CouncilConfig, ScheduledEvent, VillageConfig
from econ_sim.council import Councils
from econ_sim.scenarios import compare, effect
from econ_sim.simulation import Simulation

CONFIG = Config()


def formed(n=1, treasury=0.0, reserve=0.0):
    c = Councils.none(n)
    c.formed[:] = True
    c.treasury[:] = treasury
    c.reserve[:] = reserve
    return c


def test_council_forms_after_the_village_is_big_enough_for_long_enough():
    c = Councils.none(2)
    needed = CONFIG.council.forms_after_months
    for month in range(needed):
        new = council.check_formation(c, np.array([600.0, 100.0]), CONFIG)
        assert new.tolist() == ([True, False] if month == needed - 1 else [False, False])
    assert c.formed.tolist() == [True, False]


def test_shrinking_below_the_threshold_resets_the_count():
    c = Councils.none(1)
    council.check_formation(c, np.array([600.0]), CONFIG)
    council.check_formation(c, np.array([400.0]), CONFIG)
    assert c.months_ready.tolist() == [0]


def test_no_council_when_disabled():
    config = replace(CONFIG, council=CouncilConfig(enabled=False))
    c = Councils.none(1)
    for _ in range(12):
        council.check_formation(c, np.array([600.0]), config)
    assert not c.formed.any()


def test_taxes_stop_at_target_and_in_famine():
    income = np.array([10.0, 10.0, 10.0])
    c = formed(3, treasury=0.0)
    c.treasury[1] = 100.0  # already at target
    taxes = council.collect_taxes(income, np.array([0, 1, 2]), c, np.array([50.0, 50.0, 50.0]), np.array([False, False, True]), CONFIG)
    rate = CONFIG.council.tax_rate
    assert np.allclose(taxes, [10 * rate, 0.0, 0.0])
    assert np.allclose(income, [10 * (1 - rate), 10.0, 10.0])


def test_grain_levy_fills_the_reserve_up_to_target():
    c = formed(1, reserve=0.0)
    granary = np.array([1000.0])
    need = np.array([100.0])
    taken = council.levy_grain(c, np.array([500.0]), granary, need, np.array([False]), CONFIG)
    assert np.isclose(taken[0], CONFIG.council.grain_levy * 500.0)
    c.reserve[:] = CONFIG.council.reserve_months * need
    assert council.levy_grain(c, np.array([500.0]), granary, need, np.array([False]), CONFIG)[0] == 0.0


def test_no_grain_levy_in_famine():
    c = formed(1)
    assert council.levy_grain(c, np.array([500.0]), np.array([1000.0]), np.array([100.0]), np.array([True]), CONFIG)[0] == 0.0


def test_relief_fills_families_gaps_from_the_reserve():
    c = formed(1, reserve=5.0)
    given = council.give_relief(c, np.array([8.0, 2.0, 10.0]), np.array([10.0, 10.0, 10.0]), np.zeros(3, int), CONFIG)
    # 10 rations missing, 5 in the reserve: everyone gets half their gap.
    assert np.allclose(given, [1.0, 4.0, 0.0]) and c.reserve[0] == 0.0


def test_cash_relief_spends_only_beyond_what_the_council_keeps():
    c = formed(1, treasury=30.0)
    money = np.array([0.0, 5.0])
    given = council.cash_relief(c, money, np.array([20.0, 0.0]), np.zeros(2, int), np.array([20.0]), CONFIG)
    assert np.isclose(given[0], 10.0) and np.allclose(money, [10.0, 5.0]) and c.treasury[0] == 20.0


def test_no_relief_when_switched_off():
    config = replace(CONFIG, council=CouncilConfig(relief=False))
    c = formed(1, reserve=5.0, treasury=100.0)
    assert council.give_relief(c, np.array([0.0]), np.array([10.0]), np.zeros(1, int), config).sum() == 0
    assert council.cash_relief(c, np.zeros(1), np.array([5.0]), np.zeros(1, int), np.zeros(1), config).sum() == 0


def test_staff_are_hired_from_the_trades():
    sim = Simulation(replace(CONFIG, council=CouncilConfig(established_at_start=False)))
    pop, c = sim.world.population, sim.world.council
    c.formed[:] = True
    job = council.official_job(CONFIG)
    council.staff(pop, c, job, 4.0, CONFIG, np.random.default_rng(0))
    assert pop.count[pop.job == job].sum() == round(4.0 * pop.size / 1000)


def test_big_village_starts_with_an_established_council():
    sim = Simulation(CONFIG)
    c = sim.world.council
    assert c.formed.all() and c.reserve.sum() > 0 and c.treasury.sum() > 0


def test_growing_village_forms_a_council():
    config = replace(
        CONFIG, months=12, villages=(VillageConfig(population=520, land=182),),
        council=CouncilConfig(established_at_start=False),
    )
    sim = Simulation(config)
    sim.run()
    assert any("formed a council" in line for line in sim.log)
    assert sim.records[-1].councils == 1 and sim.records[-1].officials > 0


def test_small_village_never_forms_one():
    sim = Simulation(replace(CONFIG, months=24, villages=(VillageConfig(population=100, land=35),)))
    sim.run()
    assert sim.records[-1].councils == 0


def test_money_is_conserved_with_a_council():
    sim = Simulation(replace(CONFIG, months=36, scheduled_events=(ScheduledEvent("drought", 4),)))
    total = sim.world.money
    for _ in range(36):
        r = sim.step()
        assert np.isclose(r.savings + r.business_cash + r.treasury, total)


def test_council_relief_softens_a_drought():
    drought = (ScheduledEvent("drought", 4),)
    with_council = effect(compare(replace(CONFIG, months=24), drought, runs=6))
    without = effect(compare(replace(CONFIG, months=24, council=CouncilConfig(enabled=False)), drought, runs=6))
    assert with_council.poorest_lowest_ration[1] > without.poorest_lowest_ration[1]
    assert with_council.hungry_months[1] < without.hungry_months[1]
