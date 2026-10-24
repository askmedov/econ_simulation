"""Statistical parity between the Rust engine and the Python model.

    python scripts/parity.py setup              # starting states, 200 seeds per setting
    python scripts/parity.py village            # the village settings, 100 seeds each
    python scripts/parity.py region             # the region settings
    python scripts/parity.py village --only drought --runs 300

Both engines run the same config (written from Python's to a JSON file the
Rust engine reads) over the same number of seeds; the draws differ, so the
runs are compared in distribution: for each measure, the mean over runs in
each engine, the difference, and Welch's t (the difference over its
standard error). A measure passes when |t| is at most 3 (or, when neither
engine varies, when they agree exactly). Prints a table per setting;
`--markdown` prints it for PLAN.md. Exit code 1 if anything fails.

Needs the Rust engine built: cargo build --release (in rust/).
"""

from __future__ import annotations

import argparse
import dataclasses
import io
import json
import math
import os
import subprocess
import sys
import tempfile
from dataclasses import replace
from multiprocessing import Pool
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

from econ_sim import households as households_module  # noqa: E402
from econ_sim.config import Config, CouncilConfig, LordConfig, MigrationConfig, ScheduledEvent, TownConfig, VillageConfig  # noqa: E402
from econ_sim.geography import RegionSettings, region  # noqa: E402
from econ_sim.metrics import flat  # noqa: E402
from econ_sim.simulation import Simulation  # noqa: E402

RUST = ROOT / "rust" / "target" / "release" / "econ-sim"


# ---------------------------------------------------------------------------
# Settings


@dataclasses.dataclass
class Setting:
    name: str
    config: Config
    forced: tuple[ScheduledEvent, ...] = ()
    runs: int = 100


def village_settings() -> list[Setting]:
    base = Config()
    return [
        Setting("default village", base),
        Setting("drought@4, paired", base, (ScheduledEvent("drought", 4),)),
        Setting("no council", replace(base, council=CouncilConfig(enabled=False))),
        Setting("no lord", replace(base, lord=LordConfig(enabled=False))),
        Setting("no town", replace(base, town=TownConfig(enabled=False))),
        Setting("no migration", replace(base, migration=MigrationConfig(enabled=False))),
        Setting("small village", replace(base, villages=(VillageConfig(population=100, land=35),))),
        Setting("40 years", replace(base, months=480), runs=40),
    ]


def region_settings() -> list[Setting]:
    base = Config()
    region20 = region(RegionSettings(villages=20, mean_size=300), base)
    city = region(RegionSettings(villages=45, mean_size=250, towns=3, seed=3), base)
    region100 = region(RegionSettings(villages=100, seed=1), base)
    return [
        Setting("region of 20 villages", region20, runs=60),
        Setting("region of 20, 10 years", replace(region(RegionSettings(villages=20, mean_size=300, seed=2), base),
                                                  months=120), runs=30),
        Setting("region with a city and 3 towns", city, runs=40),
        Setting("100 villages, drought over the west", region100, (ScheduledEvent("drought", 4, near=(0.0, 12.0, 11.0)),),
                runs=30),
    ]


def setup_settings() -> list[Setting]:
    base = Config()
    return [
        Setting("default village", base, runs=200),
        Setting("small village", replace(base, villages=(VillageConfig(population=100, land=35),)), runs=300),
        Setting("region with a city and 3 towns", region(RegionSettings(villages=45, mean_size=250, towns=3, seed=3), base),
                runs=60),
    ]


# ---------------------------------------------------------------------------
# Running both engines


def write_config(config: Config) -> str:
    handle = tempfile.NamedTemporaryFile("w", suffix=".json", delete=False)
    json.dump(json.loads(json.dumps(dataclasses.asdict(config))), handle)
    handle.close()
    return handle.name


def event_text(e: ScheduledEvent) -> str:
    return f"{e.event}@{e.month}" + (f"@{e.near[0]},{e.near[1]},{e.near[2]}" if e.near else "")


