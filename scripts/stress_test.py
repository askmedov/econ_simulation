"""Stress test: run the village through many scenarios for decades and look for drift.

    python scripts/stress_test.py                   # 40 years, 6 seeds per scenario
    python scripts/stress_test.py --full            # 100 years, 16 seeds
    python scripts/stress_test.py --only drought    # scenarios whose name contains "drought"

Each scenario is run for several seeds; every year the village is measured
(population, prices, wages, how families eat, the mix of trades, how money is
spread, food stores, family sizes). The report compares the first years with
the last, and flags drift: a measure that wanders out of a plausible range or
keeps trending. Yearly averages per scenario are written to
output/stress/<scenario>.csv. Exit code 1 if anything drifted.
"""

from __future__ import annotations

import argparse
import csv
import sys
from dataclasses import dataclass, replace
from multiprocessing import Pool
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from econ_sim import households  # noqa: E402
from econ_sim.config import (  # noqa: E402
    Config,
    CouncilConfig,
    FoodConfig,
    HealthcareConfig,
    ScheduledEvent,
    VillageConfig,
)
from econ_sim.simulation import Simulation  # noqa: E402


@dataclass
class Scenario:
    name: str
    config: Config
    note: str = ""
    # Measures this scenario is meant to push out of their usual range:
    # reported, but not counted as drift.
    expected: tuple[str, ...] = ()


def scenarios() -> list[Scenario]:
    base = Config()
    trade, money, needs, demo, food = base.trade, base.money, base.needs, base.demography, base.food

    def every(event: str, first: int, gap: int, count: int) -> tuple[ScheduledEvent, ...]:
        return tuple(ScheduledEvent(event, first + gap * i) for i in range(count))

    def shares(**share: float):
        return tuple(replace(b, initial_share=share.get(b.name, 0.0)) for b in base.businesses)

    out = [
        Scenario("baseline", base),
        Scenario("no random events", replace(base, random_events=False)),
        Scenario("start in October", replace(base, start_month=10)),
        Scenario("village of 300", replace(base, villages=(VillageConfig(population=300, land=105),)), "too small for a council"),
        Scenario("village of 3000", replace(base, villages=(VillageConfig(population=3000, land=1050),))),
        # Repeated shocks
        Scenario("drought 3 years running", replace(base, scheduled_events=every("drought", 4, 12, 3))),
        Scenario("drought every other year", replace(base, scheduled_events=every("drought", 4, 24, 20))),
        Scenario("epidemic every 5 years", replace(base, scheduled_events=every("disease", 6, 60, 10))),
        Scenario("5 harsh winters in a row", replace(base, scheduled_events=every("harsh_winter", 12, 12, 5))),
        Scenario("forest fires 3 years running", replace(base, scheduled_events=every("forest_fire", 7, 12, 3))),
        Scenario(
            "everything in year 2",
            replace(base, scheduled_events=(
                ScheduledEvent("drought", 16), ScheduledEvent("disease", 18), ScheduledEvent("forest_fire", 19),
                ScheduledEvent("granary_fire", 22), ScheduledEvent("harsh_winter", 24),
            )),
        ),
        # Policies
        Scenario("no council", replace(base, council=CouncilConfig(enabled=False))),
        Scenario("no relief", replace(base, council=CouncilConfig(relief=False))),
        Scenario("no healthcare", replace(base, healthcare=HealthcareConfig(enabled=False))),
        Scenario(
            "no neighbourly help",
            replace(base, money=replace(money, sharing_rate=0.0)),
            "large poor families can't afford firewood",
            ("warmth",),
        ),
        Scenario(
            "no council, no help",
            replace(base, council=CouncilConfig(enabled=False), money=replace(money, sharing_rate=0.0)),
            "no safety net at all: families with many children and few earners go hungry",
            ("poorest_fifth", "deaths_per_1000", "population", "share_farming", "landless_share"),
        ),
        Scenario("tax 0%", replace(base, council=CouncilConfig(tax_rate=0.0))),
        Scenario("tax 30%", replace(base, council=CouncilConfig(tax_rate=0.3))),
        Scenario("12-month reserve", replace(base, council=CouncilConfig(reserve_months=12.0))),
        Scenario("grain levy 20%", replace(base, council=CouncilConfig(grain_levy=0.2))),
        Scenario("council forms later", replace(base, council=CouncilConfig(established_at_start=False))),
        # Starting conditions
        Scenario("little money", replace(base, money=replace(money, initial_savings_months=0.5))),
        Scenario("lots of money", replace(base, money=replace(money, initial_savings_months=24.0))),
        Scenario("1 month of food", replace(base, villages=(VillageConfig(initial_food_months=1.0),))),
        Scenario(
            "crowded land", replace(base, villages=(VillageConfig(land=250),)),
            "few can live off their land", ("births_per_1000", "landless_share", "money_gini", "money_top10_share"),
        ),
        Scenario("plenty of land", replace(base, villages=(VillageConfig(land=700),)), "grows into the land", ("population",)),
        Scenario("95% farmers at start", replace(base, businesses=shares(farming=0.95, woodcutting=0.03, weaving=0.01, smithing=0.01))),
        Scenario("half farmers at start", replace(base, businesses=shares(farming=0.5, woodcutting=0.15, weaving=0.3, smithing=0.05))),
        Scenario(
            "three villages",
            replace(base, villages=(VillageConfig(name="A"), VillageConfig(name="B", land=250), VillageConfig(name="C", population=400, land=140))),
        ),
    ]
    # One setting at a time, halved and doubled (or a plausible range).
    perturb = [
        ("hiring rate", lambda v: replace(base, trade=replace(trade, hiring_rate=v)), (0.02, 0.15)),
        ("mark-up speed", lambda v: replace(base, trade=replace(trade, markup_speed=v)), (0.25, 1.0)),
        ("mark-up step", lambda v: replace(base, trade=replace(trade, max_markup_change=v)), (0.05, 0.2)),
        ("grain price elasticity", lambda v: replace(base, food=replace(food, price_elasticity=v)), (1.5, 4.0)),
        ("spoilage", lambda v: replace(base, food=replace(food, spoilage=v)), (0.01, 0.04)),
        ("tool wear", lambda v: replace(base, trade=replace(trade, tool_wear=v)), (0.015, 0.06)),
        ("stock target", lambda v: replace(base, trade=replace(trade, stock_target_months=v)), (2.0, 8.0)),
        ("birth chance", lambda v: replace(base, demography=replace(demo, annual_birth_chance=v)), (0.22, 0.34)),
        ("marriage chance", lambda v: replace(base, demography=replace(demo, marriage_chance=v)), (1 / 72, 1 / 30)),
        ("spare spending", lambda v: replace(base, needs=replace(needs, spare_spending=v)), (0.1, 0.6)),
        ("savings kept", lambda v: replace(base, needs=replace(needs, savings_months=v)), (1.0, 6.0)),
        ("wage payout", lambda v: replace(base, money=replace(money, wage_payout=v)), (0.7, 1.0)),
        ("farm reserve margin", lambda v: replace(base, food=replace(food, reserve_margin=v)), (0.0, 0.25)),
    ]
    for label, make, values in perturb:
        for value in values:
            out.append(Scenario(f"{label} {value:g}", make(value)))
    return out


