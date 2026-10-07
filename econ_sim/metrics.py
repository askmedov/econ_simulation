"""Monthly statistics: totals over the population table, never per-person logs."""

from __future__ import annotations

import csv
from dataclasses import dataclass, fields
from pathlib import Path

import numpy as np

from econ_sim import rules
from econ_sim.config import Config
from econ_sim.population import Population


@dataclass
class MonthRecord:
    month_number: int  # months from the start, 1 = first simulated month
    year: int
    month: int  # calendar month, 1-12
    population: int
    children: int
    workers: int
    elderly: int
    households: int  # with at least one member
    births: int
    deaths: int
    food_produced: float
    food_needed: float
    food_eaten: float
    ration: float  # share of food need met, 1.0 = everyone fully fed
    food_spoiled: float
    food_lost: float  # destroyed by events
    food_stock: float  # at the end of the month
    food_margin: float  # normal harvest over need; below ~1.05 the land is crowded
    food_cover: float  # stores plus expected harvests, as a share of the coming year's need
    wage_cover: float  # food a month's pay buys, over food need per worker
    food_price: float  # coins per ration (averaged over villages)
    wage: float  # average monthly wage of a worker, in coins
    savings: float  # coins held by families
    business_cash: float  # coins held by businesses
    shared: float  # coins given by better-off families to families short of food money
    underfed: int  # people whose family got less than 90% of its food need
    poorest_fifth_ration: float  # share of food need met for the poorest fifth of people
    warmth: float  # share of the firewood families needed that they got
    clothing: float  # garments bought per person this month
    job_changes: int  # workers who moved to a better-paid trade
    prices: dict[str, float]  # coins per unit of each product
    jobs: dict[str, int]  # workers in each business
    avg_health: float
    poor_health: int  # people below the danger threshold
    accidents: int
    events: str  # active village events

    def value(self, name: str) -> float:
        """A measure by name, including flattened ones like "price_firewood" or "jobs_farming"."""
        return flat(self)[name]


def flat(record: MonthRecord) -> dict:
    """The record as one flat dict: prices and jobs become price_<product> and jobs_<business>."""
    row = {}
    for f in fields(record):
        value = getattr(record, f.name)
        if f.name == "prices":
            row.update({f"price_{k}": v for k, v in value.items()})
        elif f.name == "jobs":
            row.update({f"jobs_{k}": v for k, v in value.items()})
        else:
            row[f.name] = value
    return row


def age_groups(population: Population, config: Config) -> tuple[int, int, int]:
    """(children, working age, elderly) headcounts."""
    age = population.age_years
    children = int(population.count[age < config.demography.adult_age].sum())
    workers = int(population.count[rules.is_working_age(population, config)].sum())
    return children, workers, population.size - children - workers


def average_health(population: Population) -> float:
    if population.size == 0:
        return 0.0
    return float(np.average(population.health, weights=population.count))


def poor_health(population: Population, config: Config) -> int:
    return int(population.count[population.health < config.health.danger_threshold].sum())


def poorest_fifth_ration(money: np.ndarray, size: np.ndarray, bought: np.ndarray, need: np.ndarray) -> float:
    """Share of food need met for the fifth of people in the poorest families (savings per person)."""
    lived = size > 0
    if not lived.any():
        return 1.0
    money, size, bought, need = money[lived], size[lived], bought[lived], need[lived]
    order = np.argsort(money / size, kind="stable")
    people_before = np.cumsum(size[order]) - size[order]
    poorest = order[people_before < 0.2 * size.sum()]
    wanted = need[poorest].sum()
    return float(bought[poorest].sum() / wanted) if wanted > 0 else 1.0


def write_csv(records: list[MonthRecord], path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", newline="") as f:
        rows = [flat(record) for record in records]
        writer = csv.DictWriter(f, fieldnames=list(rows[0]) if rows else [f.name for f in fields(MonthRecord)])
        writer.writeheader()
        for row in rows:
            writer.writerow({k: round(v, 3) if isinstance(v, float) else v for k, v in row.items()})
