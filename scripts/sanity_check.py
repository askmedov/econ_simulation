"""Run many seeds and settings and flag outcomes no real village would show.

    python scripts/sanity_check.py            # quick: a few hundred runs
    python scripts/sanity_check.py --full     # thorough: thousands of runs

Three kinds of finding:
  impossible  values that can't happen (negative food, broken accounting)
  nonsense    behaviour no real village shows (hunger with a full granary,
              a year of hunger with no cause, implausible birth rates)
  disaster    real but rare outcomes (starvation rations, a year in which
              30% die), allowed in a small share of runs, more in harsh
              settings

Exit code 1 if anything impossible or nonsensical is found, or disasters
are more frequent than a setting allows.
"""

from __future__ import annotations

import argparse
import math
import sys
from collections import defaultdict
from dataclasses import asdict, dataclass, replace
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from econ_sim.config import Config, ScheduledEvent, VillageConfig  # noqa: E402
from econ_sim.simulation import Simulation  # noqa: E402


IMPOSSIBLE = {
    "not-a-number value",
    "negative value",
    "ration or health out of range",
    "age groups don't add up",
}
DISASTERS = {"starvation rations (below 50%)", "deaths above 30% in a year"}


def problems(sim: Simulation, extreme: bool = False) -> list[tuple[str, int]]:
    """(problem, month number) for everything implausible in a finished run.

    In an `extreme` setting (e.g. no food at all) famine is the right answer,
    so only impossible values are flagged.
    """
    found = []
    config, records = sim.config, sim.records
    start_pop = config.villages[0].population * len(config.villages)
    one_village = len(config.villages) == 1

    def flag(name: str, month: int) -> None:
        found.append((name, month))

    for r in records:
        values = asdict(r)
        if any(isinstance(v, float) and not math.isfinite(v) for v in values.values()):
            flag("not-a-number value", r.month_number)
        if min(r.population, r.food_stock, r.food_eaten, r.food_produced, r.avg_health) < 0:
            flag("negative value", r.month_number)
        if not 0 <= r.ration <= 1 + 1e-9 or r.avg_health > 100 + 1e-9:
            flag("ration or health out of range", r.month_number)
        if r.children + r.workers + r.elderly != r.population:
            flag("age groups don't add up", r.month_number)
        if extreme:
            continue
        if one_village and r.food_needed > 0 and r.ration < 0.95 and r.food_stock > 6 * r.food_needed:
            flag("rationing with 6+ months of food in store", r.month_number)
        if r.food_needed > 0 and r.food_stock > 36 * r.food_needed:
            flag("more than 3 years of food in store", r.month_number)
        if r.population >= 20 and r.ration < 0.5:
            flag("starvation rations (below 50%)", r.month_number)
    if extreme:
        return found

    if records and records[-1].population == 0:
        flag("village died out", next(r.month_number for r in records if r.population == 0))

    # Yearly rates, over full years only.
    for start in range(0, len(records) - 11, 12):
        year = records[start : start + 12]
        pop = np.mean([r.population for r in year])
        if pop < 20:
            continue
        births, deaths = sum(r.births for r in year), sum(r.deaths for r in year)
        # Allow for luck in small villages: four standard deviations above
        # a high but real birth rate of 6% a year.
        if births > 0.06 * pop + 4 * np.sqrt(0.06 * pop):
            flag("implausibly many births in a year", year[0].month_number)
        if deaths / pop > 0.30:
            flag("deaths above 30% in a year", year[0].month_number)
        # Hunger is explained by an event or by too many people on the land.
        if one_village and all(r.ration < 0.8 and not r.events and r.food_margin >= 1.05 for r in year):
            flag("a year of hunger with no event or crowding to explain it", year[0].month_number)

    if len(records) >= 36 and start_pop >= 50:
        pops = [r.population for r in records]
        for i in range(36, len(pops), 12):
            if pops[i] > 1.5 * max(pops[i - 36], 1):
                flag("population up 50%+ in 3 years", i + 1)
                break

    pop = sim.world.population
    adults = pop.age_years >= 15
    adult_count = pop.count[adults].sum()
    if adult_count >= 40:
        female_share = pop.count[adults & pop.female].sum() / adult_count
        if not 0.3 <= female_share <= 0.7:
            flag("lopsided sex ratio among adults", len(records))
    return found


