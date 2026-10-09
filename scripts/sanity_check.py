"""Run many seeds and settings and flag outcomes no real village would show.

    python scripts/sanity_check.py            # quick: a few hundred runs
    python scripts/sanity_check.py --full     # thorough: thousands of runs

Three kinds of finding:
  impossible  values that can't happen (negative food, broken accounting)
  nonsense    behaviour no real village shows (hunger with a full granary,
              a year of hunger with no cause, implausible birth rates)
  disaster    real but rare outcomes (starvation rations, a year in which
              30% die), allowed in a small share of runs, more in harsh
              settings (a setting fails when its count would be under 1%
              likely at that share)

Exit code 1 if anything impossible or nonsensical is found, or disasters
are more frequent than a setting allows.
"""

from __future__ import annotations

import argparse
import math
import sys
from collections import defaultdict
from dataclasses import dataclass, replace
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from econ_sim.checks import broken_invariants  # noqa: E402
from econ_sim.geography import RegionSettings, region  # noqa: E402
from econ_sim.config import Config, CouncilConfig, HealthcareConfig, ScheduledEvent, VillageConfig  # noqa: E402
from econ_sim.metrics import flat  # noqa: E402
from econ_sim.scenarios import default_workers, process_pool  # noqa: E402
from econ_sim.simulation import Simulation  # noqa: E402


IMPOSSIBLE = {
    "not-a-number value",
    "negative value",
    "ration or health out of range",
    "age groups don't add up",
    "money created or destroyed",
    "price not positive",
}
DISASTERS = {
    "starvation rations (below 50%)",
    "deaths above 30% in a year",
    # A small village can starve out, or empty as its people move away after
    # disasters or a bad living, as thousands of medieval villages did; a
    # village of many hundreds dying out within a few years is nonsense.
    "small village died out or was abandoned",
}


