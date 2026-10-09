# Village economy simulation

A month-by-month economic simulation for asking **"what happens over the next
few years if this event hits?"** It simulates a village of 1,000 people in
about 200 families who hold land and animals (or don't), live off their
own grain stores, keep seed for next year, cut wood, weave and make tools,
buy and sell with coins, borrow (and lose land to their creditors), marry and remarry, get sick, are born and die
(at pre-modern rates), and leave or come by how good a place their
village is to live (its livelihood, its remembered hunger and dangers, its
size, its lord's take), so that small villages can empty out,
with random events like droughts (which come in runs), cold years after
a volcanic eruption, plague, armies, raiders, forest fires and disease.
Everyone helps at harvest, women spin and weave at home, and kin give each
other grain; the village's woods thin when overcut and its soil tires when
the land is crowded. A lord takes part of the harvest and labour, the state taxes
in coin, and merchants carry grain to and from a town market; coins are
metal, flowing in and out with that trade. A village council taxes, keeps
a grain reserve, gives famine relief and pays healers. See [PLAN.md](PLAN.md) for the design and
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

# Watch a council form: no council at the start, one forms after six months
.venv/bin/python -m econ_sim --council-forms-later

# What do healers do for an epidemic?
.venv/bin/python -m econ_sim --event disease@3 --runs 200
.venv/bin/python -m econ_sim --event disease@3 --runs 200 --no-healthcare

# A debasement of the coinage, or a lord who opens his barn in a drought
.venv/bin/python -m econ_sim --event debasement@3 --runs 100
.venv/bin/python -m econ_sim --event drought@4 --runs 200 --no-council --lord-charity

# Start in October, force two events, no other randomness
.venv/bin/python -m econ_sim --start-month 10 --event disease@3 --event drought@7 --no-random-events

# The events you can force
.venv/bin/python -m econ_sim --list-events
```

Other options: `--months`, `--seed`, `--population`, `--land`,
`--food-months`, `--out`, `--workers` (cores to run on; all by default), and the policy levers `--no-council`,
`--no-relief`, `--no-healthcare`, `--council-forms-later`, `--lord-charity`
(the lord opens his barn in a famine), `--remit-tax` (the state remits its
tax in a famine year), `--no-lord`, `--no-state-tax`, `--no-town` (no
merchants), `--no-migration`, `--no-kin-help` and `--no-drought-runs`. See `--help`.

Droughts come in runs: a forced drought makes another the next April
three times likelier (30% instead of about 8%), so its three-year effect
includes the second drought it brings in some runs. `--no-drought-runs`
isolates the single drought. Everything else (products,
businesses, prices, tax rate, reserve size, healers...) is in
`econ_sim/config.py`.

A single paired run is noisy: in one run, a drought can even "save" a life
by chance. Use `--runs 100` or more to see the real effect and its range.

### Output (in `output/`)

| File | Contents |
|---|---|
| `run1.csv` | Every month of the first run: population, births, deaths, weddings, food produced, eaten (and how much from families' own stores), sold, spoiled and in store, people in landless families and how they ate, prices of every product, workers in every trade, wages, savings, how well the village and its poorest fifth ate, how warm families kept, the council's treasury, taxes, grain reserve and relief, healers and people treated, health, seed, animals, debts and land sales, the lord's rent and the state's tax, the town price and grain carried to and from it, coins lost, emigrants and immigrants (whole families, moves between villages, how good a place each village is, empty villages), homespun cloth, harvest help, grain given by kin, the woods and the soil, active events |
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
| `econ_sim/households.py` | Families: who lives with whom, marriage and heirs, their savings, land and grain |
| `econ_sim/farms.py` | Peasant farms: the harvest shared out in kind, families' own food plans, grain sales, seed |
| `econ_sim/livestock.py` | Plough animals and herds: farm output, growth, the winter cull, slaughter and distress sales |
| `econ_sim/credit.py` | Loans between families, repayment, foreclosure, distress land sales, debt cancellation |
| `econ_sim/lords.py` | The lord's demesne, barn and purse; the state's coin tax; requisitions by armies and raiders |
| `econ_sim/town.py` | The town's grain price, merchants carrying grain in and out, coins lost and debased |
| `econ_sim/migration.py` | How good a place each village is to live; who leaves (young people, families, the starving), where they go, and who comes |
| `econ_sim/work.py` | Everyone at harvest, homespun cloth, kin who give each other grain |
| `econ_sim/environment.py` | Woods that regrow and thin when overcut; soil that tires on crowded land |
| `econ_sim/checks.py` | Invariants of the state (land, debts, households, couples, kin) for tests and the sanity check |
| `econ_sim/economy.py` | Businesses: production, tools, supplies, fair prices, who works where |
| `econ_sim/market.py` | Buying and selling, mark-ups, wages, help between neighbours |
| `econ_sim/council.py` | The village council: forming, taxes, officials, grain reserve, relief |
| `econ_sim/healthcare.py` | Healers: who they see and what treatment does |
| `econ_sim/rules.py` | Production, rationing, eating, spoilage, health, births, deaths |
| `econ_sim/events.py` | Rolling, forcing and combining events |
| `econ_sim/simulation.py` | The monthly loop |
| `econ_sim/metrics.py` | Monthly statistics and CSV |
| `econ_sim/scenarios.py` | Paired what-if runs (on all cores) and their effects |
| `econ_sim/charts.py` | The overview chart |
| `econ_sim/__main__.py` | The command line |

## Tests

```bash
.venv/bin/python -m pytest
.venv/bin/python scripts/sanity_check.py          # hundreds of seeds and settings
.venv/bin/python scripts/sanity_check.py --full   # thousands
.venv/bin/python scripts/stress_test.py           # 40 years of many scenarios: does anything drift?
.venv/bin/python scripts/stress_test.py --full    # 100 years, more seeds
.venv/bin/python scripts/benchmark.py             # speed at 1,000 to 1,000,000 people, against Phase 3's targets
```

Runs go on all cores: `--workers N` on the command line (and the sanity
check), or the `ECON_SIM_WORKERS` environment variable, sets how many.

The sanity check flags outcomes no real village would show over the
near-term horizon; the stress test runs far past it to find slow drifts
(prices, households, vital rates, inequality) that a few years would hide.
See PLAN.md.
