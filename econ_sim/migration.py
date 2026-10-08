"""Migration: people leave for the town, and come when hands are short.

Young single people (`young_ages`) leave each month with `leave_chance`,
more when a wage can barely feed a family (`push`); in a famine whole
families whose food fell below `flee_below` of their need leave together.
Leavers take their share of their family's coins and grain; a family that
leaves for good leaves its land behind (it goes to a landless family) and
its debts unpaid. When a wage buys plenty (labour is short), young people
arrive from the region and join a landholding family as servants.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np

from econ_sim import rules
from econ_sim.config import Config
from econ_sim.households import Households, sizes
from econ_sim.population import NO_JOB, Population


@dataclass
class Moves:
    left: np.ndarray  # people who left, per village
    arrived: np.ndarray  # people who arrived, per village
    coins: float  # coins that left with them (net)
    grain: float  # rations that left with them


def leave(
    population: Population, households: Households, cover: np.ndarray, family_share: np.ndarray,
    config: Config, rng: np.random.Generator,
) -> Moves:
    """Young singles and starving families leave for the town."""
    cfg = config.migration
    n = len(cover)
    n_hh = len(households)
    zero = Moves(left=np.zeros(n), arrived=np.zeros(n), coins=0.0, grain=0.0)
    if not cfg.enabled or len(population) == 0:
        return zero
    age = population.age_years
    young = (population.count == 1) & ~population.married & (age >= cfg.young_ages[0]) & (age <= cfg.young_ages[1])
    push = 1.0 + cfg.push * np.maximum(cfg.content_cover - cover, 0.0)
    goes = young & (rng.random(len(population)) < rules.monthly_chance(cfg.leave_chance) * push[population.location])
    # Whole families flee a famine.
    starving = family_share < cfg.flee_below
    fleeing = starving & (rng.random(n_hh) < cfg.flee_chance)
    goes |= fleeing[population.household]
    if not goes.any():
        return zero
    size = sizes(population, n_hh).astype(np.float64)
    leaving = np.bincount(population.household[goes], weights=population.count[goes], minlength=n_hh)
    share = np.divide(leaving, size, out=np.zeros(n_hh), where=size > 0)
    coins = households.money * share
    grain = households.grain * share
    households.money -= coins
    households.grain -= grain
    left = np.bincount(population.location[goes], weights=population.count[goes], minlength=n).astype(np.float64)
    population.count[goes] = 0
    population.remove_empty()
    return Moves(left=left, arrived=np.zeros(n), coins=float(coins.sum()), grain=float(grain.sum()))


def arrive(
    population: Population, households: Households, cover: np.ndarray, config: Config, rng: np.random.Generator
) -> np.ndarray:
    """Young people come from the region when labour is short (a wage buys
    more than `welcome_cover` of a family's food) and join landholding
    families as servants. Returns arrivals per village."""
    cfg = config.migration
    n = len(cover)
    arrived = np.zeros(n)
    if not cfg.enabled:
        return arrived
    people = rules.by_location(population.count.astype(np.float64), population, n)
    expected = cfg.arrive_rate * people / 1000.0 * np.maximum(cover - cfg.welcome_cover, 0.0)
    count = rng.poisson(expected)
    rows = []
    hosting = (households.land > 0) & (sizes(population, len(households)) > 0)
    for village in np.flatnonzero(count):
        hosts = np.flatnonzero((households.location == village) & hosting)
        if not len(hosts):
            continue
        home = rng.choice(hosts, size=count[village])
        k = count[village]
        rows.append(Population(
            count=np.ones(k, dtype=np.int64),
            age_months=rng.integers(cfg.young_ages[0] * 12, cfg.young_ages[1] * 12, size=k),
            female=rng.random(k) < 0.5,
            health=np.full(k, 90.0),
            skill=rules.draw_skill(np.ones(k, dtype=np.int64), config.skill, rng),
            location=np.full(k, village),
            household=home,
            job=np.full(k, NO_JOB),
        ))
        arrived[village] = k
    if rows:
        newcomers = rows[0]
        for new in rows[1:]:
            newcomers.append(new)
        population.append(newcomers)
    return arrived
