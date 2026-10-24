"""The Rust engine reads its numbers from files exported from Python's
config (rust/configs) and checks itself against fixtures exported from
Python's rules (rust/fixtures): both must match a fresh export, so the two
engines never drift apart."""

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))

import export_config  # noqa: E402
import export_fixtures  # noqa: E402


def test_the_rust_configs_match_a_fresh_export_of_the_python_config():
    for name, data in export_config.standard_files().items():
        path = export_config.CONFIGS / name
        assert path.exists(), f"rust/configs/{name} is missing: run scripts/export_config.py"
        assert json.loads(path.read_text()) == json.loads(json.dumps(data)), (
            f"rust/configs/{name} is out of date: run scripts/export_config.py"
        )


def test_the_rust_fixtures_match_a_fresh_export_of_the_python_rules():
    geography = json.loads(json.dumps(export_fixtures.geography_file()))
    assert json.loads((export_fixtures.FIXTURES / "geography.json").read_text()) == geography, (
        "rust/fixtures/geography.json is out of date: run scripts/export_fixtures.py"
    )
    for name, (config, months) in export_fixtures.SETTINGS.items():
        sim = export_fixtures.settle_state(config, months)
        rec = export_fixtures.Recorder(sim.world.population, sim.world.households)
        export_fixtures.record_state(rec, sim)
        saved = json.loads((export_fixtures.FIXTURES / f"{name}.json").read_text())
        assert saved["state"] == json.loads(json.dumps(rec.state)), f"{name}: state out of date"
        assert saved["cases"] == json.loads(json.dumps(rec.cases)), (
            f"rust/fixtures/{name}.json is out of date: run scripts/export_fixtures.py"
        )