def rust(command: str, setting: Setting) -> str:
    if not RUST.exists():
        raise SystemExit(f"{RUST} not found: build it with `cargo build --release` in rust/")
    path = write_config(setting.config)
    try:
        args = [str(RUST), command, "--config", path, "--runs", str(setting.runs)]
        for e in setting.forced:
            args += ["--event", event_text(e)]
        return subprocess.run(args, check=True, capture_output=True, text=True).stdout
    finally:
        os.unlink(path)


def python_state(world) -> dict:
    """The world's starting state, as the Rust engine dumps it."""
    pop, hh = world.population, world.households
    return {
        "population": {k: getattr(pop, k).tolist() for k in
                       ("count", "age_months", "female", "health", "skill", "location", "household", "job", "married", "couple")},
        "households": {k: getattr(hh, k).tolist() for k in ("location", "money", "land", "grain", "animals", "debt", "lent", "kin")},
        "land": world.land.tolist(), "prices": world.prices.tolist(), "wage_level": world.wage_level.tolist(),
        "stock": world.stock.tolist(), "cash": world.cash.tolist(), "tools": world.tools.tolist(),
        "orders": world.orders.tolist(),
        "council": {"formed": world.council.formed.tolist(), "treasury": world.council.treasury.tolist(),
                    "reserve": world.council.reserve.tolist()},
        "town_level": world.town.level, "woods": world.woods.tolist(), "woods_capacity": world.woods_capacity.tolist(),
        "dealers_grain": world.dealers_grain.tolist(), "dealers_cash": world.dealers_cash.tolist(),
    }


def _python_world(config: Config) -> dict:
    return python_state(Simulation(config).world)


def _python_records(job) -> list[dict]:
    config, forced = job
    rows = []
    for arm, c in [("base", config)] + ([("event", replace(config, scheduled_events=config.scheduled_events + forced))]
                                       if forced else []):
        sim = Simulation(c)
        sim.run()
        rows += [{"arm": arm, "seed": c.seed, **flat(r)} for r in sim.records]
    return rows


def python_worlds(setting: Setting) -> list[dict]:
    configs = [replace(setting.config, seed=setting.config.seed + i) for i in range(setting.runs)]
    with Pool() as pool:
        return pool.map(_python_world, configs)


def python_records(setting: Setting) -> list[dict]:
    jobs = [(replace(setting.config, seed=setting.config.seed + i), setting.forced) for i in range(setting.runs)]
    with Pool() as pool:
        return [row for rows in pool.map(_python_records, jobs, chunksize=1) for row in rows]


def rust_records(setting: Setting) -> list[dict]:
    import csv

    reader = csv.DictReader(io.StringIO(rust("records", setting)))
    rows = []
    for row in reader:
        rows.append({k: (v if k in ("arm", "events") else float(v)) for k, v in row.items()})
    return rows


# ---------------------------------------------------------------------------
# Measures


def gini(values: np.ndarray) -> float:
    values = np.sort(np.maximum(values, 0.0))
    if values.sum() <= 0:
        return 0.0
    n = len(values)
    return float((2 * np.arange(1, n + 1) - n - 1).dot(values) / (n * values.sum()))


