"""How fast is the simulation at the sizes Phase 3 aims for?

    python scripts/benchmark.py                   # up to a million people
    python scripts/benchmark.py --quick           # up to 100,000
    python scripts/benchmark.py --profile 200000  # where the time goes at one size

Each size is a region of villages of 1,000 people (one village below
that). It times setting up the world and simulating a few months, then
projects the Phase 3 targets from them:

  region    ~100,000 people: 100 paired three-year runs in under 10 minutes
  province  ~1,000,000 people: one paired three-year run in under a minute,
            20 of them in under 20 minutes

Paired runs go two at a time or more, one per core (see `--workers`).
"""

from __future__ import annotations

import argparse
import cProfile
import io
import os
import pstats
import resource
import sys
import time
from dataclasses import replace
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from econ_sim.config import Config, VillageConfig  # noqa: E402
from econ_sim.simulation import Simulation  # noqa: E402

SIZES = (1_000, 10_000, 100_000, 1_000_000)
MONTHS_RUN = 36  # a what-if's horizon


def region(people: int, months: int) -> Config:
    villages = max(people // 1000, 1)
    each = people // villages
    return replace(
        Config(seed=1, months=months, start_month=6),
        villages=tuple(VillageConfig(name=f"v{i}", population=each, land=0.35 * each) for i in range(villages)),
    )


def measure(people: int, months: int) -> tuple[float, float]:
    """(seconds to set up, seconds a month)."""
    start = time.perf_counter()
    sim = Simulation(region(people, months))
    setup = time.perf_counter() - start
    start = time.perf_counter()
    sim.run()
    return setup, (time.perf_counter() - start) / months


def paired_runs_minutes(setup: float, per_month: float, pairs: int, workers: int) -> float:
    """Wall-clock minutes for `pairs` paired runs of MONTHS_RUN months on `workers` cores."""
    one = setup + MONTHS_RUN * per_month
    rounds = -(-2 * pairs // workers)  # each pair is two runs
    return rounds * one / 60


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--quick", action="store_true", help="only up to 100,000 people")
    parser.add_argument("--months", type=int, default=12, help="months to time at each size: a year, as months differ (default %(default)s)")
    parser.add_argument("--workers", type=int, default=4, help="cores for the projections (default %(default)s)")
    parser.add_argument("--profile", type=int, metavar="PEOPLE", help="profile one size instead")
    args = parser.parse_args()

    if args.profile:
        sim = Simulation(region(args.profile, args.months))
        profile = cProfile.Profile()
        profile.enable()
        sim.run()
        profile.disable()
        out = io.StringIO()
        pstats.Stats(profile, stream=out).sort_stats("tottime").print_stats(30)
        print("\n".join(line for line in out.getvalue().splitlines() if "/" in line or "ncalls" in line))
        return 0

    print(f"{'People':>10} {'Villages':>9} {'Setup':>8} {'Per month':>10} {'Peak memory':>12}")
    timings = {}
    for people in SIZES:
        if args.quick and people > 100_000:
            break
        setup, per_month = measure(people, args.months)
        timings[people] = setup, per_month
        memory = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / 1024
        print(f"{people:>10,} {max(people // 1000, 1):>9,} {setup:>7.1f}s {per_month:>9.3f}s {memory:>10.0f} MB")

    print(f"\nTargets ({args.workers} cores, runs of {MONTHS_RUN} months):")
    failed = False

    def target(name: str, minutes: float, limit: float) -> None:
        nonlocal failed
        ok = minutes <= limit
        failed |= not ok
        print(f"  {'ok  ' if ok else 'MISS'} {name}: {minutes:.1f} min (target {limit:g})")

    if 100_000 in timings:
        target("region, 100 paired runs of 100,000 people", paired_runs_minutes(*timings[100_000], 100, args.workers), 10)
    if 1_000_000 in timings:
        target("province, 1 paired run of 1,000,000", paired_runs_minutes(*timings[1_000_000], 1, min(args.workers, 2)), 1)
        target("province, 20 paired runs of 1,000,000", paired_runs_minutes(*timings[1_000_000], 20, args.workers), 20)
    print(f"\n(this machine has {os.cpu_count()} cores)")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
