import pytest

from econ_sim.__main__ import main


def test_single_run_writes_its_files(tmp_path, capsys):
    main(["--months", "12", "--out", str(tmp_path)])
    out = capsys.readouterr().out
    assert "After 12 months" in out
    assert {p.name for p in tmp_path.iterdir()} == {"run1.csv", "summary.csv", "log.txt"}


def test_event_comparison_reports_the_effect(tmp_path, capsys):
    main(["--months", "18", "--runs", "5", "--event", "drought@4", "--out", str(tmp_path)])
    out = capsys.readouterr().out
    assert "Effect of drought in month 4 over 18 months" in out
    assert "Extra deaths" in out
    assert (tmp_path / "run1_with_event.csv").exists()


def test_chart_is_drawn(tmp_path):
    pytest.importorskip("matplotlib")
    main(["--months", "12", "--runs", "3", "--event", "disease@2", "--plot", "--out", str(tmp_path)])
    assert (tmp_path / "overview.png").stat().st_size > 10_000


@pytest.mark.parametrize("event", ["drought", "drought@", "drought@soon"])
def test_badly_written_event_is_rejected(event, tmp_path):
    with pytest.raises(SystemExit):
        main(["--event", event, "--out", str(tmp_path)])


def test_unknown_event_is_rejected(tmp_path):
    with pytest.raises(ValueError, match="unknown village event"):
        main(["--event", "meteor@3", "--out", str(tmp_path)])


def test_list_events(capsys):
    main(["--list-events"])
    assert "drought" in capsys.readouterr().out


def test_council_can_form_during_the_run(tmp_path, capsys):
    main(["--months", "8", "--council-forms-later", "--out", str(tmp_path)])
    out = capsys.readouterr().out
    assert "A council forms once" in out and "formed a council" in out


def test_drought_runs_can_be_switched_off(tmp_path, capsys):
    main(["--months", "6", "--no-drought-runs", "--out", str(tmp_path)])
    assert "Village of" in capsys.readouterr().out
