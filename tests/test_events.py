from dataclasses import replace

import numpy as np
import pytest

from econ_sim import events
from econ_sim.config import Config, EventSpec, VillageConfig
from econ_sim.rng import RandomStreams
from econ_sim.world import create_world

CONFIG = Config()


def world(villages=1):
    config = replace(CONFIG, villages=tuple(VillageConfig() for _ in range(villages)))
    return create_world(config, RandomStreams(0))


def sure(name, **kwargs):
    defaults = dict(scope="location", chance=1.0, effects={"production_mult": 0.5})
    return EventSpec(name=name, **{**defaults, **kwargs})


def test_default_events_are_valid():
    events.validate(CONFIG.events)


@pytest.mark.parametrize(
    "spec",
    [
        sure("bad_scope", scope="planet"),
        sure("bad_effect", effects={"luck": 2.0}),
        sure("person_production", scope="person", effects={"production_mult": 0.5}),
        sure("bad_chance", chance=1.5),
        sure("long_person_event", scope="person", effects={"health_delta": -1.0}, duration=3),
    ],
)
def test_invalid_events_are_rejected(spec):
    with pytest.raises(ValueError):
        events.validate((spec,))


def test_group_chances_must_not_exceed_one():
    with pytest.raises(ValueError):
        events.validate((sure("a", group="g", chance=0.6), sure("b", group="g", chance=0.6)))


def test_certain_event_starts_and_lasts_its_duration():
    w = world()
    spec = sure("flood", duration=3)
    started = events.start_location_events(w, (spec,), np.random.default_rng(0))
    assert [e.spec.name for e in started] == ["flood"]
    for _ in range(2):
        events.advance(w)
        assert len(w.active_events) == 1
    events.advance(w)
    assert w.active_events == []


def test_active_event_is_not_started_twice():
    w = world()
    rng = np.random.default_rng(0)
    events.start_location_events(w, (sure("flood", duration=3),), rng)
    assert events.start_location_events(w, (sure("flood", duration=3),), rng) == []


def test_events_in_a_group_exclude_each_other():
    w = world()
    specs = (sure("drought", group="weather", chance=0.5), sure("rain", group="weather", chance=0.5))
    started = events.start_location_events(w, specs, np.random.default_rng(0))
    assert len(started) == 1


def test_events_only_start_in_their_months():
    w = world()  # January
    spec = sure("spring_flood", months=(4,))
    assert events.start_location_events(w, (spec,), np.random.default_rng(0)) == []
    w.month = 3  # April
    assert len(events.start_location_events(w, (spec,), np.random.default_rng(0))) == 1


def test_each_village_rolls_its_own_events():
    w = world(villages=3)
    started = events.start_location_events(w, (sure("flood"),), np.random.default_rng(0))
    assert sorted(e.location for e in started) == [0, 1, 2]


def test_modifiers_combine_active_events():
    w = world()
    specs = (
        sure("a", effects={"production_mult": 0.5, "health_delta": -2.0, "granary_loss": 0.5}),
        sure("b", effects={"production_mult": 0.8, "health_delta": -3.0, "granary_loss": 0.5}),
    )
    events.start_location_events(w, specs, np.random.default_rng(0))
    mods = events.modifiers(w)
    assert np.isclose(mods.production_mult[0], 0.4)
    assert np.isclose(mods.health_delta[0], -5.0)
    assert np.isclose(mods.granary_loss[0], 0.75)
    assert mods.mortality_mult[0] == 1.0


def test_effect_ranges_are_drawn_when_event_starts():
    w = world()
    events.start_location_events(w, (sure("fire", effects={"granary_loss": (0.1, 0.3)}),), np.random.default_rng(0))
    assert 0.1 <= w.active_events[0].effects["granary_loss"] <= 0.3


def test_person_events_hit_individuals_and_split_groups():
    w = world()
    pop = w.population
    pop.count[:] = 1000  # make every row a group
    pop.health[:] = 100.0
    spec = EventSpec(name="accident", scope="person", chance=0.1, effects={"health_delta": -30.0})
    rows_before, people_before = len(pop), pop.size
    hits = events.apply_person_events(w, (spec,), CONFIG.health, np.random.default_rng(0))
    assert hits["accident"] > 0
    assert pop.size == people_before  # nobody lost, just regrouped
    assert len(pop) > rows_before
    assert pop.count[pop.health == 70.0].sum() == hits["accident"]