def gini_and_top(money: np.ndarray, people: np.ndarray) -> tuple[float, float, float]:
    """Gini of money per person (people-weighted), top-10% share, bottom-half share."""
    alive = people > 0
    money, people = np.maximum(money[alive], 0.0), people[alive].astype(np.float64)
    if money.sum() <= 0:
        return 0.0, 0.0, 0.0
    order = np.argsort(money / people, kind="stable")
    money, people = money[order], people[order]
    cum_people = np.cumsum(people) / people.sum()
    cum_money = np.cumsum(money) / money.sum()
    lorenz = np.concatenate([[0.0], cum_money])
    gini = 1.0 - np.sum(np.diff(np.concatenate([[0.0], cum_people])) * (lorenz[1:] + lorenz[:-1]))
    top = 1.0 - np.interp(0.9, cum_people, cum_money)
    bottom = np.interp(0.5, cum_people, cum_money)
    return float(gini), float(top), float(bottom)


def run_one(job: tuple[Scenario, int, int]) -> dict[str, np.ndarray]:
    """Run one seed; return yearly measures."""
    scenario, seed, years = job
    sim = Simulation(replace(scenario.config, seed=seed, months=years * 12))
    rows: dict[str, list[float]] = {}
    names = [b.name for b in sim.config.businesses]

    def add(key: str, value: float) -> None:
        rows.setdefault(key, []).append(float(value))

    for _ in range(years):
        year = [sim.step() for _ in range(12)]
        w = sim.world
        workers = sum(sum(r.jobs.values()) for r in year) / 12 or 1.0
        add("population", year[-1].population)
        add("births_per_1000", 1000 * sum(r.births for r in year) / max(year[0].population, 1))
        add("deaths_per_1000", 1000 * sum(r.deaths for r in year) / max(year[0].population, 1))
        add("weddings_per_1000", 1000 * sum(r.weddings for r in year) / max(year[0].population, 1))
        add("sown", min(r.sown for r in year))
        add("animals_per_plot", year[-1].animals / max(sim.world.land.sum(), 1e-9))
        add("wage_cover", np.mean([r.wage_cover for r in year]))
        add("food_price", np.mean([r.food_price for r in year]))
        for product in year[0].prices:
            add(f"price_{product}", np.mean([r.prices[product] for r in year]))
        add("wage", np.mean([r.wage for r in year]))
        # Prices in days of work: money's own level moves with how much there is.
        wage = max(rows["wage"][-1], 1e-9)
        add("real_food_price", rows["food_price"][-1] / wage)
        for product in year[0].prices:
            add(f"real_price_{product}", rows[f"price_{product}"][-1] / wage)
        add("ration", np.mean([r.ration for r in year]))
        add("poorest_fifth", np.mean([r.poorest_fifth_ration for r in year]))
        add("poorest_fifth_worst", min(r.poorest_fifth_ration for r in year))
        add("warmth", np.mean([r.warmth for r in year]))
        add("health", np.mean([r.avg_health for r in year]))
        add("clothing_per_person_year", sum(r.clothing for r in year))
        for name in names:
            add(f"share_{name}", sum(r.jobs[name] for r in year) / 12 / workers)
        add("food_store_months", np.mean([r.food_stock / max(r.food_needed, 1e-9) for r in year]))
        add("food_cover", np.mean([r.food_cover for r in year]))
        total_money = w.money
        add("treasury_share", w.council.treasury.sum() / total_money)
        add("business_cash_share", w.cash.sum() / total_money)
        size = households.sizes(w.population, len(w.households))
        gini, top, bottom = gini_and_top(w.households.money, size)
        add("land_gini", gini_and_top(w.households.land, size)[0])
        wealth = w.households.money + w.households.grain * w.food_price[w.households.location]
        add("wealth_gini", gini_and_top(wealth, size)[0])
        add("landless_share", np.mean([r.landless / max(r.population, 1) for r in year]))
        add("own_food_share", np.mean([r.own_food / max(r.food_eaten, 1e-9) for r in year]))
        add("money_gini", gini)
        add("money_top10_share", top)
        add("money_bottom_half_share", bottom)
        alive = size > 0
        add("household_size", size[alive].mean() if alive.any() else 0.0)
        add("household_size_max", size.max() if len(size) else 0.0)
        add("households", alive.sum())
        add("reserve_months", w.council.reserve.sum() / max(year[-1].food_needed, 1e-9))
    return {k: np.array(v) for k, v in rows.items()}


