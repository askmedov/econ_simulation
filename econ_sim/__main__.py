"""Command line: python -m econ_sim --help"""

from __future__ import annotations

import argparse
import calendar
from dataclasses import replace
from pathlib import Path

import numpy as np

from econ_sim import geography
from econ_sim.config import Config, ScheduledEvent, VillageConfig
from econ_sim.metrics import write_csv
from econ_sim.scenarios import (
    Comparison, Effect, Run, compare, effect, effect_by, run_batch, series, spread, write_places_csv, write_summary_csv,
)
from econ_sim.simulation import Simulation


def main(argv: list[str] | None = None) -> None:
    args = _parser().parse_args(argv)
    defaults = Config()
    if args.list_events:
        _print_events(defaults)
        return

    standard = VillageConfig()
    village = VillageConfig(
        population=args.population or standard.population,
        land=args.land if args.land is not None else standard.land,
        initial_food_months=args.food_months,
    )
    config = replace(
        defaults,
        seed=args.seed,
        months=args.months,
        start_month=args.start_month,
        villages=(village,),
        random_events=not args.no_random_events,
        council=replace(
            defaults.council,
            enabled=not args.no_council,
            relief=not args.no_relief,
            established_at_start=not args.council_forms_later,
        ),
        healthcare=replace(defaults.healthcare, enabled=not args.no_healthcare),
        lord=replace(defaults.lord, enabled=not args.no_lord, charity_in_famine=args.lord_charity),
        state=replace(defaults.state, enabled=not args.no_state_tax, remit_in_famine=args.remit_tax),
        town=replace(defaults.town, enabled=not args.no_town),
        migration=replace(defaults.migration, enabled=not args.no_migration),
        work=replace(defaults.work, kin_share=0.0 if args.no_kin_help else defaults.work.kin_share),
        events=tuple(replace(e, repeat_chance=None) for e in defaults.events) if args.no_drought_runs else defaults.events,
    )
    if args.region:
        if args.land is not None:
            raise SystemExit("--land sets one village's land; in a region each village has land for its size")
        settings = geography.RegionSettings(
            villages=args.region, towns=args.towns, seed=args.map_seed,
            mean_size=args.population or geography.RegionSettings.mean_size,
        )
        # Villages get land at the usual density for their size.
        usual = replace(standard, initial_food_months=args.food_months)
        config = geography.region(settings, replace(config, villages=(usual,)))
    forced = tuple(_parse_event(text) for text in args.event)
    # Check the forced events before running anything.
    Simulation(replace(config, months=0, scheduled_events=forced))

    _print_header(config, forced, args.runs)
    if forced:
        pairs = compare(config, forced, args.runs, args.workers)
        baseline, scenario = pairs.baseline, pairs.scenario
    else:
        baseline, scenario = run_batch(config, args.runs, args.workers), None

    if scenario is None:
        _print_months(baseline)
    else:
        _print_comparison(baseline, scenario)
    shown = (scenario or baseline)[0]
    if args.runs == 1 and shown.log:
        print("\nWhat happened:")
        for line in shown.log:
            print(f"  {line}")
    if scenario is None:
        _print_outlook(baseline)
    else:
        _print_effect(effect(Comparison(baseline, scenario)), forced, args.runs, config.months)
        if config.map.enabled:
            _print_places(Comparison(baseline, scenario), config, forced)

    out = Path(args.out)
    write_csv(baseline[0].records, out / "run1.csv")
    log = ["Without forced events:", *baseline[0].log]
    if scenario is not None:
        write_csv(scenario[0].records, out / "run1_with_event.csv")
        log += ["", "With forced events:", *scenario[0].log]
    write_summary_csv(out / "summary.csv", baseline, scenario)
    (out / "log.txt").write_text("\n".join(log) + "\n")
    written = ["run1.csv", "summary.csv", "log.txt"]
    if scenario is not None:
        written.insert(1, "run1_with_event.csv")
    if config.map.enabled:
        geo = geography.build(config)
        about = {
            "terrain": np.array([v.terrain for v in config.villages]), "x_km": geo.x, "y_km": geo.y,
            "carriage": geo.transport, "population_start": np.array([v.population for v in config.villages]),
        }
        write_places_csv(out / "places.csv", tuple(v.name for v in config.villages), about, baseline, scenario)
        written.append("places.csv")
    if args.plot:
        from econ_sim.charts import plot

        plot(out / "overview.png", baseline, scenario, forced)
        written.append("overview.png")
    print(f"\nWrote {', '.join(written)} to {out}/")


