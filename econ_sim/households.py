"""Families: who lives, earns and eats together.

Each person row has a `household` index into the Households table. Families
are formed once at the start; newborns join their mother's family.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np

from econ_sim.population import Population

ADULT_AGE = 18
HEAD_AGES = (18, 49)  # women of these ages head a household at the start
PARTNER_MAX_AGE = 64
MOTHER_AGE_GAP = (18, 45)  # a child joins a woman this much older


@dataclass
class Households:
    location: np.ndarray  # village of each household
    money: np.ndarray | None = None  # savings, in coins

    def __post_init__(self) -> None:
        self.location = np.asarray(self.location, dtype=np.int32)
        if self.money is None:
            self.money = np.zeros(len(self.location))
        self.money = np.asarray(self.money, dtype=np.float64)

    def __len__(self) -> int:
        return len(self.location)


def form_households(population: Population, n_locations: int, rng: np.random.Generator) -> Households:
    """Group the starting villagers into families and set `population.household`."""
    household = np.full(len(population), -1, dtype=np.int64)
    locations: list[int] = []
    for location in range(n_locations):
        rows = np.flatnonzero(population.location == location)
        if len(rows) == 0:
            continue
        first = len(locations)
        count = _form_village(population, rows, first, household, rng)
        locations.extend([location] * count)
    population.household = household
    return Households(location=np.array(locations))


def _form_village(
    population: Population, rows: np.ndarray, first: int, household: np.ndarray, rng: np.random.Generator
) -> int:
    """Form one village's households, numbered from `first`; returns how many."""
    age = population.age_years[rows]
    female = population.female[rows]

    heads = rows[female & (age >= HEAD_AGES[0]) & (age <= HEAD_AGES[1])]
    if len(heads) == 0:  # no women of family age: everyone lives together
        household[rows] = first
        return 1
    heads = heads[np.argsort(population.age_years[heads], kind="stable")]
    ids = first + np.arange(len(heads))
    household[heads] = ids

    # Men pair with women in age order, so partners are of similar age.
    men = rows[~female & (age >= ADULT_AGE) & (age <= PARTNER_MAX_AGE)]
    men = men[np.argsort(population.age_years[men], kind="stable")]
    pairs = min(len(men), len(heads))
    household[men[:pairs]] = ids[:pairs]

    # Children join a woman old enough to be their mother.
    head_ages = population.age_years[heads]
    for child in rows[age < ADULT_AGE]:
        gap = head_ages - population.age_years[child]
        mothers = ids[(gap >= MOTHER_AGE_GAP[0]) & (gap <= MOTHER_AGE_GAP[1])]
        household[child] = rng.choice(mothers if len(mothers) else ids)

    # Everyone else (older people, unpaired men) lives with a family.
    rest = rows[household[rows] < 0]
    household[rest] = rng.choice(ids, size=len(rest))
    return len(heads)


def sizes(population: Population, n_households: int) -> np.ndarray:
    """People in each household."""
    return np.bincount(population.household, weights=population.count, minlength=n_households).astype(np.int64)


def pass_on_savings(
    households: Households, size: np.ndarray, treasury: np.ndarray | None = None, council: np.ndarray | None = None
) -> None:
    """Savings of families with nobody left go to the village council if
    there is one (`council` per village, paid into `treasury`), otherwise to
    the other families of their village."""
    gone = (size == 0) & (households.money > 0)
    if not gone.any():
        return
    n = int(households.location.max()) + 1
    if council is not None and council.any():
        to_council = gone & council[households.location]
        treasury += np.bincount(households.location[to_council], weights=households.money[to_council], minlength=n)
        households.money[to_council] = 0.0
        gone &= ~to_council
        if not gone.any():
            return
    left = np.bincount(households.location[gone], weights=households.money[gone], minlength=n)
    alive = size > 0
    heirs = np.bincount(households.location[alive], minlength=n)
    share = np.divide(left, heirs, out=np.zeros_like(left), where=heirs > 0)
    inherited = np.where(alive, share[households.location], 0.0)
    keep = gone & (heirs[households.location] == 0)  # nobody left in the village at all
    households.money = np.where(gone & ~keep, 0.0, households.money) + inherited
