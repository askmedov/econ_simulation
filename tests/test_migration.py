from dataclasses import replace

import numpy as np

from econ_sim import migration
from econ_sim.checks import broken_invariants
from econ_sim.config import Config, LordConfig, MigrationConfig, ScheduledEvent, VillageConfig
from econ_sim.households import Households
from econ_sim.migration import Memory
from econ_sim.population import NO_JOB, Population
from econ_sim.simulation import Simulation

CONFIG = Config()
CFG = CONFIG.migration


def people(ages, household, married=None, female=None, location=None):
    n = len(ages)
    return Population(
        count=np.ones(n, dtype=np.int64),
        age_months=np.asarray(ages) * 12,
        female=np.zeros(n, dtype=bool) if female is None else np.asarray(female),
        health=np.full(n, 100.0),
        skill=np.ones(n),
        location=np.zeros(n, dtype=np.int32) if location is None else np.asarray(location),
        household=np.asarray(household),
        job=np.full(n, NO_JOB),
        married=np.zeros(n, dtype=bool) if married is None else np.asarray(married),
    )


def calm(n=1):
    return Memory.calm(n)


def usual_land(people):
    return np.asarray(people, dtype=np.float64) / CONFIG.environment.people_per_plot


def test_an_ordinary_village_is_an_ordinary_place_to_live():
    people = np.array([1000.0])
    place = migration.appeal(np.array([CFG.usual_prospects]), calm(), people, usual_land(people),
                             np.array([CFG.usual_take]), CONFIG)
    assert np.isclose(place[0], 1.0)


def test_hunger_danger_smallness_and_a_harsh_lord_make_a_worse_place():
    n = 5
    memory = calm(n)
    memory.hunger[1] = 0.2
    memory.danger[2] = 0.1
    people = np.array([1000.0, 1000.0, 1000.0, 10.0, 1000.0])
    take = np.array([CFG.usual_take] * 4 + [2 * CFG.usual_take])
    place = migration.appeal(np.full(n, CFG.usual_prospects), memory, people, usual_land(people), take, CONFIG)
    assert np.isclose(place[0], 1.0)
    assert np.isclose(place[1], np.exp(-CFG.hunger_weight * 0.2))
    assert np.isclose(place[2], np.exp(-CFG.danger_weight * 0.1))
    assert np.isclose(place[3], (10.0 / CFG.small_village) ** CFG.size_exponent)
    assert np.isclose(place[4], 0.5 ** CFG.burden_exponent)
    better_living = migration.appeal(np.array([1.5 * CFG.usual_prospects]), calm(), people[:1], usual_land(people[:1]),
                                     take[:1], CONFIG)
    assert np.isclose(better_living[0], 1.5)


def test_land_to_spare_draws_people_and_crowding_sends_them_away():
    people = np.full(3, 1000.0)
    land = usual_land(people) * np.array([1.0, 2.0, 0.5])  # usual, land to spare after a plague, crowded
    place = migration.appeal(np.full(3, CFG.usual_prospects), calm(3), people, land, np.full(3, CFG.usual_take), CONFIG)
    exponent = 1.0 - CONFIG.food.labor_share
    assert np.isclose(place[0], 1.0)
    assert np.isclose(place[1], 2.0 ** exponent) and np.isclose(place[2], 0.5 ** exponent)
    emptied = migration.appeal(np.array([CFG.usual_prospects]), calm(), people[:1], 100 * usual_land(people[:1]),
                               np.array([CFG.usual_take]), CONFIG)
    assert np.isclose(emptied[0], CFG.land_range[1])  # only so much better


def test_plague_and_plunder_are_remembered_but_ordinary_deaths_are_not():
    memory = calm(3)
    people = np.array([1000.0, 1000.0, 50.0])
    usual_deaths = people * CFG.normal_mortality / 12
    deaths = np.array([usual_deaths[0], 30.0, 1.0])  # an ordinary month, a plague month, one death in a hamlet
    migration.remember(memory, np.array([1.0, 1.0, 1.0]), deaths, people, np.zeros(3), CONFIG)
    assert memory.danger[0] == 0 and memory.danger[2] == 0 and memory.danger[1] > 0
    before = memory.danger.copy()
    migration.remember(memory, np.array([0.7, 1.0, 1.0]), usual_deaths, people, np.array([0.0, 0.0, 0.2]), CONFIG)
    assert memory.hunger[0] > 0 and memory.danger[2] > 0
    assert memory.danger[1] < before[1]  # it fades


