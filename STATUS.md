# Project status

Written for whoever picks this project up next. Read this first, then
[PLAN.md](PLAN.md) (design, every step's measurements and the roadmap) and
[README.md](README.md) (how to run it). The hand-off for porting the model to
Rust is [docs/RUST_HANDOFF.md](docs/RUST_HANDOFF.md); the viewer to watch the
country evolve is built (`econ-sim view`; its hand-off was
[docs/GUI_HANDOFF.md](docs/GUI_HANDOFF.md)).

Last updated: October 2026, after the Rust engine's milestones 1-7 and its
Phase 3 work through ten million people, starting points, a ranked log, a
richer economy of four resources and a viewer (the Python model at the end
of Phase 3 step 4).

## What this is

A month-by-month simulation of a pre-industrial economy (peasant families,
their land, grain, animals, coins and debts, village businesses, a lord, the
state, market towns and the trade between them), built to answer one kind of
question: **"what happens over the next 12 to 60 months if this event
hits?"** The product is the **paired what-if**: two runs with the same
seed, one with a forced event (a drought in month 4, a plague, a debasement,
a wool slump), one without. Every rule draws from its own named random
stream, so the two runs share the same luck until the event starts, and the
gap between them is the event's effect, not chance. Repeating over many
seeds gives the average effect and its range.

It started as one village of 1,000 people, is now a region on a map
(villages, market towns, a city, roads and a river), and is meant to grow to
provinces and a country (1 to 10 million people), calibrated to **England
around 1300**, with the Great Famine of 1315-17 and the Black Death as
validation runs.

## Where things stand

- **Code:** Python 3.13 + numpy, package `econ_sim/` (about 7,300 lines in
  27 modules), `tests/` (about 3,200 lines, 305 tests), `scripts/`
  (benchmark, sanity check, stress test).
- **Latest work:** branch `claude/rust-lenders` (Rust); the
  Python model's latest is `claude/trade-network`, commit `a238613`
  ("PLAN: the trade network, measured"). `claude/busy-mendel-edwts2` is the
  designated hand-off branch and is kept fast-forwarded to the latest work.
  This file was added on `claude/project-status`.
- **No pull requests** have been opened; work is on a chain of branches,
  one per step (`claude/village-model` ... `claude/rust-lenders`), each
  built on the one before.
- **All checks pass** at `a238613`: 305 tests (about 4 minutes), the full
  sanity check (3,010 runs over many settings, including regions), the
  stress test (67 scenarios of 40 years, 0 drifting), and runs without a
  map give number-for-number the same results as before the map existed.
- **Phase 1 and 2/2b are done, Phase 3 steps 1-5 of 13 are done.** Step 5
  (regional migration: harvest hands, famine refugees) is the first built
  in Rust only, switched off by default, on `claude/regional-migration`.
  Next in the plan is step 6 (epidemics that spread).
- **Families have minds by default** in the Rust engine (the user's
  direction), and the standard setting is a region of a million people
  (`rust/configs/regions/region1m.json`; a plain `econ-sim` runs it,
  `--size 100k` the region of 100,000). A month of a million takes 0.5 s
  on one core; a paired what-if over 20 seeds, 3.2 minutes on 4 cores.
  Minds were recalibrated to be steady: in ordinary years they match the
  rules on every measure within chance, and over 40 years they don't
  drift. Families still differ: the middle 80% decide 0.7 to 1.4 times
  the rules' amounts. Branches `claude/minds-default` and
  `claude/million-people`; PLAN.md, "Rust engine", sections 8 and 9.
- **A big region has a real city and several outlets** (branch
  `claude/rust-region-layout`; PLAN.md, "Rust engine", section 10). The
  river town is the region's stronghold (15,000 people at a million),
  kept by the state's pay to its garrison and officials: three-quarters
  of the tax, spent there (Rust only). Four towns trade beyond the
  region at a million. Smaller regions are laid out as before.
- **Runs start from many states** (branch `claude/rust-starting-points`;
  PLAN.md, "Rust engine", section 12): seventeen starting points (after a
  plague, crowded, lean or full barns, short of coin, heavy lords, free
  peasants, in debt, war-torn, worn land, open or closed trade...), any
  parameter changed from the command line (`--set`, `--scale`), and
  `econ-sim starts` to compare them from the same seeds for up to ten
  years, with or without an event. Runs need not be longer than ten years
  (the user's direction).
- **The log is ranked by importance** (branch `claude/rust-chronicle`;
  PLAN.md, "Rust engine", section 13): year by year, the year in brief
  first, then each month's entries from rank 1 (much of the region, or a
  forced event) to 4 (the small change of a month), the places under each,
  worst first. `log.txt` and `log.json`; `--log-level` sets how much.
- **A richer economy** (branch `claude/rust-crafts`; PLAN.md, "Rust
  engine", section 14): iron is dug and smelted only where the hills have
  it (and in the towns nearest them), with charcoal from the place's
  woods, and merchants sell it across the region from the makers' stock
  along fixed supply lines, so trouble in the iron hills makes tools dear
  everywhere. Cloth and iron are sold beyond the region at the border
  towns. Over ten years at a million, tools stay within 0.93 to 1.25 times
  their price without it, and the exports' coins keep 1.4% more people.
  The chronicle tells when tools grow dear. `--set crafts.enabled=false`
  (or `--start village-crafts`) gives the old economy, month for month.
- **More resources and trades** (branch `claude/rust-more-crafts`;
  PLAN.md, "Rust engine", section 15): lead in the hills, salt at brine
  springs, clay in the valleys, each in patches of its own. Saltworks
  make a staple every family needs, potters make pots that take a fifth of
  families' comfort spending and need lead for their glaze, and lead is
  also sold beyond the region. A town's salt pans and kilns burn wood from
  its countryside. Over ten years at a million, tools stay below their
  price without the crafts, families get about three-quarters of their
  salt, wages buy a third more food, and the region ends 4.4% bigger.
- **A viewer** (branch `claude/rust-viewer`; PLAN.md, "Rust engine",
  section 16): `econ-sim view --size 100k --event drought@4` writes one
  HTML page (`output/viewer.html`) with the run and its pair in it, to
  replay in any browser: a map of the places month by month, coloured by
  any of 25 measures (food eaten, deaths, people, leaving and arriving,
  the price of every good, pay, coins, debts, salt got, woods, the
  resource trades' workers), with and without the forced events or the
  gap between them; the region's 37 charts; and the chronicle of the
  month, whose entries light up their places. Ten years at a million is a
  9.9 MB page made in 72 s; at ten million it shows the 984 towns with
  their countryside. Runs stay the same, month for month. Seen with it: in
  a drought, grain is dearest in the villages next to the towns, every
  town falls into famine, and the towns lose more than half their people
  in two years, mostly to the villages.
- **Economic and welfare indicators** (branch `claude/rust-indicators`;
  PLAN.md, "Rust engine", section 17): the records gain output, the cost
  of living, the real wage, coins held and gained, families' income,
  wealth a head with its Gini and the richest tenth's share, life
  expectancy at birth and deaths before five; the places gain output, the
  cost of living, the underfed, the price of land and wealth. The viewer
  maps seven of them and charts fourteen. Read-only: runs stay the same.
  A drought at a million cuts life expectancy at its worst to 8.5 years
  and the real wage by a third; ten years on, the struck region's pay buys
  half as much again as the unstruck one's.
- **Moneylenders in the towns, and an interest rate that moves** (branch
  `claude/rust-lenders`; PLAN.md, "Rust engine", section 18; the user's
  direction: financialization): each market town's richest families put
  half their coins into lending, to the town and its villages, for what
  no neighbour will lend; they lend on bonds, not land, share every
  repayment and loss by their claims, and spend their profits in the
  town. Each place's rate follows the borrowing asked against the coins
  on offer (20% at the usual 3%, between 10% and 60%). On by default;
  `--set finance.enabled=false` (or `--start village-credit`) gives the
  old credit, month for month. In a drought at a million credit averages
  36% in year two (60% in the towns all winter), the lenders lend 238,000
  coins in two years and are left owed 73,000 by villagers. They save no
  lives; where coins are scarce, dear money costs families half as much
  land again to foreclosure.
- **Ten million people run** (branch `claude/rust-ten-million`; PLAN.md,
  "Rust engine", section 11): 24,600 villages, 984 towns, 13 outlets and
  a city of 148,246 (`export_config.py --large`, then `econ-sim --size
  10m`). A paired three-year run takes 4.7 minutes on two threads and 7.7
  GB. The city of that size shrinks 11% in three ordinary years: a
  country would have several cities.
- **The Rust engine** (`rust/`, about 10,000 lines) is built through the
  hand-off's seven milestones, one branch each
  (`claude/rust-scaffolding` ... `claude/rust-minds`, latest `cceeb20`,
  also on `claude/busy-mendel-edwts2`). It agrees with the Python model
  statistically: setup, village and region parity, 879 measures. It also
  matches exactly on every deterministic function (golden fixtures). It
  runs about 17 times faster on a village and 3.3 times on a million
  people, and families can have minds of their own (off by default). The
  Python model stays the reference; details in PLAN.md, "Rust engine".

## What is built

### Phase 1: a village that feeds itself
Farming by season, a food store, births, deaths and ageing at pre-modern
rates, health from food and warmth, random events (drought, good weather,
cold years, harsh winter, forest fire, disease, plague, army, raid,
debasement, debt jubilee, granary fire, accident; a wool slump on a map),
paired what-ifs over many seeds, charts and CSVs.

### Phase 2: a village economy
A village of 1,000 in about 200 families (households with marriages,
remarriage, heirs, kin); money and a food market; four businesses
(farming, woodcutting, weaving, smithing) making four products (food,
firewood, clothing, tools) with stocks, supplies, tools, wages and
mark-ups; a council that forms, taxes, keeps a grain reserve, gives famine
relief and pays healers; basic healthcare.

### Phase 2b: a peasant village
Families hold land and grain and live off their own store (the harvest is
shared in kind); seed grain and livestock (plough animals, herds, the
November cull); debt, distress sales and a land market (foreclosure); a lord
(demesne, labour, barn) and the state (an annual tax in coin); a regional
grain market and coins that are metal, flowing in and out with trade;
everyone at harvest, cloth made at home, kin who give grain; woods that
thin, soil that tires, weather with a memory (droughts come in runs).
Checked thoroughly (PLAN.md, "Checking Phase 2b").

### Phase 3 so far
1. **Speed.** Group-by operations instead of loops over villages, one
   compaction a month, vectorised setup, runs in parallel processes. A
   month of 1,000,000 people takes about 1.5-2.1 s on one core; one paired
   three-year run at a million takes about a minute on 4 cores.
   (`scripts/benchmark.py`.)
2. **Migration by how good a place is.** People leave or come by a
   village's *appeal*: its livelihood (wage, or land yield per person after
   seed and the lord's share), safety (remembered hunger and danger), size,
   land to spare, and the lord's take. Chain migration, flight in famine.
   Small or struck villages can empty out; villages thinned by famine fill
   up again.
3. **A region on a map** (`geography.py`). Places with coordinates and
   terrain (plain, valley, upland, forest, coast), roads and rivers, the
   cost of carting grain to the cheapest market town (Dijkstra), weather
   that falls on the map (a smooth random field, about 250 km across), so
   neighbours share droughts and good years; region-wide events (cold years
   after a volcano, debasement); a generator that lays out N villages from a
   few settings (`RegionSettings`, `--region N`).
4. **Market towns** (`towns.py`). Towns are places with people (crafts,
   no farmland, 1.25x death rates, more skilful craftsmen), grain dealers
   with a store and coins, and a grain price set by what the villages
   bring and the town eats (delivered price, marked up as the dealers'
   store runs low); merchants cart grain and goods both ways on
   consignment; lords sell their grain in town and spend their rents there
   (paying townsfolk as servants); only a border town trades with the world
   beyond at an outside price.
5. **A trade network.** Towns trade grain and goods with their six nearest
   towns along roads and rivers (a river carries three times as much); the
   river town is a city when there are several towns; wool shorn each June
   and sold beyond the border (a wool slump is a lever); specialisation by
   terrain (valleys are the granary, hills the wool country).

### Findings worth knowing (measured, written up in PLAN.md)
- Weather on the map makes regional famines possible. Three region-wide
  droughts in a row kill 15%, 25% and 13% of people: **too deadly** (the
  Great Famine killed perhaps 10-15%). That is for calibration (step 11).
- Towns keep about 12% of a region's people, stable over 40 years. In a
  region-wide drought towns lose 86 per 1,000 against 14 in villages.
- Trade between towns trims a struck area's grain price (2.06 against 2.16,
  peak 2.42 against 2.62) but saves no lives the runs can detect (12.6 vs
  12.2 extra deaths per 1,000): grain crosses three legs of carting. What
  would save lives is step 7 (public granaries, relief, export bans).
- Wool coins (a third to nine-tenths of a coin a head a year) offset the
  coins the state's tax drains out of the countryside.

## How the code is laid out

| Module | What it holds |
|---|---|
| `config.py` | Every parameter, as frozen dataclasses (`Config` and its parts: demography, food, health, money, land, livestock, credit, lord, state, town, migration, work, environment, council, healthcare, map, products, businesses, events). About 1,000 lines; the single source of numbers. |
| `world.py` | `World`, the whole state, and `create_world` (setup from a config). |
| `simulation.py` | `Simulation.step()`: the monthly loop, the same steps in the same order every month; records and per-place series. |
| `population.py` | The people table (one row per person; columns below). |
| `households.py` | Families: forming them, land, kin links, weddings. |
| `rules.py` | Deaths, births, ageing, health. |
| `economy.py`, `market.py`, `work.py` | Businesses and jobs; buying and selling with coins, wages; harvest labour, home weaving, kin help. |
| `farms.py` | Sharing the harvest in kind; families living off their stores. |
| `credit.py` | Loans, repayment, foreclosure, land sales. |
| `livestock.py` | Herds, plough animals, the winter cull, shearing. |
| `lords.py` | The lord and the state: rent, labour, barn, tax, spending in town. |
| `council.py`, `healthcare.py` | The village council; healers. |
| `environment.py` | Woods and soil. |
| `events.py` | Random and scheduled events: starting, ending, combining effects; weather on the map. |
| `migration.py` | Appeal, who leaves, where they go, who hosts them. |
| `town.py` | The outside regional grain market (used when there is no map, and as the world beyond a border town). |
| `geography.py` | The map, routes, carriage costs, weather field, the region generator. |
| `towns.py` | Market towns: dealers, carts, consignment, trade between towns, the border. |
| `metrics.py` | `MonthRecord`: monthly totals (never per-person logs). |
| `scenarios.py` | Batches, paired comparisons, effects by place, CSV output; parallel runs. |
| `checks.py` | Invariants that must hold every month. |
| `rng.py` | Named random streams. |
| `__main__.py`, `charts.py` | The command line; the overview chart. |

### The state
- **People** (`Population`, columns of numpy arrays): `count` (people a
  row stands for, 1 today; kept for a weighted sample later), `age_months`,
  `female`, `health` (0-100), `skill`, `location` (place index),
  `household`, `job` (business index or `NO_JOB`), `married`, `couple`.
  Rows emptied by deaths and departures are dropped once a month; the table
  keeps spare room for newborns.
- **Households**: `location`, `money` (coins), `land` (plots), `grain`
  (rations), `animals`, `debt`, `lent`, `kin` (the family it came from).
- **Places** (villages first, then towns): per-place arrays on `World`
  (`land`, `wage_level`, `woods`, `soil`, `dealers_grain`, `dealers_cash`,
  ...), and places x products / places x businesses arrays (`stock`,
  `prices`, `markup`, `cash`, `tools`, `supplies`, `pay`, `orders`,
  `skill`). The lord, council and town (outside market) are small
  dataclasses of per-place arrays. `Geography` holds the map.

### One month (`Simulation.step`), in order
1. **Events** end and start (scheduled, then random, rolled by place, with
   the weather field on a map); debts accrue; raiders take grain; the
   outside price drifts, coins are debased or lost; fire and pests hit
   stores; herds grow or die (the November cull); distress slaughter.
2. **Production**: businesses make goods (farms by what was sown and the
   plough animals); everyone helps at harvest; fair prices x mark-ups. On a
   map, the town's grain price is the delivered price (cheapest of its
   villages plus carriage, or a neighbouring town plus the route).
3. **The year's cover** sets the grain price and whether it is a famine
   year; merchants' carts are planned (`towns.carts`); the harvest is shared
   in kind (council levy, seed, farms, plough owners, workers, holders);
   the lord's share goes to his barn and part is carted away (sold to the
   town's dealers on a map).
4. **Families' needs**: they eat from their store, offer what they can
   spare, set coins aside; kin help, the council helps; distress sales of
   animals, borrowing, land sales.
5. **Markets**, most needed first: grain (families, farms, merchants' grain,
   the lord), then the rest; consignment carts settle (`towns.settle`);
   towns trade grain with each other and the border (`between_towns`,
   `border_trade`); the council's reserve, famine foods, firewood gathered
   from the commons; woods regrow, soil tires or rests.
6. **Wages**; dealers pay out; lords pay servants; wool sold in June; council
   taxes and pays officials and healers; repayment and foreclosure; the
   state's tax after harvest; the customary wage follows the money there is.
7. **Spoilage; sowing.**
8. **Health** from food and warmth; healers see the likeliest to die first.
9. **Deaths, births, ageing**; council staff; businesses hire and fire;
   weddings; **migration** (leaving and arriving by appeal).
10. **Mark-ups** move with demand against supply (grain by how short the
    year looks, a town's by its dealers' store).
11. **Record** the month.

### Randomness and pairing
`RandomStreams(seed)[name]` is `np.random.default_rng([seed,
crc32(name)])`: one stream per rule ("events", "weather", "deaths",
"births", "migration", "marriage", "jobs", "production", "council",
"setup", "land", "kin", ...). Adding a rule never shifts another rule's
draws. A paired run is identical to its baseline until the event's month.
Effects that only exist on a map (`MAP_ONLY_EFFECTS`, e.g. `wool_price`)
are never rolled without one, so village runs keep their old draws.

### Invariants (`checks.py`, checked every month in tests and the sanity check)
Land conserved (families + demesne = village land); debts equal claims in
every village; nothing negative or NaN (grain, coins, land, animals, stocks,
barn, reserve, treasury, woods, dealers' grain and coins); soil in range;
every person in a household of their own place; couples are one man and one
woman, both married; kin links within the place. The tests also check that
**coins are conserved** (only trade with the world beyond moves them, and
that is booked in the outside purse) and that **people add up**
(births - deaths - emigrants + immigrants).

## How to run and check it

The Rust engine (`cd rust`; the binary is `target/release/econ-sim`):

```bash
cargo build --release && cargo test --release       # 183 tests
econ-sim --event drought@4 --runs 20                # the standard region of a million, with minds (4 min)
econ-sim --size 100k --event drought@4 --runs 100   # the region of 100,000 (2 min)
python ../scripts/export_config.py --large          # once: the region of ten million (7 MB, not in git)
econ-sim --size 10m --event drought@4 --workers 2   # a paired run of ten million (5 min, 7.7 GB)
econ-sim bench --config configs/regions/region1m.json   # time any setting
econ-sim starts --months 60 --runs 2                # every starting point side by side (5 min)
econ-sim starts --only standard,crowded --event drought@4   # what a drought does from each
econ-sim --start after-plague --set lord.demesne=0.4 --scale villages.land=1.2   # a start of your own
econ-sim starts --size 100k --months 120 --only standard,few-ironworkers,village-crafts   # the iron trade
econ-sim logs --size 100k --event drought@4 --log-level 2   # what happened, the most important first
econ-sim view --size 100k --event drought@4         # a page to watch it, output/viewer.html (6 s; a million: 72 s)
econ-sim view --size 100k --start crowded --event drought@4 --months 60 --page crowded.html --title "A crowded region's drought"
econ-sim --population 1000 --event drought@4 --runs 200   # one village
econ-sim --region 50 --event drought@4@0,12,11 --runs 40
econ-sim minds --event drought@4 --runs 200         # minds against the rules
econ-sim compare refugees --config configs/regions/city3.json --event drought@4 --runs 60
econ-sim sanity --full; econ-sim stress; econ-sim bench --country
python ../scripts/parity.py village                 # parity with Python (also setup, region)
python ../scripts/export_config.py                  # after changing config.py: re-export
```

The Python model:

```bash
.venv/bin/python -m pytest -q                       # 312 tests, about 2 minutes
.venv/bin/python scripts/sanity_check.py            # quick; --full: 3,010 runs, a few minutes on 4 cores
.venv/bin/python scripts/stress_test.py             # 40-year drift check; --full, --only NAME
.venv/bin/python scripts/benchmark.py --quick       # speed; --profile 200000 for where the time goes
.venv/bin/python -m econ_sim --event drought@4 --runs 200 --plot
.venv/bin/python -m econ_sim --region 50 --event drought@4@0,12,11 --runs 40
.venv/bin/python -m econ_sim --list-events
```

Outputs go to `output/`: `run1.csv`, `run1_with_event.csv`, `summary.csv`
(effects with ranges), `places.csv` (per place, in a region), `log.txt`,
`overview.png`. `ECON_SIM_WORKERS` sets the number of cores.

The machine this was built on: 4 cores, Python 3.13 / numpy 2.x in
`.venv`, Rust 1.97 (`/root/.cargo/bin/cargo`), gcc 13, clang.

## Conventions (keep them)

- **A branch per step**, named `claude/<step>` (the user asked to "make new
  branches often"), each built on the last; push with `git push -u origin
  <branch>` (retry up to 4 times, 2/4/8/16 s, on network errors); keep
  `claude/busy-mendel-edwts2` fast-forwarded to the latest work. **No pull
  requests unless the user asks.** Commit and push before ending a turn.
- **Commit messages** end with the `Co-Authored-By` and `Claude-Session`
  trailer lines the session gives; no model names in commits, code or docs.
- **Every step is verified the same way** before it is called built: the
  tests; invariants every month; the sanity check (`--full`); the stress
  test; runs without a map (or without the new feature) **number-for-number
  identical** to the previous commit (run both in a git worktree and diff
  the records); and the step's what-if measured and **written up in
  PLAN.md** with numbers (what changed, what it does, what it doesn't).
- **Style:** plain everyday words for names and comments (families, grain,
  rations, coins, carts, "how good a place it is"), comments that say why,
  tests named as sentences (`test_a_town_lives_on_its_villages_grain`),
  parameters in `config.py`, never inline. Monthly totals only, no
  per-person logs.
- When a fix is a judgement call (a parameter that makes things plausible),
  say so in PLAN.md and keep the measurement that motivated it.

## Known issues and limits

- **Multi-year famines too deadly** (above); adult mortality a little high;
  grain only a little dearer before harvest than after; seed a seventh of
  the harvest (medieval yields were 3-5 to 1); the lord, state and council
  take about 15% of the harvest (25-50% was usual; no tithe yet). All for
  calibration (step 11).
- **Towns are about 90% weavers**: few products (food, firewood, clothing,
  tools). A town bigger than about a fifth of its countryside can't be fed.
- An ordinary village settles about 10% below its starting population.
- Price levels across runs spread with the weather; the state's tax drains
  coins until state spending exists (step 7).
- A 50-village region shows some strain in its first years.
- Relief of a local drought by trade is marginal (by design of the carting
  costs; step 7 adds policies).
- Speed (Python): 2.1 s a month at 1,150,000 people (1,000 villages, 40
  towns) on one core. The Rust engine takes 0.41 s for a million without
  a map (target 0.2 s, missed: memory-bound; see PLAN.md), 0.5 s for the
  million region with minds, and meets every paired-run target, ten
  million on a map with minds included (4.7 min).
- **The grain price follows the place's whole store**, family grain
  included, not what is offered for sale. So in this model hoarding can't
  raise prices (PLAN.md, Rust milestone 7).
- **The sanity check's century setting fails about every other time** it
  runs with new seeds, in either engine. Its two "nonsense" checks
  (population up half in 3 years, a price twentyfold) fire in 1-2% of
  century runs.
- **The 10m region needs re-exporting** after the finance settings were
  added (`python scripts/export_config.py --large`): an old
  `region10m.json` lacks them, and the engine refuses it.
- **A Python bug, mirrored in Rust:** in a shearing month on a map with
  towns, `worth` is overwritten by the wool's worth, so foreclosure and
  the credit limit value animals at their wool price. Fixing it in both
  engines is the user's call.

## What's next

**Waiting on the user:** whether the grain price should follow what is offered for sale
(needed to ask whether hoarding raised prices in 1315); whether to fix
the wool `worth` bug; and whether to loosen the century sanity checks.

**Open at a million:** a drought over the whole region still starves the
market towns (187 more deaths per 1,000 there, against 24 in the
villages). The towns' dealers and families run out of coins at three times
the usual price; that belongs to step 7 (granaries, relief).

Phase 3, remaining steps, now in Rust (details in PLAN.md, "Build order"):

| Step | Adds |
|---|---|
| 6 | Epidemics spreading within and between places; quarantine |
| 7 | Provinces, tax quotas, a treasury, public granaries, famine policies, armies |
| 8 | The tithe and parish relief; lords with several manors; mills |
| 9 | Several crops, rotations, pasture, clearing woods |
| 10 | Individual workshops (firms) instead of one business per trade |
| 11 | Calibration to England c. 1300; validation against the Great Famine |
| 12 | 10 million people: a weighted sample, checked against a full million |
| 13 | A region dashboard: maps and side-by-side what-ifs (the map and the paired what-if built: `econ-sim view`) |

### The Rust direction (the user's latest request)
The user asked whether the model could have **fewer people, each with a
mind of their own**, and whether C or Rust would help, then asked for this
project to **continue in Rust**. What was measured (a toy decision rule per
person: an expected price learned from experience, fear from hunger, and a
choice to sell, hoard or flee):

| | ns per person-month |
|---|---|
| Pure Python, a loop over objects | 770 |
| numpy, columns (as the model is written) | 32 |
| Rust, a loop over structs, one core | 8.5 |

So numpy is fine while every person follows the same rule on columns, but
**decisions that differ by person, with branches and memory, are where
Python loses** (a Python loop is 25x slower than numpy and about 90x slower
than Rust). Rust over C: memory safety, `rayon` for parallel runs and
places, `serde` for the config, a good test tool, and PyO3 if Python ever
needs to call it. The recommendation: **port the engine to Rust
incrementally, keep the Python model as the reference it is checked
against**, then give households (not every person) minds: traits, memory
and expectations, deciding to sell or hoard, borrow, stay or leave.
The plan and the prompt for that are in
[docs/RUST_HANDOFF.md](docs/RUST_HANDOFF.md).