def _parser() -> argparse.ArgumentParser:
    defaults, village = Config(), VillageConfig()
    parser = argparse.ArgumentParser(
        prog="python -m econ_sim",
        description="Simulate a village economy month by month, and see what an event does to the next few years.",
        epilog="Example: python -m econ_sim --event drought@4 --runs 200 --plot",
    )
    parser.add_argument("--months", type=int, default=defaults.months, help="how many months to simulate (default %(default)s)")
    parser.add_argument("--runs", type=int, default=1, help="repeat with this many seeds and average (default %(default)s)")
    parser.add_argument("--seed", type=int, default=defaults.seed, help="random seed of the first run (default %(default)s)")
    parser.add_argument("--start-month", type=int, default=defaults.start_month, help="calendar month to start in, 1-12 (default %(default)s)")
    parser.add_argument(
        "--event", action="append", default=[], metavar="NAME@MONTH[@X,Y,KM]",
        help="force an event, e.g. drought@4; in a region, drought@4@20,10,8 strikes the villages within 8 km of "
        "(20, 10) km on the map; repeatable",
    )
    parser.add_argument("--no-random-events", action="store_true", help="only the forced events happen")
    parser.add_argument("--no-council", action="store_true", help="the village never has a council (no taxes, reserve or relief)")
    parser.add_argument("--no-relief", action="store_true", help="the council gives no famine relief")
    parser.add_argument("--no-healthcare", action="store_true", help="the council employs no healers")
    parser.add_argument(
        "--council-forms-later", action="store_true",
        help="no council at the start; one forms once the village has had 500 people for 6 months",
    )
    parser.add_argument("--no-lord", action="store_true", help="no lord: no demesne, labour services or barn")
    parser.add_argument("--lord-charity", action="store_true", help="the lord opens his barn to the hungry in a famine")
    parser.add_argument("--no-state-tax", action="store_true", help="the state takes no tax")
    parser.add_argument("--remit-tax", action="store_true", help="the state remits its tax in a famine year")
    parser.add_argument("--no-town", action="store_true", help="no merchants: grain neither leaves for nor comes from the town")
    parser.add_argument("--no-migration", action="store_true", help="nobody leaves the village or comes to it")
    parser.add_argument("--no-kin-help", action="store_true", help="kin don't give each other grain")
    parser.add_argument(
        "--no-drought-runs", action="store_true",
        help="a drought (or good year) doesn't make another the next year likelier: isolates a single forced drought",
    )
    parser.add_argument(
        "--population", type=int, default=None,
        help=f"villagers at the start (default {village.population}; in a region, people in an average village of "
        f"the plain, default {geography.RegionSettings.mean_size:g})",
    )
    parser.add_argument("--land", type=float, default=None, help=f"farmland in plots (default {village.land:g})")
    parser.add_argument(
        "--region", type=int, default=0, metavar="VILLAGES",
        help="a region of this many villages on a map, with hills, woods, a river, market towns and roads",
    )
    parser.add_argument("--towns", type=int, default=None, help="market towns in the region (default: one for every 25 villages)")
    parser.add_argument("--map-seed", type=int, default=1, help="lay out a different region (default %(default)s)")
    parser.add_argument("--food-months", type=float, default=village.initial_food_months, help="months of food in store at the start (default %(default)s)")
    parser.add_argument(
        "--workers", type=int, default=None,
        help="cores to run the runs on (default: all of them, or ECON_SIM_WORKERS)",
    )
    parser.add_argument("--out", default="output", help="folder for the CSV files and chart (default %(default)s)")
    parser.add_argument("--plot", action="store_true", help="also draw overview.png (needs matplotlib)")
    parser.add_argument("--list-events", action="store_true", help="show the events that can happen, then exit")
    return parser


