"""The state of the simulated world, and how it is set up at the start."""

from __future__ import annotations

from dataclasses import dataclass, field

import numpy as np

from econ_sim import rules
from econ_sim.config import Config, EventSpec
from econ_sim.households import Households, form_households
from econ_sim.population import Population
from econ_sim.rng import RandomStreams


@dataclass
class ActiveEvent:
    spec: EventSpec
    location: int
    months_left: int  # including the current month
    effects: dict[str, float]  # ranges already drawn


@dataclass
class World:
    month: int  # months since the start; 0 is January of year 1
    population: Population
    granary: np.ndarray  # stored food per village, in rations
    land: np.ndarray  # farmland per village, in plots
    households: Households
    names: tuple[str, ...]  # village names
    active_events: list[ActiveEvent] = field(default_factory=list)

    @property
    def year(self) -> int:
        return self.month // 12 + 1

    @property
    def month_of_year(self) -> int:
        """Calendar month, 1-12."""
        return self.month % 12 + 1

    @property
    def n_locations(self) -> int:
        return len(self.names)


def create_world(config: Config, streams: RandomStreams) -> World:
    rng = streams["setup"]
    population = Population.empty()
    for location, village in enumerate(config.villages):
        population.append(_initial_people(village.population, location, config, rng))

    n = len(config.villages)
    households = form_households(population, n, rng)
    rules.update_jobs(population, config)
    need = rules.by_location(rules.food_need(population, config), population, n)
    months_of_food = np.array([v.initial_food_months for v in config.villages])
    return World(
        month=config.start_month - 1,
        population=population,
        granary=need * months_of_food,
        land=np.array([v.land for v in config.villages], dtype=np.float64),
        households=households,
        names=tuple(v.name for v in config.villages),
    )


def _initial_people(n: int, location: int, config: Config, rng: np.random.Generator) -> Population:
    demo = config.demography
    ages = np.arange(demo.max_initial_age + 1)
    weights = np.exp(-demo.initial_age_decay * ages)
    years = rng.choice(ages, size=n, p=weights / weights.sum())
    # Alternate sexes down the age order, so every age group is balanced.
    by_age = np.argsort(years + rng.random(n))
    female = np.zeros(n, dtype=bool)
    female[by_age[rng.integers(2) :: 2]] = True
    count = np.ones(n, dtype=np.int64)
    return Population(
        count=count,
        age_months=years * 12 + rng.integers(0, 12, size=n),
        female=female,
        health=rng.uniform(*config.health.initial, size=n),
        skill=rules.draw_skill(count, config.skill, rng),
        location=np.full(n, location),
    )
