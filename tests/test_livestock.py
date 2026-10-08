from dataclasses import replace

import numpy as np

from econ_sim import economy, farms, livestock, rules
from econ_sim.config import Config, ScheduledEvent
from econ_sim.households import Households
from econ_sim.simulation import Simulation

CONFIG = Config()
CFG = CONFIG.livestock


def test_plough_animals_raise_farm_output_up_to_a_full_set():
    land = np.array([100.0, 100.0, 100.0])
    factor = livestock.farm_factor(np.array([0.0, 100 * CFG.per_plot / 2, 1000.0]), land, CONFIG)
    assert np.allclose(factor, [1.0, 1 + CFG.farm_boost / 2, 1 + CFG.farm_boost])
    assert np.isclose(livestock.owners_part(factor)[2], CFG.farm_boost / (1 + CFG.farm_boost))


def test_herds_grow_in_good_years_and_die_off_in_droughts():
    good = Households(np.zeros(1), animals=np.array([100.0]))
    livestock.grow_and_die(good, np.ones(1), np.ones(1), CONFIG)
    assert good.animals[0] > 100.0
    bad = Households(np.zeros(1), animals=np.array([100.0]))
    lost = livestock.grow_and_die(bad, np.array([0.6]), np.array([1.5]), CONFIG)
    assert bad.animals[0] < 100.0 and lost[0] > 0


def test_the_winter_cull_thins_herds_to_what_can_be_fed_and_stores_the_meat():
    hh = Households(np.zeros(2), animals=np.array([30.0, 10.0]))
    land = np.array([100.0])  # room for 100 x winter_capacity_per_plot
    room = CFG.winter_capacity_per_plot * 100
    culled, meat = livestock.winter_cull(hh, land, CONFIG)
    assert np.isclose(hh.animals.sum(), room) and np.isclose(culled[0], 40 - room)
    assert np.isclose(hh.animals[0] / hh.animals[1], 3.0)  # everyone in proportion
    assert np.isclose(hh.grain.sum(), meat[0]) and np.isclose(meat[0], culled[0] * CFG.meat_rations)


def test_families_without_food_or_coins_slaughter_their_animals():
    hh = Households(np.zeros(3), money=np.array([0.0, 100.0, 0.0]), grain=np.array([0.0, 0.0, 50.0]),
                    animals=np.array([5.0, 5.0, 5.0]))
    killed, meat = livestock.slaughter_in_hunger(hh, np.full(3, 10.0), np.ones(1), CONFIG)
    assert hh.animals[0] < 5.0 and hh.animals[1] == 5.0 and hh.animals[2] == 5.0
    assert np.isclose(hh.grain[0], 20.0) and np.isclose(meat[0], 20.0)


def test_distress_sales_drive_the_price_down():
    worth = np.array([10.0])

    def sell(sellers):
        hh = Households(np.zeros(sellers + 1), money=np.array([0.0] * sellers + [100.0]),
                        animals=np.array([4.0] * sellers + [0.0]))
        short = np.array([30.0] * sellers + [0.0])
        spare = np.array([0.0] * sellers + [100.0])
        before = hh.money.sum()
        sales = livestock.distress_sales(hh, short, spare, worth, CONFIG)
        assert np.isclose(hh.money.sum(), before) and np.isclose(hh.animals.sum(), 4.0 * sellers)
        return sales, hh

    few, hh = sell(1)
    assert np.isclose(few.price[0], 10.0) and np.isclose(hh.animals[-1], 3.0)  # 3 animals raise 30 coins
    many, hh = sell(20)
    assert many.price[0] < few.price[0]
    assert many.price[0] >= CFG.lowest_price * 10.0 - 1e-9
    assert hh.money[-1] >= 100.0 * (1 - CFG.buyers_spend) - 1e-9


def test_seed_is_picked_at_harvest_but_never_more_than_a_share_of_it():
    assert np.isclose(sum(farms.seed_plan(m, CONFIG) for m in range(1, 13)), 1.0)
    assert farms.seed_plan(3, CONFIG) == 0.0
    needed = np.array([600.0])
    seed, grain = np.zeros(1), np.array([1000.0])
    kept = farms.keep_seed(grain, seed, needed, np.array([1000.0]), 8, CONFIG)
    assert np.isclose(kept[0], 600 * farms.seed_plan(8, CONFIG))
    # A poor harvest: no more than max_seed_share of it.
    seed, grain = np.zeros(1), np.array([100.0])
    kept = farms.keep_seed(grain, seed, needed, np.array([100.0]), 8, CONFIG)
    assert np.isclose(kept[0], CONFIG.food.max_seed_share * 100)
    # Nothing outside the seed months.
    assert farms.keep_seed(np.array([1000.0]), np.zeros(1), needed, np.array([1000.0]), 4, CONFIG)[0] == 0


def test_short_seed_is_made_up_by_landholders_at_sowing():
    hh = Households(np.zeros(2), land=np.array([10.0, 0.0]), grain=np.array([200.0, 200.0]))
    seed = np.array([50.0])
    sown, given = farms.sow(hh, seed, np.array([100.0]), np.full(2, 10.0))
    assert np.isclose(sown[0], 1.0) and np.isclose(given[0], 50.0)
    assert np.isclose(hh.grain[0], 150.0) and hh.grain[1] == 200.0 and seed[0] == 0
    hh = Households(np.zeros(1), land=np.array([10.0]), grain=np.array([30.0]))
    sown, _ = farms.sow(hh, np.array([50.0]), np.array([100.0]), np.full(1, 10.0))
    assert np.isclose(sown[0], 0.6)  # they keep two months of food


def test_harvests_follow_the_seed_sown_until_the_next_sowing():
    outlook = farms.sown_outlook(np.array([0.5]), 3, 12, CONFIG)[0]
    # From April to October this year's sowing (half the plots, worked
    # harder); after it, a full one.
    assert np.allclose(outlook[:7], 0.5 ** (1 - CONFIG.food.labor_share)) and np.allclose(outlook[7:], 1.0)


def test_few_farmers_farm_only_the_land_that_repays_its_seed():
    land = np.array([350.0])
    per_plot = 12 * 1.67 * 1.25 * 1.3
    many = economy.land_in_use(land, np.array([400.0]), CONFIG, per_plot)
    few = economy.land_in_use(land, np.array([20.0]), CONFIG, per_plot)
    assert many[0] == 350.0 and few[0] < 350.0 / 4


def test_a_severe_drought_cuts_next_years_sowing_and_a_mild_one_does_not():
    def sown_after(mult):
        events = tuple(
            replace(e, effects={"production_mult": mult}) if e.name == "drought" else e for e in CONFIG.events
        )
        sim = Simulation(replace(CONFIG, seed=1, months=12, random_events=False, events=events,
                                 scheduled_events=(ScheduledEvent("drought", 4),)))
        sim.run()
        return sim.world.sown[0]

    assert sown_after(0.8) == 1.0
    assert sown_after(0.2) < 0.9


def test_animals_change_hands_in_a_famine():
    sim = Simulation(replace(CONFIG, seed=0, months=36, scheduled_events=(ScheduledEvent("drought", 4),
                                                                          ScheduledEvent("drought", 16))))
    records = sim.run()
    assert sum(r.animals_sold for r in records) + sum(r.meat for r in records) > 0
    assert all(r.animals >= 0 for r in records)
