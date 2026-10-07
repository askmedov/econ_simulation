# Village economy simulation

A month-by-month economic simulation for asking **"what happens over the next
few years if this event hits?"** It simulates a village of 1,000 people in
about 200 families who farm, cut wood, weave and make tools, buy and sell
with coins, get sick, are born and die, with random events like droughts,
forest fires and disease. A village council taxes, keeps a grain reserve,
gives famine relief and pays healers. See [PLAN.md](PLAN.md) for the design and
roadmap.

## Setup

```bash
python -m venv .venv
.venv/bin/pip install numpy matplotlib pytest
```

Python 3.11+. matplotlib is only needed for charts.

## Running it

```bash
# One village, 36 months
.venv/bin/python -m econ_sim

# What does a drought in month 4 do? One paired run (same luck, with and without)
.venv/bin/python -m econ_sim --event drought@4

# The same, averaged over 200 paired runs, with a chart
.venv/bin/python -m econ_sim --event drought@4 --runs 200 --plot

# The same drought in a village without a council (no reserve, no relief)
.venv/bin/python -m econ_sim --event drought@4 --runs 200 --no-council

# What do healers do for an epidemic?
.venv/bin/python -m econ_sim --event disease@3 --runs 200
.venv/bin/python -m econ_sim --event disease@3 --runs 200 --no-healthcare

# Start in October, force two events, no other randomness
.venv/bin/python -m econ_sim --start-month 10 --event disease@3 --event drought@7 --no-random-events

# The events you can force
.venv/bin/python -m econ_sim --list-events
```

Other options: `--months`, `--seed`, `--population`, `--land`,
`--food-months`, `--out`, and the policy levers `--no-council`,
`--no-relief`, `--no-healthcare`. See `--help`. Everything else (products,
businesses, prices, tax rate, reserve size, healers...) is in
`econ_sim/config.py`.

A single paired run is noisy: in one run, a drought can even "save" a life
by chance. Use `--runs 100` or more to see the real effect and its range.

### Output (in `output/`)

| File | Contents |
|---|---|
| `run1.csv` | Every month of the first run: population, births, deaths, food produced, eaten, spoiled and in store, prices of every product, workers in every trade, wages, savings, how well the village and its poorest fifth ate, how warm families kept, the council's treasury, taxes, grain reserve and relief, healers and people treated, health, active events |
| `run1_with_event.csv` | The same run with the forced events |
| `summary.csv` | Key measures' average and 10–90% range across runs (population, births, deaths, food stock and price, wage, how well the village and its poorest fifth ate, health, relief, people treated, treasury, grain reserve), for the baseline, with the events (`_event`), and the difference (`_diff`) |
| `log.txt` | Notable happenings in the first run (events, food shortages, a council forming) |
| `overview.png` | Food in store, food price, health, food need met for the village and its poorest fifth, and deaths over time (with `--plot`) |

## Using it from Python

```python
from econ_sim.config import Config, ScheduledEvent
from econ_sim.scenarios import compare, effect

result = compare(Config(months=36), (ScheduledEvent("drought", month=4),), runs=200)
print(effect(result).extra_deaths.mean)
```

All parameters are in [`econ_sim/config.py`](econ_sim/config.py).

## Layout

| File | Role |
|---|---|
| `econ_sim/config.py` | Every parameter, and the event definitions |
| `econ_sim/population.py` | The population table (one row = `count` identical people) |
| `econ_sim/rng.py` | Named random streams |
| `econ_sim/world.py` | World state and the starting village |
| `econ_sim/households.py` | Families: who lives with whom, and their savings |
| `econ_sim/economy.py` | Businesses: production, tools, supplies, fair prices, who works where |
| `econ_sim/market.py` | Buying and selling, mark-ups, wages, help between neighbours |
| `econ_sim/council.py` | The village council: forming, taxes, officials, grain reserve, relief |
| `econ_sim/healthcare.py` | Healers: who they see and what treatment does |
| `econ_sim/rules.py` | Production, rationing, eating, spoilage, health, births, deaths |
| `econ_sim/events.py` | Rolling, forcing and combining events |
| `econ_sim/simulation.py` | The monthly loop |
| `econ_sim/metrics.py` | Monthly statistics and CSV |
| `econ_sim/scenarios.py` | Paired what-if runs and their effects |
| `econ_sim/charts.py` | The overview chart |
| `econ_sim/__main__.py` | The command line |

## Tests

```bash
.venv/bin/python -m pytest
.venv/bin/python scripts/sanity_check.py          # hundreds of seeds and settings
.venv/bin/python scripts/sanity_check.py --full   # thousands
```

The sanity check flags outcomes no real village would show; see PLAN.md.