@dataclass
class Setting:
    label: str
    config: Config
    seeds: int
    # Share of runs allowed to have a disaster; None means an extreme setting
    # where only impossible values count.
    disasters_allowed: float | None = 0.05


def settings(full: bool) -> list[Setting]:
    base = Config()
    seeds = 400 if full else 100
    all_bad = tuple(
        ScheduledEvent(name, month)
        for name, month in [("drought", 4), ("disease", 9), ("granary_fire", 10), ("harsh_winter", 12)]
    )
    out = [
        Setting("default 36 months", base, seeds),
        Setting("default 60 months", replace(base, months=60), seeds),
        Setting("century", replace(base, months=1200), 60 if full else 15, disasters_allowed=0.5),
        Setting("no random events, century", replace(base, months=1200, random_events=False), 10 if full else 3, 0.0),
        Setting("lots of land", replace(base, villages=(VillageConfig(land=1500),)), seeds // 4),
        Setting("somewhat crowded", replace(base, villages=(VillageConfig(land=250),)), seeds // 4),
        Setting("tiny village", replace(base, villages=(VillageConfig(population=20, land=7),)), seeds // 2),
        Setting("small village", replace(base, villages=(VillageConfig(population=100, land=35),)), seeds // 2),
        Setting("big village", replace(base, villages=(VillageConfig(population=5000, land=1750),)), seeds // 20),
        Setting("two villages", replace(base, villages=(VillageConfig(name="A"), VillageConfig(name="B", land=250))), seeds // 4),
        # Harsh: disasters are expected now and then.
        Setting("every bad event at once", replace(base, scheduled_events=all_bad), seeds // 2, disasters_allowed=0.3),
        # Extreme: famine is the right answer.
        Setting(
            "whole harvest lost",
            replace(base, start_month=10, villages=(VillageConfig(initial_food_months=0),)),
            seeds // 4,
            None,
        ),
        Setting("empty granary in January", replace(base, villages=(VillageConfig(initial_food_months=0),)), seeds // 4, None),
        Setting("far too little land", replace(base, villages=(VillageConfig(land=120),)), seeds // 4, None),
    ]
    for month in range(2, 13):
        out.append(Setting(f"start in month {month}", replace(base, start_month=month), seeds // 10))
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--full", action="store_true", help="many more seeds")
    args = parser.parse_args()

    failures, runs = [], 0
    for setting in settings(args.full):
        counts: dict[str, int] = defaultdict(int)
        examples: dict[str, str] = {}
        for seed in range(setting.seeds):
            sim = Simulation(replace(setting.config, seed=seed))
            sim.run()
            runs += 1
            seen = set()
            for name, month in problems(sim, extreme=setting.disasters_allowed is None):
                if name in seen:
                    continue
                seen.add(name)
                counts[name] += 1
                examples.setdefault(name, f"seed {seed}, month {month}")
        notes = []
        for name, n in sorted(counts.items(), key=lambda item: -item[1]):
            share = n / setting.seeds
            if name in DISASTERS:
                bad = share > (setting.disasters_allowed or 0.0)
                kind = "disaster"
            else:
                bad, kind = True, "impossible" if name in IMPOSSIBLE else "nonsense"
            notes.append(f"{'FAIL' if bad else 'ok  '} {kind}: {name} in {n} of {setting.seeds} runs (e.g. {examples[name]})")
            if bad:
                failures.append(setting.label)
        status = "FAIL" if setting.label in failures else "ok"
        print(f"{status:<4} {setting.label} ({setting.seeds} runs)")
        for note in notes:
            print(f"       {note}")

    print(f"\n{runs} runs checked; " + (f"{len(set(failures))} settings failed" if failures else "all settings passed"))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
