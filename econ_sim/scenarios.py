"""What-if experiments: many runs, with and without forced events.

Every run with the forced events is paired with a baseline run that has the
same seed. Because each rule draws from its own named random stream, the
pair shares the same luck up to the month the event starts, so the gap
between them is the event's effect rather than chance. Repeating this over
many seeds shows how large and how certain that effect is.
"""

from __future__ import annotations

import csv
import multiprocessing
import os
import sys
from concurrent.futures import ProcessPoolExecutor
from dataclasses import dataclass, replace
from pathlib import Path

import numpy as np

from econ_sim.config import Config, ScheduledEvent
from econ_sim.metrics import MonthRecord
from econ_sim.simulation import Simulation

# Monthly measures worth comparing, in the order they are reported.
METRICS = (
    "population",
    "births",
    "deaths",
    "food_stock",
    "food_price",
    "wage",
    "ration",
    "poorest_fifth_ration",
    "landless_ration",
    "underfed",
    "avg_health",
    "poor_health",
    "relief",
    "treated",
    "treasury",
    "food_reserve",
)


@dataclass
class Run:
    """What a finished run leaves: its settings, monthly records and log
    (not its world, which can be large)."""

    config: Config
    records: list[MonthRecord]
    log: list[str]


def run_one(config: Config) -> Run:
    sim = Simulation(config)
    sim.run()
    return Run(config=sim.config, records=sim.records, log=sim.log)


def default_workers() -> int:
    """Cores to run seeds on: `ECON_SIM_WORKERS` if set, else all of them."""
    return int(os.environ.get("ECON_SIM_WORKERS", 0)) or os.cpu_count() or 1


def process_pool(workers: int) -> ProcessPoolExecutor:
    """A pool of `workers` processes: forked where the system allows (fast,
    and it works from a notebook or a piped script), spawned elsewhere."""
    fork = sys.platform.startswith("linux") and "fork" in multiprocessing.get_all_start_methods()
    return ProcessPoolExecutor(max_workers=workers, mp_context=multiprocessing.get_context("fork" if fork else "spawn"))


def run_many(configs: list[Config], workers: int | None = None) -> list[Run]:
    """Run each config, several at a time on separate cores; results in order."""
    workers = min(default_workers() if workers is None else workers, len(configs))
    if workers <= 1:
        return [run_one(config) for config in configs]
    with process_pool(workers) as pool:
        return list(pool.map(run_one, configs))


def run_batch(config: Config, runs: int, workers: int | None = None) -> list[Run]:
    """Run `runs` simulations with seeds config.seed, config.seed + 1, ..."""
    return run_many([replace(config, seed=config.seed + i) for i in range(runs)], workers)


def series(sims: list[Run], metric: str) -> np.ndarray:
    """A metric as a (runs x months) array."""
    return np.array([[r.value(metric) for r in sim.records] for sim in sims], dtype=np.float64)


@dataclass
class Spread:
    """Average across runs, with the range that 80% of runs fall into."""

    mean: np.ndarray | float
    low: np.ndarray | float  # 10th percentile
    high: np.ndarray | float  # 90th percentile


def spread(values: np.ndarray) -> Spread:
    """Spread across runs (axis 0) of a (runs x ...) array."""
    low, high = np.percentile(values, [10, 90], axis=0)
    return Spread(values.mean(axis=0), low, high)


@dataclass
class Comparison:
    baseline: list[Run]
    scenario: list[Run]

    def difference(self, metric: str) -> np.ndarray:
        """Scenario minus baseline, run by run (runs x months)."""
        return series(self.scenario, metric) - series(self.baseline, metric)


def compare(
    config: Config, forced: tuple[ScheduledEvent, ...], runs: int, workers: int | None = None
) -> Comparison:
    """Paired runs of `config` without and with the `forced` events (all of
    them at once, on `workers` cores)."""
    with_events = replace(config, scheduled_events=config.scheduled_events + forced)
    seeds = [config.seed + i for i in range(runs)]
    results = run_many([replace(config, seed=s) for s in seeds] + [replace(with_events, seed=s) for s in seeds], workers)
    return Comparison(baseline=results[:runs], scenario=results[runs:])


@dataclass
class Effect:
    """What the forced events did over the whole run, across runs."""

    extra_deaths: Spread
    fewer_births: Spread
    population_change: Spread  # at the end
    lowest_ration: tuple[float, float]  # (baseline, scenario), averaged over runs
    poorest_lowest_ration: tuple[float, float]  # the poorest fifth's worst month
    landless_lowest_ration: tuple[float, float]  # landless families' worst month
    hungry_months: tuple[float, float]  # months in which the village ate under 97% of its need
    highest_price: tuple[float, float]  # food price at its peak
    lowest_health: tuple[float, float]
    relief: tuple[float, float]  # rations the council gave out over the whole run
    lord_relief: tuple[float, float]  # rations a charitable lord gave out over the whole run
    treated: tuple[float, float]  # people healers saw over the whole run


def effect(comparison: Comparison) -> Effect:
    def pair(stat) -> tuple[float, float]:
        return (
            float(stat(comparison.baseline).mean()),
            float(stat(comparison.scenario).mean()),
        )

    return Effect(
        extra_deaths=spread(comparison.difference("deaths").sum(axis=1)),
        fewer_births=spread(-comparison.difference("births").sum(axis=1)),
        population_change=spread(comparison.difference("population")[:, -1]),
        lowest_ration=pair(lambda sims: series(sims, "ration").min(axis=1)),
        poorest_lowest_ration=pair(lambda sims: series(sims, "poorest_fifth_ration").min(axis=1)),
        landless_lowest_ration=pair(lambda sims: series(sims, "landless_ration").min(axis=1)),
        hungry_months=pair(lambda sims: (series(sims, "ration") < 0.97).sum(axis=1)),
        highest_price=pair(lambda sims: series(sims, "food_price").max(axis=1)),
        lowest_health=pair(lambda sims: series(sims, "avg_health").min(axis=1)),
        relief=pair(lambda sims: series(sims, "relief").sum(axis=1)),
        lord_relief=pair(lambda sims: series(sims, "lord_relief").sum(axis=1)),
        treated=pair(lambda sims: series(sims, "treated").sum(axis=1)),
    )


def write_summary_csv(
    path: Path, baseline: list[Run], scenario: list[Run] | None = None
) -> None:
    """One row per month: each metric's average and 10-90% range across runs.

    With a scenario, adds the same for the scenario (`_event`) and for the
    difference between paired runs (`_diff`).
    """
    columns: dict[str, np.ndarray] = {}
    first = baseline[0].records
    columns["month_number"] = np.array([r.month_number for r in first])
    columns["year"] = np.array([r.year for r in first])
    columns["month"] = np.array([r.month for r in first])
    for metric in METRICS:
        groups = [("", series(baseline, metric))]
        if scenario is not None:
            groups.append(("_event", series(scenario, metric)))
            groups.append(("_diff", series(scenario, metric) - series(baseline, metric)))
        for suffix, values in groups:
            s = spread(values)
            columns[f"{metric}{suffix}"] = s.mean
            columns[f"{metric}{suffix}_low"] = s.low
            columns[f"{metric}{suffix}_high"] = s.high

    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", newline="") as f:
        writer = csv.writer(f)
        writer.writerow(columns)
        for i in range(len(first)):
            writer.writerow(_fmt(values[i]) for values in columns.values())


def _fmt(value) -> str | int:
    if isinstance(value, (np.integer, int)):
        return int(value)
    return f"{float(value):.3f}".rstrip("0").rstrip(".")