def setup_measures(state: dict, config: Config) -> dict[str, float]:
    """What a starting state looks like, in numbers that should agree in distribution."""
    pop = {k: np.asarray(v) for k, v in state["population"].items()}
    hh = {k: np.asarray(v, dtype=np.float64) for k, v in state["households"].items()}
    count = pop["count"].astype(np.float64)
    years = pop["age_months"] // 12
    people = count.sum()
    size = np.bincount(pop["household"], weights=count, minlength=len(hh["location"]))
    living = size > 0
    adults = years >= 18
    holders = living & (hh["land"] > 0)
    jobs = pop["job"]
    nb = len(config.businesses)
    working = (jobs >= 0) & (jobs < nb)
    out = {
        "people": people,
        "mean age": float((years * count).sum() / people),
        "children (under 15)": float(count[years < 15].sum() / people),
        "old (60 and over)": float(count[years >= 60].sum() / people),
        "women": float(count[pop["female"].astype(bool)].sum() / people),
        "households": float(living.sum()),
        "household size": float(size[living].mean()),
        "largest household": float(size.max()),
        "people living alone": float((size == 1).sum() / living.sum()),
        "adults married": float(count[adults & pop["married"].astype(bool)].sum() / count[adults].sum()),
        "couples": float(len(np.unique(pop["couple"][pop["couple"] >= 0]))),
        "families with kin": float((hh["kin"][living] >= 0).mean()),
        "landless families": float(1.0 - holders.sum() / living.sum()),
        "plots per holder": float(hh["land"][holders].mean()),
        "land gini": gini(hh["land"][living]),
        "animals": float(hh["animals"].sum()),
        "animals gini": gini(hh["animals"][living]),
        "coins": float(hh["money"].sum()),
        "coins gini": gini(hh["money"][living]),
        "grain": float(hh["grain"].sum()),
        "grain gini": gini(hh["grain"][living]),
        "wage": float(np.mean(state["wage_level"])),
        "town level": float(state["town_level"]),
        "woods": float(np.sum(state["woods"])),
        "treasury": float(np.sum(state["council"]["treasury"])),
        "reserve": float(np.sum(state["council"]["reserve"])),
        "dealers' grain": float(np.sum(state["dealers_grain"])),
        "business cash": float(np.sum(state["cash"])),
        "officials": float(count[jobs == nb].sum()),
        "healers": float(count[jobs == nb + 1].sum()),
    }
    for b, spec in enumerate(config.businesses):
        out[f"share {spec.name}"] = float(count[jobs == b].sum() / max(count[working].sum(), 1.0))
    for p, spec in enumerate(config.products):
        out[f"price {spec.name}"] = float(np.mean(np.asarray(state["prices"])[:, p]))
    return out


RUN_MEASURES = (
    "population at the end", "births", "deaths", "emigrants", "immigrants", "weddings", "grain price", "wage", "ration",
    "poorest fifth's ration", "landless ration", "underfed share", "health", "treasury", "reserve", "food in store",
    "debt at the end", "animals at the end", "land sold", "coins in families' hands", "lowest ration", "highest grain price",
)


def run_measures(rows: list[dict]) -> dict[str, float]:
    """One run's totals and monthly means."""
    def col(name):
        return np.array([float(r[name]) for r in rows])

    pop = col("population")
    return {
        "population at the end": pop[-1],
        "births": col("births").sum(),
        "deaths": col("deaths").sum(),
        "emigrants": col("emigrants").sum(),
        "immigrants": col("immigrants").sum(),
        "weddings": col("weddings").sum(),
        "grain price": col("food_price").mean(),
        "wage": col("wage").mean(),
        "ration": col("ration").mean(),
        "poorest fifth's ration": col("poorest_fifth_ration").mean(),
        "landless ration": col("landless_ration").mean(),
        "underfed share": (col("underfed") / np.maximum(pop, 1)).mean(),
        "health": col("avg_health").mean(),
        "treasury": col("treasury").mean(),
        "reserve": col("food_reserve").mean(),
        "food in store": col("food_stock").mean(),
        "debt at the end": col("debt")[-1],
        "animals at the end": col("animals")[-1],
        "land sold": col("land_sold").sum(),
        "coins in families' hands": col("savings").mean(),
        "lowest ration": col("ration").min(),
        "highest grain price": col("food_price").max(),
        "town people": col("town_people").mean(),
        "town ration": col("town_ration").mean(),
        "grain sent to town": col("grain_exported").sum(),
        "wool coins": col("wool_sold").sum(),
        "grain between towns": col("grain_between_towns").sum(),
    }


def paired_measures(base: list[dict], event: list[dict]) -> dict[str, float]:
    def col(rows, name):
        return np.array([float(r[name]) for r in rows])

    return {
        "extra deaths": (col(event, "deaths") - col(base, "deaths")).sum(),
        "fewer births": (col(base, "births") - col(event, "births")).sum(),
        "population change": col(event, "population")[-1] - col(base, "population")[-1],
        "price peak with": col(event, "food_price").max(),
        "lowest ration with": col(event, "ration").min(),
        "extra emigrants": (col(event, "emigrants") - col(base, "emigrants")).sum(),
    }