# (measure, lowest plausible, highest plausible) for the average of the last five years
RANGES = {
    "ration": (0.9, 1.01),
    "poorest_fifth": (0.85, 1.01),
    "warmth": (0.85, 1.01),
    "health": (75.0, 100.0),
    "share_farming": (0.55, 0.92),
    "share_woodcutting": (0.02, 0.25),
    "share_smithing": (0.005, 0.15),
    "treasury_share": (0.0, 0.25),
    "business_cash_share": (0.0, 0.3),
    # Coins pool with families who sell grain; labourers hold few. Land
    # concentrates through debt as the village grows: a land Gini of 0.8-0.9
    # and a majority landless are within what crowded old villages showed.
    "money_top10_share": (0.0, 0.85),
    "money_gini": (0.0, 0.9),
    "wealth_gini": (0.0, 0.85),
    "land_gini": (0.0, 0.9),
    "landless_share": (0.0, 0.75),
    "food_store_months": (0.5, 12.0),
    "household_size": (3.0, 9.0),
    "household_size_max": (0.0, 30.0),
    # A high-pressure pre-industrial village: early marriage, many births,
    # many deaths.
    "births_per_1000": (30.0, 60.0),
    "deaths_per_1000": (25.0, 60.0),
    "weddings_per_1000": (5.0, 20.0),
    "sown": (0.85, 1.01),
    "animals_per_plot": (0.1, 0.5),
    "wage_cover": (0.65, 2.5),
}
# Measures that should stay within these multiples of their first-year value.
RELATIVE = {
    "population": (0.6, 1.6),
    "real_food_price": (0.4, 2.5),
    "real_price_firewood": (0.33, 3.0),
    "real_price_clothing": (0.33, 3.0),
    "real_price_tools": (0.33, 3.0),
}
# Measures whose trend over the second half shouldn't exceed this share per
# decade. Nominal prices and wages may trend (with money and population);
# prices in days of work shouldn't keep running away.
TRENDS = {
    "population": 0.15, "real_food_price": 0.3, "wage_cover": 0.3, "household_size": 0.15,
    "land_gini": 0.15, "landless_share": 0.3,
}