def _parse_event(text: str) -> ScheduledEvent:
    name, sep, rest = text.partition("@")
    month, _, where = rest.partition("@")
    if not sep or not month.isdigit():
        raise SystemExit(f"--event {text!r}: expected NAME@MONTH, e.g. drought@4, or NAME@MONTH@X,Y,KM")
    near = None
    if where:
        try:
            x, y, km = (float(part) for part in where.split(","))
        except ValueError:
            raise SystemExit(f"--event {text!r}: expected a place as X,Y,KM, e.g. drought@4@20,10,8") from None
        near = (x, y, km)
    return ScheduledEvent(event=name.strip().replace(" ", "_"), month=int(month), near=near)


def _label(record) -> str:
    return f"{record.month_number:>3}  {calendar.month_abbr[record.month]} Y{record.year}"


def _print_header(config: Config, forced: tuple[ScheduledEvent, ...], runs: int) -> None:
    v = config.villages[0]
    if config.map.enabled:
        print(geography.describe(config, geography.build(config)))
    else:
        print(f"Village of {v.population} people, {v.land:g} plots of land, {v.initial_food_months:g} months of food in store")
    when = f"{calendar.month_name[config.start_month]} of year 1"
    randomness = "random events on" if config.random_events else "no random events"
    print(f"{config.months} months from {when}; seed {config.seed}; {runs} run{'s' * (runs != 1)}; {randomness}")
    cc = config.council
    if not cc.enabled:
        print("No council")
    elif len(config.villages) > 1:
        big = sum(village.population >= cc.forms_at_population for village in config.villages)
        start = f"{big} start with one" if cc.established_at_start else "none at the start"
        print(f"Councils in villages of {cc.forms_at_population} people or more ({start})")
    elif v.population >= cc.forms_at_population and cc.established_at_start:
        care = f"{config.healthcare.healers_per_1000:g} healers per 1,000" if config.healthcare.enabled else "no healers"
        print(
            f"Council: {cc.tax_rate:.0%} tax, {cc.reserve_months:g}-month food reserve, "
            f"relief {'on' if cc.relief else 'off'}, {care}"
        )
    else:
        print(f"A council forms once the village has {cc.forms_at_population} people for {cc.forms_after_months} months")
    if forced:
        print("Forced: " + ", ".join(_forced_label(e) for e in forced))
    print()


def _forced_label(event: ScheduledEvent) -> str:
    where = f" within {event.near[2]:g} km of ({event.near[0]:g}, {event.near[1]:g})" if event.near else ""
    return f"{event.event.replace('_', ' ')} in month {event.month}{where}"


def _print_months(sims: list[Run]) -> None:
    if len(sims) == 1:
        print(
            f"{'Month':<12} {'Pop':>5} {'Born':>5} {'Died':>5} {'Stock':>7} {'Price':>6} {'Wage':>6} "
            f"{'Ration':>7} {'Poorest':>8} {'Health':>7}  Events"
        )
        for r in sims[0].records:
            print(
                f"{_label(r):<12} {r.population:>5} {r.births:>5} {r.deaths:>5} {r.food_stock:>7,.0f} "
                f"{r.food_price:>6.2f} {r.wage:>6.2f} {r.ration:>7.0%} {r.poorest_fifth_ration:>8.0%} "
                f"{r.avg_health:>7.0f}  {r.events.replace('_', ' ').replace('+', ', ')}"
            )
        return
    # Many runs: averages, with the range 80% of runs fall into.
    print("Averages across runs (80% of runs fall in the range in brackets)")
    print(f"{'Month':<12} {'Pop':>18} {'Died so far':>16} {'Food price':>17} {'Ration':>17} {'Poorest fifth':>17} {'Health':>7}")
    pop, deaths = spread(series(sims, "population")), spread(series(sims, "deaths").cumsum(axis=1))
    price, ration = spread(series(sims, "food_price")), spread(series(sims, "ration"))
    poorest, health = spread(series(sims, "poorest_fifth_ration")), spread(series(sims, "avg_health"))
    for i, r in enumerate(sims[0].records):
        print(
            f"{_label(r):<12} {_range(pop, i, '{:.0f}'):>18} {_range(deaths, i, '{:.1f}'):>16} "
            f"{_range(price, i, '{:.2f}'):>17} {_range(ration, i, '{:.0%}'):>17} {_range(poorest, i, '{:.0%}'):>17} "
            f"{health.mean[i]:>7.0f}"
        )


def _range(s, i: int, fmt: str) -> str:
    return f"{fmt.format(s.mean[i])} ({fmt.format(s.low[i])}-{fmt.format(s.high[i])})"


