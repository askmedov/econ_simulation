from dataclasses import replace

import numpy as np

from econ_sim import healthcare, rules
from econ_sim.config import Config, HealthcareConfig, ScheduledEvent, VillageConfig
from econ_sim.population import Population
from econ_sim.scenarios import compare, effect, series
from econ_sim.simulation import Simulation

CONFIG = Config()


def people(health, age_months=None, count=None):
    n = len(health)
    return Population(
        count=np.ones(n) if count is None else count,
        age_months=np.full(n, 30 * 12) if age_months is None else age_months,
        female=np.zeros(n, bool), health=np.array(health, dtype=float), skill=np.ones(n), location=np.zeros(n),
    )


def treat(pop, healers, config=CONFIG):
    risk = rules.death_chance(pop, np.ones(1), config)
    return healthcare.treat(pop, np.array([healers]), risk, config)


def test_those_most_likely_to_die_are_seen_first_up_to_capacity():
    pop = people([90.0, 5.0, 60.0, 25.0])
    per_healer = CONFIG.healthcare.patients_per_healer
    care, treated = treat(pop, 2.0 / per_healer)  # room for 2 patients
    assert treated[0] == 2
    cut = CONFIG.healthcare.treatment_mortality
    assert care.tolist() == [1.0, cut, 1.0, cut]  # the starving, not the healthy


def test_treatment_restores_some_health():
    pop = people([10.0])
    treat(pop, 1.0)
    assert pop.health[0] == 10.0 + CONFIG.healthcare.treatment_recovery


def test_infants_and_the_old_are_seen_but_healthy_adults_are_not():
    pop = people([95.0, 95.0, 95.0], age_months=np.array([3, 30 * 12, 75 * 12]))
    care, treated = treat(pop, 10.0)
    assert care[0] < 1.0 and care[1] == 1.0 and care[2] < 1.0 and treated[0] == 2


def test_an_outbreak_brings_more_patients():
    pop = people([95.0] * 3, age_months=np.array([3 * 12, 30 * 12, 65 * 12]))
    calm = rules.death_chance(pop, np.ones(1), CONFIG)
    outbreak = rules.death_chance(pop, np.array([2.5]), CONFIG)
    _, seen_calm = healthcare.treat(pop, np.array([10.0]), calm, CONFIG)
    _, seen_outbreak = healthcare.treat(pop, np.array([10.0]), outbreak, CONFIG)
    assert seen_outbreak[0] > seen_calm[0]


def test_groups_are_split_when_only_some_can_be_seen():
    pop = people([10.0], count=np.array([100]))
    per_healer = CONFIG.healthcare.patients_per_healer
    _, treated = treat(pop, 40.0 / per_healer)
    assert treated[0] == 40 and pop.size == 100 and len(pop) == 2


def test_no_treatment_without_healers_or_when_disabled():
    pop = people([10.0])
    care, treated = treat(pop, 0.0)
    assert care.tolist() == [1.0] and treated[0] == 0
    care, _ = treat(pop, 5.0, replace(CONFIG, healthcare=HealthcareConfig(enabled=False)))
    assert care.tolist() == [1.0]


def test_care_lowers_the_risk_of_dying():
    pop = people([20.0, 20.0])
    risk = rules.death_chance(pop, np.ones(1), CONFIG, care=np.array([1.0, 0.6]))
    assert np.isclose(risk[1], 0.6 * risk[0])


def test_council_hires_and_pays_healers():
    sim = Simulation(replace(CONFIG, months=3))
    sim.run()
    expected = round(CONFIG.healthcare.healers_per_1000 * sim.world.population.size / 1000)
    assert abs(sim.records[-1].healers - expected) <= 1
    assert sim.records[-1].treated > 0


def test_no_healers_without_a_council():
    sim = Simulation(replace(CONFIG, months=6, villages=(VillageConfig(population=100, land=35),)))
    sim.run()
    assert sim.records[-1].healers == 0 and sim.records[-1].treated == 0


def test_healthcare_blunts_an_epidemic():
    outbreak = (ScheduledEvent("disease", 3),)
    config = replace(CONFIG, months=12, random_events=False)
    with_care = compare(config, outbreak, runs=8)
    without = compare(replace(config, healthcare=HealthcareConfig(enabled=False)), outbreak, runs=8)
    deaths = lambda result: series(result.scenario, "deaths").sum()
    assert deaths(with_care) < deaths(without)
