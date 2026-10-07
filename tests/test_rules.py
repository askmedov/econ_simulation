from dataclasses import replace

import numpy as np

from econ_sim import rules
from econ_sim.config import Config, FoodConfig, HealthConfig
from econ_sim.population import Population

CONFIG = Config()


def people(ages_years, count=1, female=False, health=100.0, location=0):
    n = len(ages_years)
    return Population(
        count=np.full(n, count),
        age_months=np.asarray(ages_years) * 12,
        female=np.full(n, female),
        health=np.full(n, health, dtype=float),
        skill=np.ones(n),
        location=np.full(n, location),
    )


def test_monthly_chance_compounds_to_annual():
    monthly = rules.monthly_chance(0.2)
    assert np.isclose(1 - (1 - monthly) ** 12, 0.2)


def test_children_need_less_food():
    need = rules.food_need(people([5, 30]), CONFIG)
    assert need.tolist() == [CONFIG.food.child_need, CONFIG.food.adult_need]


def test_food_need_scales_with_group_size():
    assert rules.food_need(people([30], count=50), CONFIG).tolist() == [50.0]


def test_seasons_average_to_one():
    assert np.isclose(rules.season_factors(CONFIG).mean(), 1.0)


NO_SPOILAGE = replace(CONFIG, food=replace(CONFIG.food, spoilage=0.0))


def outlook(per_month: float, months: int = 11) -> np.ndarray:
    return np.full((1, months), per_month)


def test_plan_ration_is_full_when_food_will_last():
    ration = rules.plan_ration(np.array([10_000.0]), np.array([100.0]), outlook(100.0), CONFIG)
    assert ration.tolist() == [1.0]


def test_plan_ration_stretches_a_short_stock():
    # No harvest expected: 600 rations must last 12 months of 100-ration need.
    ration = rules.plan_ration(np.array([600.0]), np.array([100.0]), outlook(0.0), NO_SPOILAGE)
    assert np.isclose(ration[0], 0.5)


def test_plan_ration_allows_for_spoilage():
    # Exactly enough food without spoilage is not enough once stores rot.
    stock, need = np.array([1200.0]), np.array([100.0])
    assert np.isclose(rules.plan_ration(stock, need, outlook(0.0), NO_SPOILAGE)[0], 1.0)
    assert rules.plan_ration(stock, need, outlook(0.0), CONFIG)[0] < 1.0


def test_plan_ration_counts_on_the_coming_harvest():
    # Little in store, but a big harvest next month.
    harvest = np.zeros((1, 11))
    harvest[0, 0] = 2000.0
    ration = rules.plan_ration(np.array([100.0]), np.array([100.0]), harvest, NO_SPOILAGE)
    assert np.isclose(ration[0], 1.0)


def test_plan_ration_can_be_switched_off():
    config = replace(CONFIG, food=FoodConfig(planning_months=0))
    ration = rules.plan_ration(np.array([0.0]), np.array([100.0]), outlook(0.0), config)
    assert ration.tolist() == [1.0]


def test_harvest_outlook_follows_seasons_and_events():
    events = np.ones((1, 11))
    events[0, :3] = 0.5  # a drought lasting three more months
    ahead = rules.harvest_outlook(np.array([100.0]), events, 3, CONFIG)  # now March
    season = rules.season_factors(CONFIG)
    assert np.isclose(ahead[0, 0], 100.0 * season[3] * 0.5)  # April, drought
    assert np.isclose(ahead[0, 3], 100.0 * season[6])  # July, drought over


def test_spoilage_takes_a_share_of_the_stock():
    granary = np.array([100.0])
    spoiled = rules.spoil(granary, 0.02)
    assert np.isclose(spoiled[0], 2.0) and np.isclose(granary[0], 98.0)


def test_health_recovers_when_fed():
    pop = people([30], health=50.0)
    rules.update_health(pop, np.ones(1), np.zeros(1), CONFIG)
    assert pop.health[0] == 50.0 + CONFIG.health.recovery


def test_health_falls_toward_level_set_by_ration():
    pop = people([30], health=100.0)
    for _ in range(50):
        rules.update_health(pop, np.array([0.8]), np.zeros(1), CONFIG)
    # Health settles in proportion to how far 80% is from starvation rations.
    starving = CONFIG.health.starvation_ration
    assert np.isclose(pop.health[0], 100 * (0.8 - starving) / (1 - starving))