def by_run(rows: list[dict]) -> dict[tuple[str, float], list[dict]]:
    runs: dict[tuple[str, float], list[dict]] = {}
    for r in rows:
        runs.setdefault((r["arm"], float(r["seed"])), []).append(r)
    return runs


def measures_of(rows: list[dict], paired: bool) -> list[dict[str, float]]:
    runs = by_run(rows)
    out = []
    for (arm, seed), base in sorted(runs.items()):
        if arm != "base":
            continue
        m = run_measures(base)
        if paired:
            m.update(paired_measures(base, runs[("event", seed)]))
        out.append(m)
    return out


# ---------------------------------------------------------------------------
# Comparing


def welch(a: np.ndarray, b: np.ndarray) -> float:
    """Welch's t of the difference in means; 0 when the means agree to
    rounding (a measure that doesn't vary from run to run, such as a
    starting price, differs only in its last bits)."""
    if np.isclose(a.mean(), b.mean(), rtol=1e-9, atol=1e-12):
        return 0.0
    se = math.sqrt(a.var(ddof=1) / len(a) + b.var(ddof=1) / len(b))
    return float((b.mean() - a.mean()) / se) if se > 0 else math.inf


def compare(python: list[dict[str, float]], rust_: list[dict[str, float]], skip_zero: bool = True) -> list[tuple]:
    rows = []
    for key in python[0]:
        a = np.array([m[key] for m in python], dtype=np.float64)
        b = np.array([m[key] for m in rust_], dtype=np.float64)
        if skip_zero and not a.any() and not b.any():
            continue
        t = welch(a, b)
        rows.append((key, a.mean(), b.mean(), t, abs(t) <= 3.0))
    return rows


def show(title: str, rows: list[tuple], runs: int, markdown: bool) -> int:
    failed = sum(not ok for *_, ok in rows)
    status = "ok" if not failed else f"{failed} FAIL"
    if markdown:
        print(f"\n**{title}** ({runs} runs each): {status}\n")
        print("| Measure | Python | Rust | t |")
        print("|---|---|---|---|")
        for key, a, b, t, ok in rows:
            print(f"| {key} | {a:.4g} | {b:.4g} | {t:+.1f}{'' if ok else ' **fail**'} |")
    else:
        print(f"\n{title} ({runs} runs each): {status}")
        print(f"  {'measure':<30}{'Python':>12}{'Rust':>12}{'t':>8}")
        for key, a, b, t, ok in rows:
            print(f"  {key:<30}{a:>12.4g}{b:>12.4g}{t:>+8.1f}{'' if ok else '  FAIL'}")
    return failed


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("what", choices=["setup", "village", "region"])
    parser.add_argument("--only", default="", help="settings whose name contains this")
    parser.add_argument("--runs", type=int, default=None, help="runs per setting (default: each setting's)")
    parser.add_argument("--markdown", action="store_true", help="tables for PLAN.md")
    args = parser.parse_args()
    chosen = {"setup": setup_settings, "village": village_settings, "region": region_settings}[args.what]()
    chosen = [s for s in chosen if args.only.lower() in s.name.lower()]
    failed = 0
    for setting in chosen:
        if args.runs:
            setting = replace(setting, runs=args.runs)
        if args.what == "setup":
            python = [setup_measures(state, setting.config) for state in python_worlds(setting)]
            rust_ = [setup_measures(json.loads(line), setting.config) for line in rust("world", setting).splitlines()]
            rows = compare(python, rust_, skip_zero=False)
        else:
            python = measures_of(python_records(setting), bool(setting.forced))
            rust_ = measures_of(rust_records(setting), bool(setting.forced))
            rows = compare(python, rust_)
        failed += show(setting.name, rows, setting.runs, args.markdown)
    print(f"\n{'all measures agree' if not failed else f'{failed} measures differ'} (|t| <= 3)")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