def drift_flags(mean: dict[str, np.ndarray], reference: dict[str, np.ndarray] | None = None) -> list[tuple[str, str]]:
    """(measure, message) for each measure that drifted. Real prices are
    compared with `reference`'s first year (the baseline village, as a
    scenario's own first year may be a famine); other measures with their own."""
    flags = []
    years = len(mean["population"])
    last = slice(max(years - 5, 0), years)
    for key, (low, high) in RANGES.items():
        value = mean[key][last].mean()
        if not low <= value <= high:
            flags.append((key, f"{key} {value:.2f} outside [{low:g}, {high:g}]"))
    for key, (low, high) in RELATIVE.items():
        first = reference if reference is not None and key.startswith("real_") else mean
        start, value = first[key][0], mean[key][last].mean()
        ratio = value / start if start else np.inf
        if not low <= ratio <= high:
            flags.append((key, f"{key} x{ratio:.2f} from year 1 ({start:.2f} -> {value:.2f})"))
    for key, limit in TRENDS.items():
        half = mean[key][years // 2 :]
        if len(half) >= 6 and half.mean() > 0:
            slope = np.polyfit(np.arange(len(half)), np.log(np.maximum(half, 1e-9)), 1)[0] * 10
            if abs(slope) > limit:
                flags.append((key, f"{key} still trending {slope:+.0%} a decade"))
    return flags


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--full", action="store_true", help="100 years, 16 seeds")
    parser.add_argument("--years", type=int, default=None)
    parser.add_argument("--seeds", type=int, default=None)
    parser.add_argument("--only", default="", help="run only scenarios whose name contains this")
    parser.add_argument("--out", default="output/stress")
    args = parser.parse_args()
    years = args.years or (100 if args.full else 40)
    seeds = args.seeds or (16 if args.full else 6)
    chosen = [s for s in scenarios() if args.only.lower() in s.name.lower()]
    jobs = [(s, seed, years) for s in chosen for seed in range(seeds)]
    with Pool() as pool:
        results = pool.map(run_one, jobs, chunksize=1)

    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    drifted = 0
    print(f"{len(chosen)} scenarios x {seeds} seeds x {years} years\n")
    header = (
        f"{'scenario':<30} {'pop':>11} {'food price':>11} {'wage':>11} {'ration':>6} {'poor5':>6} {'warm':>5} "
        f"{'b/1k':>4} {'d/1k':>4} {'farm':>5} {'weave':>5} {'gini':>5} {'top10':>5} {'treas':>5} {'hh':>4}"
    )
    print(header)
    means = [{k: np.mean([r[k] for r in results[i * seeds : (i + 1) * seeds]], axis=0) for k in results[0]} for i in range(len(chosen))]
    reference = next((m for s, m in zip(chosen, means) if s.name == "baseline"), None)
    for scenario, mean in zip(chosen, means):
        with (out / f"{scenario.name.replace(' ', '_').replace('%', 'pct').replace(',', '')}.csv").open("w", newline="") as f:
            writer = csv.writer(f)
            writer.writerow(["year", *mean])
            for y in range(years):
                writer.writerow([y + 1, *(f"{mean[k][y]:.4g}" for k in mean)])
        last = slice(years - 5, years)
        m = lambda k: mean[k][last].mean()
        print(
            f"{scenario.name:<30} {mean['population'][0]:>5.0f}>{m('population'):<5.0f} "
            f"{mean['food_price'][0]:>5.2f}>{m('food_price'):<5.2f} {mean['wage'][0]:>5.2f}>{m('wage'):<5.2f} "
            f"{m('ration'):>6.2f} {m('poorest_fifth'):>6.2f} {m('warmth'):>5.2f} "
            f"{m('births_per_1000'):>4.0f} {m('deaths_per_1000'):>4.0f} {m('share_farming'):>5.2f} {m('share_weaving'):>5.2f} "
            f"{m('money_gini'):>5.2f} {m('money_top10_share'):>5.2f} {m('treasury_share'):>5.2f} {m('household_size'):>4.1f}"
        )
        flags = drift_flags(mean, reference)
        surprises = [message for key, message in flags if key not in scenario.expected]
        for key, message in flags:
            if key in scenario.expected:
                print(f"{'':<4}expected ({scenario.note}): {message}")
        if surprises:
            drifted += 1
            for message in surprises:
                print(f"{'':<4}DRIFT {message}")
    print(f"\n{drifted} of {len(chosen)} scenarios drifted; yearly averages in {out}/")
    return 1 if drifted else 0


if __name__ == "__main__":
    sys.exit(main())
