"""Starting points name parameters the config has (the Rust engine runs
them; see econ_sim/starts.py)."""

import dataclasses

from econ_sim.config import Config
from econ_sim.geography import RegionSettings, region
from econ_sim.starts import STARTS

REGION = dataclasses.asdict(region(RegionSettings(villages=30), Config()))


def reached(data, steps):
    """The values a path leads to: through a list, the item named, or every item."""
    if not steps:
        return [data]
    step, rest = steps[0], steps[1:]
    if isinstance(data, dict):
        assert step in data, f"no parameter {step!r} (among {sorted(data)})"
        return reached(data[step], rest)
    assert isinstance(data, (list, tuple)) and data, f"{step!r} goes past a value"
    named = [item for item in data if isinstance(item, dict) and item.get("name") == step]
    if named:
        return [v for item in named for v in reached(item, rest)]
    return [v for item in data for v in reached(item, steps)]


def test_every_starting_point_changes_parameters_the_config_has():
    for start in STARTS:
        for path, value in start.set:
            for old in reached(REGION, path.split(".")):
                assert type(old) is type(value) or (isinstance(old, (int, float)) and isinstance(value, (int, float))), (
                    f"{start.name}: {path} is a {type(old).__name__}, set to a {type(value).__name__}"
                )
        for path, factor in start.scale:
            for old in reached(REGION, path.split(".")):
                assert isinstance(old, (int, float)) and not isinstance(old, bool), f"{start.name}: {path} is no number"
                assert factor > 0


def test_starting_points_have_their_own_names_and_say_what_they_are():
    names = [s.name for s in STARTS]
    assert len(names) == len(set(names)) and names[0] == "standard"
    assert not STARTS[0].set and not STARTS[0].scale
    assert all(s.about and s.about[0].isupper() for s in STARTS)
