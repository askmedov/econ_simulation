"""Random events: starting, ending and combining their effects.

Event kinds are data (see `EventSpec` in config). This module only knows how
to roll them and turn the active ones into multipliers for the rules.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np

from econ_sim import geography
from econ_sim.config import EffectValue, EventSpec, HealthConfig, ScheduledEvent
from econ_sim.geography import Geography
from econ_sim.world import ActiveEvent, World

EFFECTS = (
    "production_mult", "health_delta", "mortality_mult", "fertility_mult", "granary_loss", "heating_mult", "debt_cancel",
    "requisition", "debase", "woods_burned",
)
PERSON_EFFECTS = ("health_delta",)
REACHES = ("village", "weather", "region")


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
    debt_cancel: np.ndarray
    requisition: np.ndarray
    debase: np.ndarray
    woods_burned: np.ndarray

    @classmethod
    def neutral(cls, n_locations: int, n_businesses: int = 1) -> Modifiers:
        ones, zeros = np.ones(n_locations), np.zeros(n_locations)
        return cls(
            np.ones((n_locations, n_businesses)), zeros.copy(), ones.copy(), ones.copy(), zeros.copy(), ones.copy(),
            zeros.copy(), zeros.copy(), zeros.copy(), zeros.copy(),
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
        if spec.repeat_chance is not None and not (0.0 <= spec.repeat_chance <= 1.0 and spec.chance < 1.0):
            raise ValueError(f"{spec.name}: repeat_chance must be between 0 and 1, with chance below 1")
        if spec.scope == "person" and spec.duration != 1:
            raise ValueError(f"{spec.name}: person events last one month")
        if spec.reach not in REACHES or spec.end not in ("bad", "good"):
            raise ValueError(f"{spec.name}: reach must be one of {', '.join(REACHES)}, and end 'bad' or 'good'")
        if spec.group:
            group_chance[spec.group] = group_chance.get(spec.group, 0.0) + spec.chance
    for group, total in group_chance.items():
        if total > 1.0:
            raise ValueError(f"event group {group!r}: chances add up to more than 1")
    # After a year with one of a group's events, its repeat chance replaces its usual one.
    for spec in specs:
        if spec.group and spec.repeat_chance is not None:
            others = group_chance[spec.group] - spec.chance
            if others + spec.repeat_chance > 1.0:
                raise ValueError(f"event group {spec.group!r}: chances add up to more than 1 after {spec.name}")


def chance_now(spec: EventSpec, repeated: bool) -> float:
    """An event's chance this month: its `repeat_chance` if it started in the
    last 12 months, otherwise a chance lowered so that it still starts in
    `chance` of eligible months in the long run."""
    if spec.repeat_chance is None:
        return spec.chance
    if repeated:
        return spec.repeat_chance
    # Long-run share p = q / (1 - r + q), for a chance q after a year without it.
    p, r = spec.chance, spec.repeat_chance
    return p * (1.0 - r) / (1.0 - p)


def validate_schedule(
    scheduled: tuple[ScheduledEvent, ...], specs: tuple[EventSpec, ...], n_locations: int, on_map: bool = False
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
        if item.near is not None and (not on_map or item.village is not None or len(item.near) != 3):
            raise ValueError(f"{item.event}: 'near' (x, y, km) needs a map, and no village number")


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


@dataclass
class Weather:
    """The map the weather falls on, how far it reaches, and the random
    stream it is drawn from."""

    geo: Geography
    scale_km: float
    rng: np.random.Generator


def start_location_events(
    world: World, specs: tuple[EventSpec, ...], rng: np.random.Generator, weather: Weather | None = None
) -> list[ActiveEvent]:
    """Roll village-wide events. Events in the same group exclude each other.
    With `weather` (a map), weather events follow the season's weather over
    the map and region events strike every village at once."""
    candidates: dict[str, list[EventSpec]] = {}
    for spec in specs:
        if _can_start(spec, "location", world.month_of_year):
            candidates.setdefault(spec.group or f"_{spec.name}", []).append(spec)
    if weather is not None:
        return _start_on_map(world, candidates, rng, weather)

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
                last = world.last_started.get((spec.name, location))
                threshold += chance_now(spec, last is not None and world.month - last <= 12)
                if roll < threshold:
                    started.append(_start(world, spec, location, rng))
                    break
    return started


def _start_on_map(
    world: World, candidates: dict[str, list[EventSpec]], rng: np.random.Generator, weather: Weather
) -> list[ActiveEvent]:
    """Start this month's events on a map. Each village rolls for its own
    events as without a map; weather events take the season's weather at
    each village (bad ones where it is worst, good ones where it is best);
    region events take one roll for all."""
    n = world.n_locations
    keys = list(candidates)
    rolls = rng.random((n, len(keys)))
    started = []
    for column, key in enumerate(keys):
        members = candidates[key]
        names = {spec.name for spec in members}
        free = np.ones(n, dtype=bool)
        for event in world.active_events:
            if event.spec.name in names or event.spec.group == key:
                free[event.location] = False
        for reach in REACHES:
            group = [spec for spec in members if spec.reach == reach]
            if not group:
                continue
            if reach == "region":
                roll = np.full(n, weather.rng.random())
            elif reach == "weather":
                roll = geography.weather(weather.geo, weather.scale_km, weather.rng)
            else:
                roll = rolls[:, column]
            sides = ("bad", "good") if reach == "weather" else (None,)
            for side in sides:
                at = 1.0 - roll if side == "good" else roll
                threshold = np.zeros(n)
                for spec in group:
                    if side is not None and spec.end != side:
                        continue
                    last = np.array([world.last_started.get((spec.name, loc), -(10**9)) for loc in range(n)])
                    repeated = world.month - last <= 12
                    low = threshold.copy()
                    threshold += np.array([chance_now(spec, r) for r in (False, True)])[repeated.astype(int)]
                    for loc in np.flatnonzero(free & (at >= low) & (at < threshold)):
                        started.append(_start(world, spec, int(loc), rng))
                        free[loc] = False
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
        if item.near is not None:
            locations = np.flatnonzero(world.geo.near(item.near)).tolist()
        else:
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
    world.last_started[(spec.name, location)] = world.month
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
            elif effect in ("granary_loss", "debt_cancel", "requisition", "debase", "woods_burned"):
                share = getattr(mods, effect)
                share[loc] = 1.0 - (1.0 - share[loc]) * (1.0 - value)
            else:
                getattr(mods, effect)[loc] *= value
    return mods
