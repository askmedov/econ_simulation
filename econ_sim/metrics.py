"""Monthly statistics: totals over the population table, never per-person logs."""

from __future__ import annotations

import csv
from dataclasses import asdict, dataclass, fields
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
    births: int
    deaths: int
    food_produced: float
    food_needed: float
    food_eaten: float
    ration: float  # share of food need met, 1.0 = everyone fully fed
    food_spoiled: float
    food_lost: float  # destroyed by events
    food_stock: float  # at the end of the month
    avg_health: float
    poor_health: int  # people below the danger threshold
    accidents: int
    events: str  # active village events


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


def write_csv(records: list[MonthRecord], path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=[field.name for field in fields(MonthRecord)])
        writer.writeheader()
        for record in records:
            row = asdict(record)
            writer.writerow({k: round(v, 3) if isinstance(v, float) else v for k, v in row.items()})