def test_young_singles_leave_more_from_a_worse_place():
    config = replace(CONFIG, migration=replace(CFG, family_leave_chance=0.0, stay_in_region=0.0))
    left = []
    for place in (1.0, 0.3):
        total = 0
        for seed in range(30):
            pop = people([20] * 200, list(range(200)))
            hh = Households(np.zeros(200), money=np.full(200, 4.0))
            moves = migration.move(pop, hh, np.array([place]), calm(), np.ones(200), config, np.random.default_rng(seed))
            total += moves.left[0]
        left.append(total)
    assert left[1] > 2 * left[0] > 0


def test_a_leaver_takes_a_share_of_the_family_coins_and_grain():
    config = replace(CONFIG, migration=replace(CFG, leave_chance=1.0, family_leave_chance=0.0))
    pop = people([40, 38, 20, 10], [0, 0, 0, 0], married=[True, True, False, False], female=[False, True, False, False])
    hh = Households(np.zeros(1), money=np.array([40.0]), grain=np.array([80.0]))
    moves = migration.move(pop, hh, np.array([1.0]), calm(), np.ones(1), config, np.random.default_rng(0))
    assert moves.left[0] == 1 and pop.size == 3 and pop.count[pop.age_years == 20].sum() == 0  # dropped at month end
    assert np.isclose(moves.coins, 10.0) and np.isclose(moves.grain, 20.0)
    assert np.isclose(hh.money[0], 30.0) and np.isclose(hh.grain[0], 60.0)


def test_starving_families_flee_together_and_leave_their_land():
    config = replace(CONFIG, migration=replace(CFG, leave_chance=0.0, family_leave_chance=0.0, flee_chance=1.0))
    pop = people([40, 38, 12, 50], [0, 0, 0, 1], married=[True, True, False, True])
    hh = Households(np.zeros(2), grain=np.array([5.0, 50.0]), land=np.array([3.0, 3.0]))
    moves = migration.move(pop, hh, np.array([1.0]), calm(), np.array([0.3, 0.9]), config, np.random.default_rng(0))
    assert moves.left[0] == 3 and moves.families == 1 and pop.size == 1
    assert hh.grain[0] == 0 and hh.land[0] == 3.0


def test_landless_families_uproot_more_readily_than_landholders():
    config = replace(CONFIG, migration=replace(CFG, leave_chance=0.0, family_leave_chance=0.5, stay_in_region=0.0))
    gone = {True: 0, False: 0}
    for seed in range(20):
        pop = people([40] * 100, list(range(100)), married=[True] * 100)
        land = np.where(np.arange(100) < 50, 0.0, 2.0)
        hh = Households(np.zeros(100), land=land)
        migration.move(pop, hh, np.array([1.0]), calm(), np.ones(100), config, np.random.default_rng(seed))
        gone[True] += int((pop.count[:50] == 0).sum())
        gone[False] += int((pop.count[50:] == 0).sum())
    assert gone[True] > 2 * gone[False]


def test_people_follow_those_who_went_before():
    config = replace(CONFIG, migration=replace(CFG, family_leave_chance=0.0, stay_in_region=0.0))
    left = []
    for leaving in (0.0, 0.2):
        memory = calm()
        memory.leaving[:] = leaving
        total = 0
        for seed in range(30):
            pop = people([20] * 200, list(range(200)))
            hh = Households(np.zeros(200))
            total += migration.move(pop, hh, np.array([1.0]), memory, np.ones(200), config, np.random.default_rng(seed)).left[0]
        left.append(total)
    assert left[1] > 1.5 * left[0]


def test_those_who_stay_in_the_region_move_to_a_better_village_with_their_goods():
    config = replace(CONFIG, migration=replace(CFG, leave_chance=1.0, family_leave_chance=0.0, stay_in_region=1.0))
    # Village 0 is a poor place; village 1 a good one with a landholding family.
    pop = people([20, 45, 45], [0, 1, 1], location=[0, 1, 1], married=[False, True, True])
    hh = Households(np.array([0, 1]), money=np.array([6.0, 10.0]), grain=np.array([4.0, 50.0]), land=np.array([0.0, 5.0]))
    moves = migration.move(pop, hh, np.array([0.3, 1.2]), calm(2), np.ones(2), config, np.random.default_rng(0))
    assert moves.left[0] == 1 and moves.arrived[1] == 1 and moves.within == 1 and moves.coins == 0
    assert pop.location[0] == 1 and pop.household[0] == 1 and pop.size == 3
    assert np.isclose(hh.money[1], 16.0) and np.isclose(hh.grain[1], 54.0)  # coins and grain came along