def _print_comparison(baseline: list[Run], scenario: list[Run]) -> None:
    averaged = len(baseline) > 1
    if averaged:
        print(f"Averages of {len(baseline)} paired runs")
    print(f"{'':<12} {'Food in store':>15} {'Food price':>15} {'Ration':>15} {'Health':>13} {'Extra':>8} {'Fewer':>8}")
    print(
        f"{'Month':<12} {'without':>8}{'with':>7} {'without':>8}{'with':>7} {'without':>8}{'with':>7} "
        f"{'without':>8}{'with':>5} {'deaths':>8} {'births':>8}"
    )
    mean = lambda sims, metric: series(sims, metric).mean(axis=0)
    stock = mean(baseline, "food_stock"), mean(scenario, "food_stock")
    price = mean(baseline, "food_price"), mean(scenario, "food_price")
    ration = mean(baseline, "ration"), mean(scenario, "ration")
    health = mean(baseline, "avg_health"), mean(scenario, "avg_health")
    extra_deaths = (series(scenario, "deaths") - series(baseline, "deaths")).cumsum(axis=1).mean(axis=0)
    fewer_births = (series(baseline, "births") - series(scenario, "births")).cumsum(axis=1).mean(axis=0)
    count = "{:>8.1f}" if averaged else "{:>8.0f}"
    for i, r in enumerate(baseline[0].records):
        print(
            f"{_label(r):<12} {stock[0][i]:>8,.0f}{stock[1][i]:>7,.0f} {price[0][i]:>8.2f}{price[1][i]:>7.2f} "
            f"{ration[0][i]:>8.0%}{ration[1][i]:>7.0%} "
            f"{health[0][i]:>8.0f}{health[1][i]:>5.0f} {count.format(extra_deaths[i])} {count.format(fewer_births[i])}"
        )


def _print_effect(result: Effect, forced: tuple[ScheduledEvent, ...], runs: int, months: int) -> None:
    from econ_sim.charts import event_label

    print(f"\nEffect of {event_label(forced)} over {months} months", end="")
    print(" (one run):" if runs == 1 else f" (average of {runs} runs; 80% of runs fall in the range in brackets):")

    def number(value: float, sign: str, decimals: int) -> str:
        value = round(float(value), decimals) + 0.0  # + 0.0 turns -0.0 into 0.0
        return f"{value:{sign}.{decimals}f}" if value else "0"

    def show(name: str, s, sign: str = "") -> None:
        text = number(s.mean, sign, 1 if runs > 1 else 0)
        if runs > 1:
            text += f"  ({number(s.low, sign, 0)} to {number(s.high, sign, 0)})"
        print(f"  {name:<28}{text}")

    show("Extra deaths", result.extra_deaths, "+")
    show("Fewer births", result.fewer_births)
    show("Population at the end", result.population_change, "+")
    print(f"  {'Highest food price':<28}{result.highest_price[0]:.2f} without, {result.highest_price[1]:.2f} with")
    print(f"  {'Lowest ration':<28}{result.lowest_ration[0]:.0%} without, {result.lowest_ration[1]:.0%} with")
    print(
        f"  {'Poorest fifth, worst month':<28}{result.poorest_lowest_ration[0]:.0%} without, "
        f"{result.poorest_lowest_ration[1]:.0%} with"
    )
    print(
        f"  {'Landless, worst month':<28}{result.landless_lowest_ration[0]:.0%} without, "
        f"{result.landless_lowest_ration[1]:.0%} with"
    )
    months_fmt = ".1f" if runs > 1 else ".0f"
    print(
        f"  {'Months on short rations':<28}{result.hungry_months[0]:{months_fmt}} without, "
        f"{result.hungry_months[1]:{months_fmt}} with"
    )
    print(f"  {'Lowest average health':<28}{result.lowest_health[0]:.0f} without, {result.lowest_health[1]:.0f} with")
    print(f"  {'People who left':<28}{result.left[0]:.0f} without, {result.left[1]:.0f} with")
    if result.relief[1] > 0 or result.relief[0] > 0:
        print(f"  {'Council relief (rations)':<28}{result.relief[0]:.0f} without, {result.relief[1]:.0f} with")
    if result.lord_relief[1] > 0 or result.lord_relief[0] > 0:
        print(f"  {'Lord relief (rations)':<28}{result.lord_relief[0]:.0f} without, {result.lord_relief[1]:.0f} with")
    if result.treated[1] > 0 or result.treated[0] > 0:
        print(f"  {'People treated by healers':<28}{result.treated[0]:.0f} without, {result.treated[1]:.0f} with")


