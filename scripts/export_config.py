"""Export the model's parameters to JSON for the Rust engine.

Python's `Config` stays the one source of numbers: the Rust engine reads
these files (`rust/configs/`) and never keeps numbers of its own.

    python scripts/export_config.py                  # rewrite every file in rust/configs/
    python scripts/export_config.py --region 50 --map-seed 3 --out my_region.json

Each file is `dataclasses.asdict(config)` as it is (tuples become lists).
The suites (`sanity.json`, `stress.json`) hold the settings of
`scripts/sanity_check.py` and `scripts/stress_test.py`, each with its whole
config, so the Rust checks run exactly what the Python ones run.
"""

from __future__ import annotations

import argparse
import dataclasses
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(ROOT / "scripts"))

from econ_sim.config import Config  # noqa: E402
from econ_sim.geography import SIZE_BY_TERRAIN, RegionSettings, region  # noqa: E402

CONFIGS = ROOT / "rust" / "configs"

# Regions the Rust engine is checked on (the sanity check's, the stress
# test's, and a bigger one for parity per place).
REGIONS = {
    "region20": RegionSettings(villages=20, mean_size=300),
    "region20_seed2": RegionSettings(villages=20, mean_size=300, seed=2),
    "city3": RegionSettings(villages=45, mean_size=250, towns=3, seed=3),
    "region100": RegionSettings(villages=100, seed=1),
}


def as_json(config: Config) -> dict:
    """The config as plain JSON data."""
    return json.loads(json.dumps(dataclasses.asdict(config)))


def dump(data, path: Path) -> None:
    """Write `data`: a config readable, a suite of many compact, one per line."""
    path.parent.mkdir(parents=True, exist_ok=True)
    if isinstance(data, list):
        text = "[\n" + ",\n".join(json.dumps(item, separators=(",", ":")) for item in data) + "\n]"
    else:
        text = json.dumps(data, indent=1)
    path.write_text(text + "\n")


def sanity_suite() -> list[dict]:
    import sanity_check

    quick, full = sanity_check.settings(False), sanity_check.settings(True)
    return [
        {"label": q.label, "seeds": q.seeds, "seeds_full": f.seeds, "disasters_allowed": q.disasters_allowed,
         "config": as_json(q.config)}
        for q, f in zip(quick, full)
    ]


def stress_suite() -> list[dict]:
    import stress_test

    return [
        {"name": s.name, "note": s.note, "expected": list(s.expected), "config": as_json(s.config)}
        for s in stress_test.scenarios()
    ]


def region_settings() -> dict:
    """What `geography.region` lays a region out from by default (the Rust
    engine's region generator reads its numbers here)."""
    return {"settings": json.loads(json.dumps(dataclasses.asdict(RegionSettings()))), "size_by_terrain": SIZE_BY_TERRAIN}


def standard_files() -> dict[str, object]:
    """Every file in rust/configs/, by its path there."""
    files: dict[str, object] = {"village.json": as_json(Config()), "region_settings.json": region_settings()}
    for name, settings in REGIONS.items():
        files[f"regions/{name}.json"] = as_json(region(settings, Config()))
    files["sanity.json"] = sanity_suite()
    files["stress.json"] = stress_suite()
    return files


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--region", type=int, default=0, metavar="VILLAGES", help="export one region of this many villages")
    parser.add_argument("--towns", type=int, default=None)
    parser.add_argument("--map-seed", type=int, default=1)
    parser.add_argument("--mean-size", type=float, default=RegionSettings.mean_size)
    parser.add_argument("--out", default=None, help="where to write the one region (default: stdout)")
    args = parser.parse_args()
    if args.region:
        settings = RegionSettings(villages=args.region, towns=args.towns, seed=args.map_seed, mean_size=args.mean_size)
        data = as_json(region(settings, Config()))
        if args.out:
            dump(data, Path(args.out))
        else:
            print(json.dumps(data, indent=1))
        return 0
    for name, data in standard_files().items():
        dump(data, CONFIGS / name)
    print(f"Wrote {len(standard_files())} files to {CONFIGS}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