def problems(sim: Simulation, extreme: bool = False) -> list[tuple[str, int]]:
    """(problem, month number) for everything implausible in a finished run.

    In an `extreme` setting (e.g. no food at all) famine is the right answer,
    so only impossible values are flagged.
    """
    found = []
    config, records = sim.config, sim.records
    start_pop = sum(v.population for v in config.villages)
    one_village = len(config.villages) == 1

    def flag(name: str, month: int) -> None:
        found.append((name, month))

    def coins(r) -> float:
        return r.savings + r.business_cash + r.treasury + r.lord_purse + r.state_purse + r.town_purse + r.coins_lost

    money = coins(records[0]) if records else 0.0
    start_prices = dict(records[0].prices) if records else {}
    hungry_amid_plenty = 0
    for r in records:
        values = flat(r)
        if any(isinstance(v, float) and not math.isfinite(v) for v in values.values()):
            flag("not-a-number value", r.month_number)
        if min(r.population, r.food_stock, r.food_eaten, r.food_produced, r.avg_health, r.savings, r.business_cash,
               r.treasury, r.food_reserve) < 0:
            flag("negative value", r.month_number)
        if not math.isclose(coins(r), money, rel_tol=1e-6, abs_tol=1e-6):
            flag("money created or destroyed", r.month_number)
        if not all(price > 0 for price in r.prices.values()) and r.population > 0:
            flag("price not positive", r.month_number)
        if not 0 <= r.ration <= 1 + 1e-9 or r.avg_health > 100 + 1e-9:
            flag("ration or health out of range", r.month_number)
        if r.children + r.workers + r.elderly != r.population:
            flag("age groups don't add up", r.month_number)
        if extreme:
            continue
        # Families own their stores now, so a poor family can go short while a
        # rich one holds plenty; but the whole village going hungry amid
        # full stores means something is broken.
        if one_village and r.food_needed > 0 and r.ration < 0.85 and r.food_stock > 6 * r.food_needed:
            flag("rationing with 6+ months of food in store", r.month_number)
        if r.food_needed > 0 and r.food_stock > 36 * r.food_needed:
            flag("more than 3 years of food in store", r.month_number)
        if r.population >= 20 and r.ration < 0.5:
            flag("starvation rations (below 50%)", r.month_number)
        if any(not start_prices[k] / 20 < v < 20 * start_prices[k] for k, v in r.prices.items()):
            flag("a price off by 20x", r.month_number)
        if r.population >= 50 and r.jobs.get("farming", 1) == 0:
            flag("nobody farms", r.month_number)
        # Families too poor to buy food while the granary is full, for months.
        plenty = one_village and r.food_stock > 6 * r.food_needed
        hungry_amid_plenty = hungry_amid_plenty + 1 if plenty and r.underfed > r.population / 6 else 0
        if hungry_amid_plenty == 6:
            flag("many go hungry for months while the granary is full", r.month_number)
    # The state at the end: land, debts, households and couples consistent.
    for name in broken_invariants(sim.world):
        flag(f"broken invariant: {name}", len(records))
    if extreme:
        return found

    if records and records[-1].population == 0:
        died = next(r.month_number for r in records if r.population == 0)
        flag("small village died out or was abandoned" if start_pop < 300 else "village died out", died)

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
        # A region on a map: shared weather, carriage by road and river, terrain.
        Setting("region of 20 villages", region(RegionSettings(villages=20, mean_size=300), base), seeds // 10),
        Setting("region, 10 years", replace(region(RegionSettings(villages=20, mean_size=300, seed=2), base), months=120),
                seeds // 20),
        Setting("no council", replace(base, council=CouncilConfig(enabled=False)), seeds // 2),
        Setting("no healthcare", replace(base, healthcare=HealthcareConfig(enabled=False)), seeds // 4),
        Setting("council forming", replace(base, villages=(VillageConfig(population=520, land=182),),
                                            council=CouncilConfig(established_at_start=False)), seeds // 4),
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


def too_many(count: int, runs: int, allowed: float) -> bool:
    """Whether `count` disasters in `runs` runs is implausible (under 1%
    likely) if they happen in at most `allowed` of runs."""
    if allowed <= 0:
        return count > 0
    tail = sum(math.comb(runs, k) * allowed**k * (1 - allowed) ** (runs - k) for k in range(count, runs + 1))
    return tail < 0.01


def check(job: tuple[Config, bool]) -> list[tuple[str, int]]:
    """Run one setting with one seed and return its problems."""
    config, extreme = job
    sim = Simulation(config)
    sim.run()
    return problems(sim, extreme=extreme)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--full", action="store_true", help="many more seeds")
    parser.add_argument("--workers", type=int, default=None, help="cores to use (default: all)")
    args = parser.parse_args()

    failures, runs = [], 0
    chosen = settings(args.full)
    jobs = [(replace(s.config, seed=seed), s.disasters_allowed is None) for s in chosen for seed in range(s.seeds)]
    workers = args.workers or default_workers()
    if workers > 1:
        with process_pool(workers) as pool:
            found = iter(list(pool.map(check, jobs, chunksize=4)))
    else:
        found = iter([check(job) for job in jobs])
    for setting in chosen:
        counts: dict[str, int] = defaultdict(int)
        examples: dict[str, str] = {}
        for seed in range(setting.seeds):
            runs += 1
            seen = set()
            for name, month in next(found):
                if name in seen:
                    continue
                seen.add(name)
                counts[name] += 1
                examples.setdefault(name, f"seed {seed}, month {month}")
        notes = []
        for name, n in sorted(counts.items(), key=lambda item: -item[1]):
            if name in DISASTERS:
                # Too many if that many would happen less than 1% of the time
                # at the allowed rate (a few in a handful of runs is luck).
                bad = too_many(n, setting.seeds, setting.disasters_allowed or 0.0)
                kind = "disaster"
            else:
                impossible = name in IMPOSSIBLE or name.startswith("broken invariant")
                bad, kind = True, "impossible" if impossible else "nonsense"
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