def test_a_family_moving_to_another_village_sets_up_a_new_home_there():
    config = replace(CONFIG, migration=replace(CFG, leave_chance=0.0, family_leave_chance=1.0, rooted=1.0,
                                               stay_in_region=1.0))
    pop = people([40, 38, 10, 50], [0, 0, 0, 1], location=[0, 0, 0, 1], married=[True, True, False, True])
    pop.couple[:2] = 7
    hh = Households(np.array([0, 1]), money=np.array([9.0, 1.0]), grain=np.array([12.0, 1.0]), land=np.array([1.0, 5.0]))
    rng = np.random.default_rng(0)
    place = np.array([0.2, 1.5])
    moves = migration.move(pop, hh, place, calm(2), np.ones(2), config, rng)
    moved = pop.location[:3] == 1
    assert moved.all() and len(hh) >= 3
    home = pop.household[0]
    assert (pop.household[:3] == home).all() and hh.location[home] == 1
    assert np.isclose(hh.money[home], 9.0) and np.isclose(hh.grain[home], 12.0) and hh.land[home] == 0
    assert hh.land[0] == 1.0 and moves.families >= 1  # the land stays behind


def test_newcomers_come_to_good_places_and_settlers_to_empty_holdings_but_not_to_dangerous_ones():
    config = replace(CONFIG, migration=replace(CFG, arrive_rate=1000.0, settle_rate=1.0))
    pop = people([40] * 4, [0, 1, 2, 3], location=[0, 1, 2, 3], married=[True] * 4)
    hh = Households(np.arange(4), land=np.full(4, 5.0))
    room = np.array([1.0, 1.0, 500.0, 500.0])
    arrived = migration.arrive(pop, hh, np.array([1.0, 2.0, 1.0, 0.3]), room, config, np.random.default_rng(0))
    assert arrived[0] == 0  # an ordinary place: nobody comes
    assert arrived[1] > 0  # a good place
    assert arrived[2] > 0  # empty holdings in an ordinary place draw settlers
    assert arrived[3] == 0  # but not in a dangerous, hungry one


def test_a_good_place_draws_only_so_many_newcomers():
    pop = people([40] * 100, list(range(100)), married=[True] * 100)
    hh = Households(np.zeros(100), land=np.ones(100))
    room = np.zeros(1)
    at_cap = CFG.welcome + CFG.max_pull
    capped = migration.arrive(pop.select(np.arange(100)), hh, np.array([at_cap]), room, CONFIG, np.random.default_rng(3))
    beyond = migration.arrive(pop.select(np.arange(100)), hh, np.array([at_cap + 5.0]), room, CONFIG, np.random.default_rng(3))
    assert capped[0] == beyond[0]


def test_a_small_village_with_a_bad_economy_can_empty_out_but_a_big_one_does_not():
    harsh = LordConfig(demesne=0.4, labour_service=0.25)

    def died_out(population, seeds):
        count = 0
        for seed in range(seeds):
            config = replace(Config(seed=seed, months=360, lord=harsh),
                             villages=(VillageConfig(population=population, land=0.22 * population),))
            count += Simulation(config).run()[-1].population == 0
        return count

    assert died_out(80, 8) >= 1
    assert died_out(1000, 2) == 0


def test_moving_between_villages_keeps_the_books():
    villages = tuple(VillageConfig(name=f"v{i}", population=p, land=0.35 * p) for i, p in enumerate((60, 150, 400)))
    config = replace(Config(seed=4, months=120, scheduled_events=(ScheduledEvent("plague", 6, village=0),)),
                     villages=villages)
    sim = Simulation(config)
    money = sim.world.money
    people = sim.world.population.size
    for _ in range(120):
        r = sim.step()
        people += r.births - r.deaths - r.emigrants + r.immigrants
        assert people == r.population
        assert broken_invariants(sim.world) == []
        assert np.isclose(sim.world.money, money)
    assert sum(r.moved_within for r in sim.records) > 0


def test_no_migration_when_switched_off():
    records = Simulation(Config(seed=1, months=24, migration=MigrationConfig(enabled=False))).run()
    assert sum(r.emigrants + r.immigrants for r in records) == 0


def test_a_known_epidemic_is_danger_even_where_few_die():
    memory = calm(1)
    people = np.array([50.0])
    migration.remember(memory, np.ones(1), np.array([1.0]), people, np.zeros(1), CONFIG, mortality_mult=np.array([8.0]))
    assert memory.danger[0] > 0
