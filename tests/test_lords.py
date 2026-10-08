from dataclasses import replace

import numpy as np

from econ_sim import lords
from econ_sim.config import Config, CouncilConfig, LordConfig, ScheduledEvent, StateConfig
from econ_sim.households import Households
from econ_sim.lords import Lords
from econ_sim.simulation import Simulation

CONFIG = Config()


def test_the_lord_holds_his_demesne_unless_there_is_no_lord():
    land = np.array([100.0, 40.0])
    assert np.allclose(lords.take_demesne(land, CONFIG), land * CONFIG.lord.demesne)
    no_lord = replace(CONFIG, lord=LordConfig(enabled=False))
    assert (lords.take_demesne(land, no_lord) == 0).all()


def test_the_barn_is_carted_away_and_sold_for_coins_outside_the_village():
    lord = Lords.none(1)
    lords.into_barn(lord, np.array([100.0]))
    away = lords.cart_away(lord, CONFIG)
    assert np.isclose(away[0], 100.0 * CONFIG.lord.carted_away) and np.isclose(lord.barn[0], 100.0 - away[0])
    offered = lords.offer(lord, CONFIG)
    lords.sold(lord, offered, np.array([2.0]))
    assert np.isclose(lord.purse[0], 2.0 * offered[0]) and np.isclose(lord.barn[0], 80.0 - offered[0])


def test_a_charitable_lord_opens_his_barn_only_in_a_famine():
    gap = np.array([5.0, 15.0])
    location = np.zeros(2, dtype=np.int32)
    charitable = replace(CONFIG, lord=LordConfig(charity_in_famine=True))
    lord = Lords.none(1)
    lord.barn[:] = 10.0
    assert (lords.charity(lord, gap, location, np.array([False]), charitable) == 0).all()
    given = lords.charity(lord, gap, location, np.array([True]), charitable)
    assert np.allclose(given, [2.5, 7.5]) and lord.barn[0] == 0  # the barn runs out, shared by need
    lord.barn[:] = 10.0
    assert (lords.charity(lord, gap, location, np.array([True]), CONFIG) == 0).all()  # not by default


def test_the_state_takes_coins_then_seizes_grain():
    hh = Households(np.zeros(3), money=np.array([100.0, 1.0, 0.0]), land=np.array([2.0, 0.0, 0.0]),
                    grain=np.array([0.0, 50.0, 0.0]))
    lord = Lords.none(1)
    wage, rent, price = np.array([4.0]), np.array([10.0]), np.array([1.0])
    take = lords.collect_state_tax(hh, lord, wage, rent, price, np.array([False]), CONFIG)
    hearth = CONFIG.state.hearth_tax_months * 4.0
    first = hearth + CONFIG.state.land_tax * 2.0 * 10.0
    assert np.isclose(hh.money[0], 100.0 - first)
    assert hh.money[1] == 0 and np.isclose(hh.grain[1], 50.0 - (hearth - 1.0))  # the rest in grain
    assert np.isclose(take.coins[0], first + 1.0) and np.isclose(lord.state_purse[0], take.coins[0])
    assert np.isclose(take.grain[0], hearth - 1.0)  # the third family has nothing: let go


def test_the_state_remits_its_tax_in_a_famine_if_told_to():
    hh = Households(np.zeros(1), money=np.array([100.0]))
    remitting = replace(CONFIG, state=StateConfig(remit_in_famine=True))
    take = lords.collect_state_tax(hh, Lords.none(1), np.array([4.0]), np.zeros(1), np.ones(1), np.array([True]), remitting)
    assert take.coins[0] == 0 and hh.money[0] == 100.0


def test_soldiers_take_a_share_of_everything():
    hh = Households(np.zeros(2), grain=np.array([10.0, 30.0]), animals=np.array([2.0, 0.0]))
    lord = Lords.none(1)
    lord.barn[:] = 20.0
    farm = np.array([8.0])
    village, barn = lords.requisition(hh, lord, farm, np.array([0.25]))
    assert np.isclose(village[0], 10.0 + 2.0) and np.isclose(barn[0], 5.0)
    assert np.allclose(hh.grain, [7.5, 22.5]) and np.isclose(hh.animals[0], 1.5) and np.isclose(farm[0], 6.0)


def test_lord_relief_is_reported_apart_from_the_councils():
    config = Config(seed=0, months=24, council=CouncilConfig(enabled=False), lord=LordConfig(charity_in_famine=True),
                    scheduled_events=(ScheduledEvent("drought", 4),))
    records = Simulation(config).run()
    assert sum(r.relief for r in records) == 0
    assert sum(r.lord_relief for r in records) > 0
