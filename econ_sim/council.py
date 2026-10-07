"""The village council: it forms once the village is big enough, then taxes
wages, employs officials, keeps a food reserve filled by a grain levy, and
gives famine relief.

State is one value per village. Council staff have job codes past the
businesses: officials are `official_job(config)`.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np

from econ_sim.config import Config
from econ_sim.economy import in_business
from econ_sim.population import NO_JOB, Population


def official_job(config: Config) -> int:
    return len(config.businesses)


@dataclass
class Councils:
    formed: np.ndarray  # whether each village has a council
    months_ready: np.ndarray  # months the village has been big enough
    treasury: np.ndarray  # coins
    reserve: np.ndarray  # food held for famine relief, in rations

    @classmethod
    def none(cls, n_locations: int) -> Councils:
        return cls(
            formed=np.zeros(n_locations, dtype=bool),
            months_ready=np.zeros(n_locations, dtype=np.int64),
            treasury=np.zeros(n_locations),
            reserve=np.zeros(n_locations),
        )


def check_formation(councils: Councils, people: np.ndarray, config: Config) -> np.ndarray:
    """Count months each village has been big enough; returns villages whose council forms now."""
    cfg = config.council
    if not cfg.enabled:
        return np.zeros(len(people), dtype=bool)
    big = people >= cfg.forms_at_population
    councils.months_ready = np.where(big, councils.months_ready + 1, 0)
    new = ~councils.formed & (councils.months_ready >= cfg.forms_after_months)
    councils.formed |= new
    return new


def staff(
    population: Population, councils: Councils, job: int, per_1000: float, config: Config, rng: np.random.Generator
) -> None:
    """Hire workers from the trades into a council job, or let extras go, to
    reach `per_1000` staff per 1,000 people where a council has formed."""
    n = len(councils.formed)
    people = np.bincount(population.location, weights=population.count, minlength=n)
    wanted = np.where(councils.formed, np.round(per_1000 * people / 1000.0), 0.0)
    for location in range(n):
        here = population.location == location
        current = np.flatnonzero(here & (population.job == job))
        have = int(population.count[current].sum())
        if have > wanted[location]:
            extra = rng.permutation(current)[: int(have - wanted[location])]
            population.job[extra] = NO_JOB  # they rejoin a trade next month
        elif have < wanted[location]:
            pool = np.flatnonzero(here & in_business(population, config) & (population.count == 1))
            hired = rng.permutation(pool)[: int(wanted[location] - have)]
            population.job[hired] = job


def collect_taxes(
    income: np.ndarray, location: np.ndarray, councils: Councils, target: np.ndarray, famine: np.ndarray, config: Config
) -> np.ndarray:
    """Take tax from families' wages where a council sits and its treasury
    is below `target`, except in a `famine` (per village). Changes `income`;
    returns tax per village."""
    cfg = config.council
    n = len(councils.formed)
    taxing = councils.formed & (councils.treasury < target) & ~famine
    tax = income * cfg.tax_rate * taxing[location]
    income -= tax
    collected = np.bincount(location, weights=tax, minlength=n)
    councils.treasury += collected
    return collected


def pay_staff(
    population: Population, councils: Councils, job: int, pay: np.ndarray, n_households: int
) -> tuple[np.ndarray, np.ndarray]:
    """Pay council staff `pay` each (per village) from the treasury, or what
    it can afford. Returns (income per household, paid per village)."""
    n = len(councils.formed)
    staff_rows = population.job == job
    heads = np.bincount(population.location[staff_rows], weights=population.count[staff_rows], minlength=n)
    owed = heads * pay
    paid = np.minimum(owed, councils.treasury)
    rate = np.divide(paid, heads, out=np.zeros(n), where=heads > 0)
    councils.treasury -= paid
    earned = np.where(staff_rows, population.count * rate[population.location], 0.0)
    income = np.bincount(population.household, weights=earned, minlength=n_households).astype(np.float64)
    return income, paid


def levy_grain(
    councils: Councils, produced: np.ndarray, granary: np.ndarray, need: np.ndarray, famine: np.ndarray, config: Config
) -> np.ndarray:
    """Take a share of the harvest into the council's reserve (a tithe in
    kind) until it holds its target, except in a famine. Changes `granary`;
    returns rations taken per village."""
    cfg = config.council
    room = np.maximum(cfg.reserve_months * need - councils.reserve, 0.0)
    taken = np.where(councils.formed & ~famine, np.minimum(cfg.grain_levy * produced, room), 0.0)
    taken = np.minimum(taken, granary)
    councils.reserve += taken
    granary -= taken
    return taken


def give_relief(
    councils: Councils, bought: np.ndarray, need: np.ndarray, location: np.ndarray, config: Config
) -> np.ndarray:
    """Families who couldn't buy enough food get the rest free from the
    reserve, shared out if it runs short. Returns rations given per household."""
    if not config.council.relief:
        return np.zeros_like(bought)
    n = len(councils.formed)
    gap = np.maximum(need - bought, 0.0)
    wanted = np.bincount(location, weights=gap, minlength=n)
    given = np.where(councils.formed, np.minimum(wanted, councils.reserve), 0.0)
    share = np.divide(given, wanted, out=np.zeros(n), where=wanted > 0)
    councils.reserve -= given
    return gap * share[location]


def cash_relief(
    councils: Councils, money: np.ndarray, shortfall: np.ndarray, location: np.ndarray, keep: np.ndarray, config: Config
) -> np.ndarray:
    """Give coins to families who can't afford this month's food and firewood,
    from whatever the treasury holds beyond `keep` (per village). Changes
    `money`; returns coins given per village."""
    n = len(councils.formed)
    if not config.council.relief:
        return np.zeros(n)
    wanted = np.bincount(location, weights=shortfall, minlength=n)
    spare = np.maximum(councils.treasury - keep, 0.0)
    given = np.where(councils.formed, np.minimum(wanted, spare), 0.0)
    share = np.divide(given, wanted, out=np.zeros(n), where=wanted > 0)
    money += shortfall * share[location]
    councils.treasury -= given
    return given
