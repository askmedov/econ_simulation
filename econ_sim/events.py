"""Random events: starting, ending and combining their effects.

Event kinds are data (see `EventSpec` in config). This module only knows how
to roll them and turn the active ones into multipliers for the rules.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np

from econ_sim.config import EffectValue, EventSpec, HealthConfig, ScheduledEvent
from econ_sim.world import ActiveEvent, World

EFFECTS = ("production_mult", "health_delta", "mortality_mult", "fertility_mult", "granary_loss", "heating_mult")
PERSON_EFFECTS = ("health_delta",)


@dataclass
class Modifiers:
    """Combined effect of all active events: one value per village, except
    production_mult, which is per village and business."""

    production_mult: np.ndarray
    health_delta: np.ndarray
    mortality_mult: np.ndarray
    fertility_mult: np.ndarray
    granary_loss: np.ndarray
    heating_mult: np.ndarray

    @classmethod
    def neutral(cls, n_locations: int, n_businesses: int = 1) -> Modifiers:
        ones, zeros = np.ones(n_locations), np.zeros(n_locations)
        return cls(
            np.ones((n_locations, n_businesses)), zeros.copy(), ones.copy(), ones.copy(), zeros.copy(), ones.copy()
        )


def validate(specs: tuple[EventSpec, ...]) -> None:
    group_chance: dict[str, float] = {}
    for spec in specs:
        if spec.scope not in ("location", "person"):
            raise ValueError(f"{spec.name}: unknown scope {spec.scope!r}")
        allowed = EFFECTS if spec.scope == "location" else PERSON_EFFECTS
        for effect in spec.effects:
            if effect not in allowed:
                raise ValueError(f"{spec.name}: effect {effect!r} not allowed for {spec.scope} events")
        if not 0.0 <= spec.chance <= 1.0:
            raise ValueError(f"{spec.name}: chance must be between 0 and 1")
        if spec.scope == "person" and spec.duration != 1:
            raise ValueError(f"{spec.name}: person events last one month")
        if spec.group:
            group_chance[spec.group] = group_chance.get(spec.group, 0.0) + spec.chance
    for group, total in group_chance.items():
        if total > 1.0:
            raise ValueError(f"event group {group!r}: chances add up to more than 1")


def validate_schedule(
    scheduled: tuple[ScheduledEvent, ...], specs: tuple[EventSpec, ...], n_locations: int
) -> None:
    location_events = {s.name for s in specs if s.scope == "location"}
    for item in scheduled:
        if item.event not in location_events:
            known = ", ".join(sorted(location_events))
            raise ValueError(f"unknown village event {item.event!r}; choose from: {known}")
        if item.month < 1:
            raise ValueError(f"{item.event}: month must be 1 or later")
        if item.village is not None and not 0 <= item.village < n_locations:
            raise ValueError(f"{item.event}: no village number {item.village}")


def _draw(value: EffectValue | int | tuple[int, int], rng: np.random.Generator, whole: bool = False):
    if isinstance(value, tuple):
        low, high = value
        return int(rng.integers(low, high + 1)) if whole else float(rng.uniform(low, high))
    return value


def _can_start(spec: EventSpec, scope: str, month_of_year: int) -> bool:
    return spec.scope == scope and (spec.months is None or month_of_year in spec.months)


def advance(world: World) -> None:
    """Count down active events and drop those that have run their course."""
    for event in world.active_events:
        event.months_left -= 1
    world.active_events = [e for e in world.active_events if e.months_left > 0]


def start_location_events(
    world: World, specs: tuple[EventSpec, ...], rng: np.random.Generator
) -> list[ActiveEvent]:
    """Roll village-wide events. Events in the same group exclude each other."""
    candidates: dict[str, list[EventSpec]] = {}
    for spec in specs:
        if _can_start(spec, "location", world.month_of_year):
            candidates.setdefault(spec.group or f"_{spec.name}", []).append(spec)

    started = []
    for location in range(world.n_locations):
        here = [e.spec for e in world.active_events if e.location == location]
        busy = {s.name for s in here} | {s.group for s in here if s.group}
        for key, members in candidates.items():
            roll = rng.random()
            if busy & ({key} | {s.name for s in members}):
                continue
            threshold = 0.0
            for spec in members:
                threshold += spec.chance
                if roll < threshold:
                    started.append(_start(world, spec, location, rng))
                    break
    return started


def start_scheduled_events(
    world: World,
    scheduled: tuple[ScheduledEvent, ...],
    specs: tuple[EventSpec, ...],
    month_number: int,
    rng: np.random.Generator,
) -> list[ActiveEvent]:
    """Start the events forced for this month (`month_number` counts from 1).

    They start whatever their usual chance or season, unless the same event
    is already under way in that village.
    """
    by_name = {spec.name: spec for spec in specs}
    started = []
    for item in scheduled:
        if item.month != month_number:
            continue
        spec = by_name[item.event]
        locations = range(world.n_locations) if item.village is None else [item.village]
        for location in locations:
            if any(e.spec.name == spec.name and e.location == location for e in world.active_events):
                continue
            started.append(_start(world, spec, location, rng))
    return started


def _start(world: World, spec: EventSpec, location: int, rng: np.random.Generator) -> ActiveEvent:
    event = ActiveEvent(
        spec=spec,
        location=location,
        months_left=_draw(spec.duration, rng, whole=True),
        effects={k: _draw(v, rng) for k, v in spec.effects.items()},
    )
    world.active_events.append(event)
    return event


def apply_person_events(
    world: World, specs: tuple[EventSpec, ...], health: HealthConfig, rng: np.random.Generator
) -> dict[str, int]:
    """Hit individual people with personal events; returns how many each event hit."""
    hits = {}
    population = world.population
    for spec in specs:
        if not _can_start(spec, "person", world.month_of_year):
            continue
        taken = rng.binomial(population.count, spec.chance)
        hits[spec.name] = int(taken.sum())
        if not hits[spec.name]:
            continue
        rows = population.split(taken)
        if "health_delta" in spec.effects:
            value = spec.effects["health_delta"]
            delta = rng.uniform(*value, size=len(rows)) if isinstance(value, tuple) else value
            population.health[rows] = np.clip(population.health[rows] + delta, 0.0, health.maximum)
    return hits


def _hits(event: ActiveEvent, business: str) -> bool:
    return event.spec.businesses is None or business in event.spec.businesses


def production_outlook(world: World, months: int, business: str) -> np.ndarray:
    """Multiplier on one business's output in each of the next `months` months from events under way."""
    outlook = np.ones((world.n_locations, months))
    for event in world.active_events:
        mult = event.effects.get("production_mult")
        if mult is not None and _hits(event, business):
            outlook[event.location, : event.months_left - 1] *= mult
    return outlook


def modifiers(world: World, businesses: tuple[str, ...] = ("farming",)) -> Modifiers:
    """Combine active events; `businesses` names the columns of production_mult."""
    mods = Modifiers.neutral(world.n_locations, len(businesses))
    for event in world.active_events:
        loc = event.location
        for effect, value in event.effects.items():
            if effect == "production_mult":
                for b, name in enumerate(businesses):
                    if _hits(event, name):
                        mods.production_mult[loc, b] *= value
            elif effect == "health_delta":
                mods.health_delta[loc] += value
            elif effect == "granary_loss":
                mods.granary_loss[loc] = 1.0 - (1.0 - mods.granary_loss[loc]) * (1.0 - value)
            else:
                getattr(mods, effect)[loc] *= value
    return mods