def _print_places(comparison: Comparison, config: Config, forced: tuple[ScheduledEvent, ...]) -> None:
    """The effect where the forced events struck and elsewhere, by terrain
    and by distance from market."""
    geo = geography.build(config)
    terrains = [t.name for t in config.map.terrains]
    cost = np.where(np.isfinite(geo.transport), geo.transport, 9.0)
    cuts = np.quantile(cost, [1 / 3, 2 / 3])
    thirds = np.searchsorted(cuts, cost, side="right")
    by_cost = [f"nearest third (under {cuts[0]:.0%})", "middle third", f"farthest third (over {cuts[1]:.0%})"]
    tables = [("terrain", geo.terrain, terrains), ("cost of carting grain to market", thirds, by_cost)]
    struck = np.zeros(len(config.villages), dtype=bool)
    for event in forced:
        if event.near is not None:
            struck |= geo.near(event.near)
        elif event.village is not None:
            struck[event.village] = True
        else:
            struck[:] = True
    if not struck.all():
        tables.insert(0, ("where the forced events struck", (~struck).astype(int), ["struck", "elsewhere"]))
    runs = len(comparison.baseline)
    for title, groups, labels in tables:
        print(f"\nBy {title}:")
        print(f"  {'':<34}{'Villages':>9}{'People':>9}{'Extra deaths':>14}{'per 1,000':>10}{'Worst month':>17}{'Left':>13}")
        for e in effect_by(comparison, groups, labels):
            per_1000 = 1000 * e.extra_deaths.mean / max(e.people, 1)
            deaths = f"{e.extra_deaths.mean:+.1f}" if runs > 1 else f"{e.extra_deaths.mean:+.0f}"
            worst = f"{e.lowest_ration[0]:.0%} -> {e.lowest_ration[1]:.0%}"
            left = f"{e.left[0]:.0f} -> {e.left[1]:.0f}"
            print(f"  {e.label:<34}{e.villages:>9}{e.people:>9,.0f}{deaths:>14}{per_1000:>10.1f}{worst:>17}{left:>13}")
    print("  (worst month: the share of their need its villages ate in their hungriest month, without -> with)")


def _print_outlook(sims: list[Run]) -> None:
    pop = series(sims, "population")
    deaths, births = series(sims, "deaths").sum(axis=1), series(sims, "births").sum(axis=1)
    hungry = (series(sims, "ration") < 0.97).sum(axis=1)
    start = sims[0].records[0].population - sims[0].records[0].births + sims[0].records[0].deaths
    months = pop.shape[1]
    print(f"\nAfter {months} months" + ("" if len(sims) == 1 else f" (average of {len(sims)} runs)") + ":")
    print(f"  Population {start} -> {pop[:, -1].mean():.0f}; births {births.mean():.0f}, deaths {deaths.mean():.0f}")
    print(f"  Months the village ate under 97% of its food need: {hungry.mean():.1f}")


def _print_events(config: Config) -> None:
    print(f"{'Event':<14} {'Scope':<9} {'Reach':<8} {'Chance':>8}  {'When':<14} {'Lasts':<8} Effects")
    for spec in config.events:
        when = ", ".join(calendar.month_abbr[m] for m in spec.months) if spec.months else "any month"
        lasts = "-".join(map(str, spec.duration)) if isinstance(spec.duration, tuple) else str(spec.duration)
        effects = ", ".join(f"{k} {v}" for k, v in spec.effects.items())
        if spec.repeat_chance is not None:
            effects += f"; {spec.repeat_chance:.0%} the year after one"
        reach = spec.reach if spec.scope == "location" else ""
        print(f"{spec.name:<14} {spec.scope:<9} {reach:<8} {spec.chance:>8.1%}  {when:<14} {lasts + ' mo':<8} {effects}")
    print("\nChance: per eligible month, in the long run.")
    print("Reach, in a region (--region): a village on its own, the weather (shared by neighbours), or the whole region.")
    print("Force a village event with --event NAME@MONTH, e.g. --event drought@4")


if __name__ == "__main__":
    main()
