from dataclasses import asdict, replace

import numpy as np
import pytest

from econ_sim.config import Config, ScheduledEvent, VillageConfig
from econ_sim.scenarios import compare, effect, run_batch, series, spread, write_summary_csv
from econ_sim.simulation import Simulation

# A village near the limit of what its land can feed, where shocks bite.
CROWDED = Config(seed=10, months=36, villages=(VillageConfig(population=180, land=50.0),))
DROUGHT = (ScheduledEvent("drought", month=4),)


def test_scheduled_event_starts_in_its_month_and_village():
    config = replace(
        CROWDED,
        villages=(VillageConfig(name="A"), VillageConfig(name="B")),
        random_events=False,
        scheduled_events=(ScheduledEvent("disease", month=5, village=1),),
    )
    sim = Simulation(config)
    sim.run()
    assert [r.events for r in sim.records[3:5]] == ["", "B:disease"]
    assert "B: Disease outbreak (scheduled)" in sim.log[0]


def test_scheduled_event_ignores_its_usual_season():
    # Droughts normally start only in April; forced, one starts in month 1 (January).
    config = replace(CROWDED, random_events=False, scheduled_events=(ScheduledEvent("drought", month=1),))
    sim = Simulation(config)
    sim.run(1)
    assert sim.records[0].events == "drought"


def test_unknown_scheduled_event_is_rejected():
    with pytest.raises(ValueError, match="unknown village event"):
        Simulation(replace(CROWDED, scheduled_events=(ScheduledEvent("meteor", month=1),)))


def test_random_events_can_be_switched_off():
    sim = Simulation(replace(CROWDED, months=600, random_events=False))
    sim.run()
    assert all(r.events == "" and r.accidents == 0 for r in sim.records)


def test_start_month_sets_the_calendar():
    sim = Simulation(replace(CROWDED, start_month=10))
    sim.run(4)
    assert [(r.month_number, r.month, r.year) for r in sim.records] == [(1, 10, 1), (2, 11, 1), (3, 12, 1), (4, 1, 2)]


def test_paired_runs_match_until_the_event_starts():
    late = (ScheduledEvent("disease", month=30),)
    result = compare(CROWDED, late, runs=3)
    for base, event in zip(result.baseline, result.scenario):
        before = slice(0, 29)
        assert [asdict(r) for r in base.records[before]] == [asdict(r) for r in event.records[before]]


def test_drought_lowers_food_stock_in_every_run():
    result = compare(CROWDED, DROUGHT, runs=10)
    stock_gap = result.difference("food_stock")
    # Before the drought nothing differs; after the harvest every run has less
    # food, except runs whose baseline had a drought that April anyway.
    had_drought_anyway = np.array(["drought" in sim.records[3].events for sim in result.baseline])
    assert (stock_gap[:, :3] == 0).all()
    assert (stock_gap[~had_drought_anyway, 9] < 0).all()
    assert (stock_gap[had_drought_anyway, 9] == 0).all()


def test_drought_effect_is_harmful_on_average():
    result = effect(compare(CROWDED, DROUGHT, runs=20))
    assert result.extra_deaths.mean > 0
    assert result.lowest_ration[1] < result.lowest_ration[0]
    assert result.hungry_months[1] > result.hungry_months[0]
    assert result.extra_deaths.low <= result.extra_deaths.mean <= result.extra_deaths.high


def test_batch_uses_consecutive_seeds():
    sims = run_batch(replace(CROWDED, months=12), runs=3)
    assert [s.config.seed for s in sims] == [10, 11, 12]
    assert series(sims, "population").shape == (3, 12)


def test_spread_reports_mean_and_middle_80_percent():
    s = spread(np.arange(101, dtype=float)[:, None])
    assert s.mean[0] == 50 and s.low[0] == 10 and s.high[0] == 90


def test_summary_csv_has_baseline_event_and_difference_columns(tmp_path):
    result = compare(replace(CROWDED, months=6), DROUGHT, runs=2)
    path = tmp_path / "summary.csv"
    write_summary_csv(path, result.baseline, result.scenario)
    header, *rows = path.read_text().splitlines()
    assert len(rows) == 6
    for column in ("deaths", "deaths_event", "deaths_diff_low", "ration_event_high"):
        assert column in header.split(",")


def test_runs_on_several_cores_match_runs_on_one():
    config = Config(seed=3, months=12)
    one = run_batch(config, runs=3, workers=1)
    many = run_batch(config, runs=3, workers=3)
    assert [[asdict(r) for r in run.records] for run in one] == [[asdict(r) for r in run.records] for run in many]
    assert [run.log for run in one] == [run.log for run in many]