def test_health_stays_within_bounds():
    pop = people([30, 30], health=np.array([99.0, 1.0]))
    rules.update_health(pop, np.ones(1), np.array([0.0]), CONFIG)
    assert pop.health[0] == 100.0
    rules.update_health(pop, np.zeros(1), np.zeros(1), CONFIG)
    assert pop.health.min() >= 0.0


def test_events_change_health():
    pop = people([30], health=80.0)
    rules.update_health(pop, np.ones(1), np.array([-8.0]), CONFIG)
    assert pop.health[0] == 80.0 + CONFIG.health.recovery - 8.0


def test_death_risk_is_higher_for_infants_old_and_weak():
    pop = people([0, 30, 85, 30])
    pop.health[3] = 10.0
    chance = rules.death_chance(pop, np.ones(1), CONFIG)
    assert chance[1] < chance[0] and chance[1] < chance[2] and chance[1] < chance[3]


def test_group_deaths_match_individual_deaths_on_average():
    # A row of 10,000 identical people should lose as many as 10,000 rows of 1.
    rng = np.random.default_rng(0)
    mult, config = np.array([20.0]), CONFIG
    group = sum(rules.deaths(people([85], count=10_000), mult, config, rng, 1)[0] for _ in range(20))
    singles = sum(rules.deaths(people([85] * 10_000), mult, config, rng, 1)[0] for _ in range(20))
    expected = 20 * 10_000 * rules.death_chance(people([85]), mult, config)[0]
    assert abs(group - expected) / expected < 0.03
    assert abs(singles - expected) / expected < 0.03


def test_deaths_remove_people():
    pop = people([30] * 5, health=0.0)
    config = replace(CONFIG, health=HealthConfig(max_extra_mortality=1.0))
    died = rules.deaths(pop, np.ones(1), config, np.random.default_rng(0), 1)
    assert died[0] == 5 and len(pop) == 0


def test_only_healthy_fertile_women_give_birth():
    config = replace(CONFIG, demography=replace(CONFIG.demography, annual_birth_chance=1.0))
    rng = np.random.default_rng(0)
    candidates = [
        people([25], count=1000, female=True),  # fertile
        people([25], count=1000, female=False),  # men
        people([10], count=1000, female=True),  # too young
        people([50], count=1000, female=True),  # too old
        people([25], count=1000, female=True, health=30.0),  # too weak
    ]
    born = [rules.births(pop, np.ones(1), config, rng, 1)[0] for pop in candidates]
    assert born[0] > 0 and born[1:] == [0, 0, 0, 0]


def test_newborns_join_the_population():
    config = replace(CONFIG, demography=replace(CONFIG.demography, annual_birth_chance=1.0))
    pop = people([25] * 200, female=True, health=95.0, location=2)
    born = rules.births(pop, np.ones(3), config, np.random.default_rng(0), 3)
    babies = pop.age_months == 0
    assert born[2] == pop.count[babies].sum() > 0
    assert (pop.location[babies] == 2).all() and (pop.health[babies] == 95.0).all()


def test_people_grow_older():
    pop = people([30])
    rules.grow_older(pop)
    assert pop.age_months[0] == 30 * 12 + 1


def test_wage_cover_is_food_a_wage_buys_over_need_per_worker():
    # Pay 2 coins, food 1 coin: a wage buys 2 rations; need is 1.5 per worker.
    cover = rules.wage_cover(np.array([2.0]), np.array([1.0]), np.array([150.0]), np.array([100.0]))
    assert np.isclose(cover[0], 2.0 / 1.5)


def test_births_slow_when_a_wage_can_barely_feed_a_family():
    low, high = CONFIG.demography.wage_cover_for_births
    factor = rules.birth_factor(np.array([low - 0.1, (low + high) / 2, high + 0.1]), CONFIG)
    floor = CONFIG.demography.crowded_birth_factor
    assert np.allclose(factor, [floor, (1 + floor) / 2, 1.0])

def test_realistic_plan_allows_for_weaker_workers():
    # Half the year's food has to come from future harvests; on short rations
    # workers grow less, so the realistic plan rations harder.
    stock, need, ahead = np.array([600.0]), np.array([100.0]), outlook(40.0)
    naive = rules.plan_ration(stock, need, ahead, NO_SPOILAGE)[0]
    realistic = rules.plan_ration_realistically(stock, need, ahead, NO_SPOILAGE)[0]
    assert realistic < naive < 1.0


def test_realistic_plan_keeps_full_rations_when_food_is_plentiful():
    ration = rules.plan_ration_realistically(np.array([5000.0]), np.array([100.0]), outlook(100.0), CONFIG)
    assert ration.tolist() == [1.0]
