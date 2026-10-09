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
from dataclasses import dataclass, field, replace
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
    (not its world, which can be large), and a few measures for each
    village (see `simulation.PLACE_SERIES`), as (months x villages) arrays."""

    config: Config
    records: list[MonthRecord]
    log: list[str]
    places: dict[str, np.ndarray] = field(default_factory=dict)


def run_one(config: Config) -> Run:
    sim = Simulation(config)
    sim.run()
    return Run(config=sim.config, records=sim.records, log=sim.log, places=sim.place_series())


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
    left: tuple[float, float]  # people who left their village over the whole run
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
        left=pair(lambda sims: series(sims, "emigrants").sum(axis=1)),
        relief=pair(lambda sims: series(sims, "relief").sum(axis=1)),
        lord_relief=pair(lambda sims: series(sims, "lord_relief").sum(axis=1)),
        treated=pair(lambda sims: series(sims, "treated").sum(axis=1)),
    )


def place_series(sims: list[Run], name: str) -> np.ndarray:
    """A per-village measure as a (runs x months x villages) array."""
    return np.array([sim.places[name] for sim in sims], dtype=np.float64)


@dataclass
class PlaceEffect:
    """What the forced events did to one group of villages (by terrain, or
    by distance from market), across runs."""

    label: str
    villages: int
    people: float  # at the start
    extra_deaths: Spread  # over the whole run
    population_change: Spread  # at the end
    lowest_ration: tuple[float, float]  # (baseline, scenario): the group's worst month, averaged over runs
    left: tuple[float, float]  # people who left its villages over the whole run


def effect_by(comparison: Comparison, groups: np.ndarray, labels: list[str]) -> list[PlaceEffect]:
    """The forced events' effect on each group of villages (`groups` gives
    each village's index into `labels`); empty groups are left out."""
    sims = comparison.baseline, comparison.scenario
    people = [place_series(s, "population") for s in sims]
    start = np.array([v.population for v in comparison.baseline[0].config.villages], dtype=np.float64)
    out = []
    for g, label in enumerate(labels):
        mine = groups == g
        if not mine.any():
            continue

        def total(s: list[Run], name: str) -> np.ndarray:
            return place_series(s, name)[:, :, mine].sum(axis=2)  # runs x months

        def worst(s: list[Run], who: np.ndarray) -> float:
            fed = place_series(s, "ration")[:, :, mine]
            weights = np.maximum(who[:, :, mine], 1e-9)
            return float(((fed * weights).sum(axis=2) / weights.sum(axis=2)).min(axis=1).mean())

        out.append(PlaceEffect(
            label=label, villages=int(mine.sum()), people=float(start[mine].sum()),
            extra_deaths=spread((total(sims[1], "deaths") - total(sims[0], "deaths")).sum(axis=1)),
            population_change=spread(total(sims[1], "population")[:, -1] - total(sims[0], "population")[:, -1]),
            lowest_ration=(worst(sims[0], people[0]), worst(sims[1], people[1])),
            left=(float(total(sims[0], "left").sum(axis=1).mean()), float(total(sims[1], "left").sum(axis=1).mean())),
        ))
    return out


def write_places_csv(path: Path, names: tuple[str, ...], about: dict[str, np.ndarray], baseline: list[Run],
                     scenario: list[Run] | None = None) -> None:
    """One row per village: what it is (`about`: terrain, place on the map,
    cost of carriage...) and how it fared, averaged over runs (and with the
    forced events, if any)."""
    arms = [("", baseline)] + ([("_event", scenario)] if scenario is not None else [])
    columns: dict[str, list] = {"village": list(names), **{k: list(v) for k, v in about.items()}}
    for suffix, sims in arms:
        people = place_series(sims, "population")
        columns[f"population_end{suffix}"] = people[:, -1].mean(axis=0)
        columns[f"deaths{suffix}"] = place_series(sims, "deaths").sum(axis=1).mean(axis=0)
        columns[f"left{suffix}"] = place_series(sims, "left").sum(axis=1).mean(axis=0)
        columns[f"lowest_ration{suffix}"] = place_series(sims, "ration").min(axis=1).mean(axis=0)
        columns[f"highest_price{suffix}"] = place_series(sims, "food_price").max(axis=1).mean(axis=0)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", newline="") as f:
        writer = csv.writer(f)
        writer.writerow(columns)
        for i in range(len(names)):
            writer.writerow(v[i] if isinstance(v[i], str) else _fmt(v[i]) for v in columns.values())


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
