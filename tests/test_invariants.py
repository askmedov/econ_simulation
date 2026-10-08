from dataclasses import replace

import numpy as np
import pytest

from econ_sim.checks import broken_invariants
from econ_sim.config import Config, CouncilConfig, LandConfig, ScheduledEvent, VillageConfig
from econ_sim.rng import RandomStreams
from econ_sim.simulation import Simulation
from econ_sim.world import create_world

DISASTERS = tuple(
    ScheduledEvent(event, month)
    for event, month in [("drought", 4), ("plague", 9), ("army", 14), ("drought", 16), ("debasement", 20),
                         ("debt_jubilee", 30), ("raid", 33)]
)
SETTINGS = {
    "default": Config(months=72),
    "five villages": replace(Config(months=72), villages=tuple(
        VillageConfig(name=f"v{i}", population=200 + 150 * i, land=80 + 50 * i) for i in range(5))),
    "no council": Config(months=72, council=CouncilConfig(enabled=False)),
    "one disaster after another": Config(months=48, scheduled_events=DISASTERS),
    "heirs keep the land": Config(months=72, land=LandConfig(partible=False)),
    "hamlet": replace(Config(months=72), villages=(VillageConfig(population=20, land=7),)),
}


@pytest.mark.parametrize("name", SETTINGS)
def test_invariants_hold_every_month(name):
    for seed in range(2):
        sim = Simulation(replace(SETTINGS[name], seed=seed))
        money = sim.world.money
        for _ in range(sim.config.months):
            sim.step()
            assert broken_invariants(sim.world) == [], f"seed {seed}, month {sim.world.month}"
            assert np.isclose(sim.world.money, money, rtol=1e-9)


def test_the_checks_catch_broken_state():
    def broken(change) -> list[str]:
        world = create_world(Config(seed=1), RandomStreams(1))
        assert broken_invariants(world) == []
        change(world)
        return broken_invariants(world)

    def lose_a_couple_member(w):
        w.population.married[w.population.couple >= 0] = False

    assert broken(lambda w: w.households.land.__setitem__(0, w.households.land[0] + 1)) == ["land not conserved"]
    assert broken(lambda w: w.households.debt.__setitem__(0, 5.0)) == ["debts and claims don't balance"]
    assert broken(lambda w: w.households.grain.__setitem__(0, -1.0)) == ["negative or not-a-number grain"]
    assert broken(lambda w: w.population.location.__setitem__(0, 1)) == ["people living outside their household's village"]
    assert broken(lose_a_couple_member) == ["unmarried people in a couple"]
    assert broken(lambda w: w.households.kin.__setitem__(0, len(w.households))) == ["kin links to no family or another village"]
