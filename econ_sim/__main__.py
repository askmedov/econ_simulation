"""Command line: python -m econ_sim --help"""

from __future__ import annotations

import argparse
import calendar
from dataclasses import replace
from pathlib import Path

import numpy as np

from econ_sim.config import Config, ScheduledEvent, VillageConfig
from econ_sim.metrics import write_csv
from econ_sim.scenarios import Comparison, Effect, effect, run_batch, series, spread, write_summary_csv
from econ_sim.simulation import Simulation


def main(argv: list[str] | None = None) -> None:
    args = _parser().parse_args(argv)
    defaults = Config()
    if args.list_events:
        _print_events(defaults)
        return

    village = VillageConfig(population=args.population, land=args.land, initial_food_months=args.food_months)
    config = replace(
        defaults,
        seed=args.seed,
        months=args.months,
        start_month=args.start_month,
        villages=(village,),
        random_events=not args.no_random_events,
        council=replace(defaults.council, enabled=not args.no_council, relief=not args.no_relief),
    )
    forced = tuple(_parse_event(text) for text in args.event)
    # Check the forced events before running anything.
    Simulation(replace(config, months=0, scheduled_events=forced))

    _print_header(config, forced, args.runs)
    baseline = run_batch(config, args.runs)
    scenario = run_batch(replace(config, scheduled_events=forced), args.runs) if forced else None

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
    parser.add_argument("--event", action="append", default=[], metavar="NAME@MONTH", help="force an event, e.g. drought@4; repeatable")
    parser.add_argument("--no-random-events", action="store_true", help="only the forced events happen")
    parser.add_argument("--no-council", action="store_true", help="the village never has a council (no taxes, reserve or relief)")
    parser.add_argument("--no-relief", action="store_true", help="the council gives no famine relief")
    parser.add_argument("--population", type=int, default=village.population, help="villagers at the start (default %(default)s)")
    parser.add_argument("--land", type=float, default=village.land, help="farmland in plots (default %(default)s)")
    parser.add_argument("--food-months", type=float, default=village.initial_food_months, help="months of food in store at the start (default %(default)s)")
    parser.add_argument("--out", default="output", help="folder for the CSV files and chart (default %(default)s)")
    parser.add_argument("--plot", action="store_true", help="also draw overview.png (needs matplotlib)")
    parser.add_argument("--list-events", action="store_true", help="show the events that can happen, then exit")
    return parser


def _parse_event(text: str) -> ScheduledEvent:
    name, sep, month = text.partition("@")
    if not sep or not month.isdigit():
        raise SystemExit(f"--event {text!r}: expected NAME@MONTH, e.g. drought@4")
    return ScheduledEvent(event=name.strip().replace(" ", "_"), month=int(month))


def _label(record) -> str:
    return f"{record.month_number:>3}  {calendar.month_abbr[record.month]} Y{record.year}"


def _print_header(config: Config, forced: tuple[ScheduledEvent, ...], runs: int) -> None:
    v = config.villages[0]
    print(f"Village of {v.population} people, {v.land:g} plots of land, {v.initial_food_months:g} months of food in store")
    when = f"{calendar.month_name[config.start_month]} of year 1"
    randomness = "random events on" if config.random_events else "no random events"
    print(f"{config.months} months from {when}; seed {config.seed}; {runs} run{'s' * (runs != 1)}; {randomness}")
    cc = config.council
    if not cc.enabled:
        print("No council")
    elif v.population >= cc.forms_at_population and cc.established_at_start:
        print(f"Council: {cc.tax_rate:.0%} tax, {cc.reserve_months:g}-month food reserve, relief {'on' if cc.relief else 'off'}")
    else:
        print(f"A council forms once the village has {cc.forms_at_population} people for {cc.forms_after_months} months")
    if forced:
        print("Forced: " + ", ".join(f"{e.event.replace('_', ' ')} in month {e.month}" for e in forced))
    print()


def _print_months(sims: list[Simulation]) -> None:
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
    print(f"{'Month':<12} {'Pop':>14} {'Died so far':>16} {'Stock':>18} {'Ration':>17} {'Health':>8}")
    pop, deaths = spread(series(sims, "population")), spread(series(sims, "deaths").cumsum(axis=1))
    stock, ration = spread(series(sims, "food_stock")), spread(series(sims, "ration"))
    health = spread(series(sims, "avg_health"))
    for i, r in enumerate(sims[0].records):
        print(
            f"{_label(r):<12} {_range(pop, i, '{:.0f}'):>14} {_range(deaths, i, '{:.1f}'):>16} "
            f"{_range(stock, i, '{:,.0f}'):>18} {_range(ration, i, '{:.0%}'):>17} {health.mean[i]:>8.0f}"
        )


def _range(s, i: int, fmt: str) -> str:
    return f"{fmt.format(s.mean[i])} ({fmt.format(s.low[i])}-{fmt.format(s.high[i])})"


def _print_comparison(baseline: list[Simulation], scenario: list[Simulation]) -> None:
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
    months_fmt = ".1f" if runs > 1 else ".0f"
    print(
        f"  {'Months on short rations':<28}{result.hungry_months[0]:{months_fmt}} without, "
        f"{result.hungry_months[1]:{months_fmt}} with"
    )
    print(f"  {'Lowest average health':<28}{result.lowest_health[0]:.0f} without, {result.lowest_health[1]:.0f} with")
    if result.relief[1] > 0 or result.relief[0] > 0:
        print(f"  {'Council relief (rations)':<28}{result.relief[0]:.0f} without, {result.relief[1]:.0f} with")


def _print_outlook(sims: list[Simulation]) -> None:
    pop = series(sims, "population")
    deaths, births = series(sims, "deaths").sum(axis=1), series(sims, "births").sum(axis=1)
    hungry = (series(sims, "ration") < 1.0).sum(axis=1)
    start = sims[0].records[0].population - sims[0].records[0].births + sims[0].records[0].deaths
    months = pop.shape[1]
    print(f"\nAfter {months} months" + ("" if len(sims) == 1 else f" (average of {len(sims)} runs)") + ":")
    print(f"  Population {start} -> {pop[:, -1].mean():.0f}; births {births.mean():.0f}, deaths {deaths.mean():.0f}")
    print(f"  Months on short rations: {hungry.mean():.1f}")


def _print_events(config: Config) -> None:
    print(f"{'Event':<14} {'Scope':<9} {'Chance':>8}  {'When':<10} {'Lasts':<8} Effects")
    for spec in config.events:
        when = ", ".join(calendar.month_abbr[m] for m in spec.months) if spec.months else "any month"
        lasts = "-".join(map(str, spec.duration)) if isinstance(spec.duration, tuple) else str(spec.duration)
        effects = ", ".join(f"{k} {v}" for k, v in spec.effects.items())
        print(f"{spec.name:<14} {spec.scope:<9} {spec.chance:>8.1%}  {when:<10} {lasts + ' mo':<8} {effects}")
    print("\nForce a village event with --event NAME@MONTH, e.g. --event drought@4")


if __name__ == "__main__":
    main()
