from dataclasses import asdict, replace

import numpy as np
import pytest

from econ_sim.config import Config, VillageConfig
from econ_sim.metrics import write_csv
from econ_sim.population import Population
from econ_sim.simulation import Simulation


def run(config: Config):
    sim = Simulation(config)
    sim.run()
    return sim


def test_same_seed_gives_same_history():
    a = run(Config(seed=1, months=240))
    b = run(Config(seed=1, months=240))
    assert [asdict(r) for r in a.records] == [asdict(r) for r in b.records]
    assert a.log == b.log


def test_different_seeds_give_different_histories():
    a = run(Config(seed=1, months=240))
    b = run(Config(seed=2, months=240))
    assert [r.population for r in a.records] != [r.population for r in b.records]


def test_food_is_accounted_for():
    sim = Simulation(Config(seed=3, months=240))
    stock = sim.world.granary.sum()
    for _ in range(240):
        r = sim.step()
        # Families eat what they buy plus relief from the council's reserve.
        stock += r.food_produced - (r.food_eaten - r.relief) - r.food_spoiled - r.food_lost - r.food_levied
        assert np.isclose(stock, r.food_stock)


def test_population_matches_births_and_deaths():
    sim = Simulation(Config(seed=4, months=240))
    people = sim.world.population.size
    for _ in range(240):
        r = sim.step()
        people += r.births - r.deaths
        assert people == r.population == r.children + r.workers + r.elderly


@pytest.mark.parametrize("seed", range(5))
def test_village_survives_a_century_with_default_settings(seed):
    # Long runs are a sanity check: no runaway growth, no collapse.
    sim = run(Config(seed=seed, months=1200))
    start = Config().villages[0].population
    assert 0.3 * start < sim.records[-1].population < 4 * start


def test_droughts_happen_and_villages_go_hungry():
    sim = run(Config(seed=0, months=1200))
    droughts = [r for r in sim.records if "drought" in r.events]
    assert droughts
    assert any(r.ration < 1 for r in sim.records)


def test_villages_are_independent_without_trade():
    config = Config(seed=5, months=120, villages=(VillageConfig(name="A"), VillageConfig(name="B", land=10.0)))
    sim = run(config)
    assert sorted(set(sim.world.population.location.tolist())) == [0, 1]


def test_grouped_population_runs_and_keeps_totals_consistent():
    # The same rules must work when each row stands for many people.
    sim = Simulation(Config(seed=6, months=120, villages=(VillageConfig(population=10_000, land=5_000.0),)))
    pop = sim.world.population
    grouped = Population(
        count=np.full(100, 100),
        age_months=pop.age_months[:100],
        female=pop.female[:100],
        health=pop.health[:100],
        skill=pop.skill[:100],
        location=pop.location[:100],
    )
    sim.world.population = grouped
    people = grouped.size
    for _ in range(120):
        r = sim.step()
        people += r.births - r.deaths
        assert people == r.population == sim.world.population.size
    assert sim.world.population.count.max() > 1


def test_an_empty_village_keeps_running_quietly():
    config = Config(seed=0, months=120, villages=(VillageConfig(population=10, land=0.0, initial_food_months=0),))
    sim = run(config)
    assert sim.extinct and len(sim.records) == 120
    assert sim.records[-1].population == 0
    assert sum("last villager" in line for line in sim.log) == 1


def test_csv_has_one_row_per_month(tmp_path):
    sim = run(Config(seed=0, months=24))
    path = tmp_path / "months.csv"
    write_csv(sim.records, path)
    lines = path.read_text().splitlines()
    assert len(lines) == 25 and lines[0].startswith("month_number,year,month,population")


def test_starting_village_is_balanced_between_women_and_men():
    sim = Simulation(Config(seed=0))
    pop = sim.world.population
    assert pop.count[pop.female].sum() == pop.size // 2
    adults = pop.age_years >= 15
    women = pop.count[adults & pop.female].sum()
    assert abs(women - pop.count[adults].sum() / 2) <= 1


def test_village_recovers_after_a_drought_ends():
    # Regression: villagers used to judge harvests by recent bad years and
    # kept themselves hungry long after the drought was over.
    config = Config(seed=64, months=60, villages=(VillageConfig(population=100, land=35),))
    sim = run(config)
    last_year = sim.records[-12:]
    assert min(r.ration for r in last_year) > 0.8
