# Continuing in Rust: the hand-off

Two parts: **the prompt** to give the next agent (copy it as it is), and
**the plan in detail** it points to. Read [STATUS.md](../STATUS.md) first for
what the model is and how it is checked.

## The prompt

> You are continuing the project in `askmedov/econ_simulation`: a
> month-by-month simulation of a pre-industrial economy (England c. 1300)
> whose product is the **paired what-if**: two runs with the same seed, one
> with a forced event, so the gap between them is the event's effect. It is
> written in Python + numpy (`econ_sim/`, about 7,300 lines, 305 tests) and
> has reached Phase 3 step 4 of 13: a region on a map, with market towns,
> a city, trade between towns, wool and terrain.
>
> **Your task is to move the engine to Rust**, so that it runs about ten
> times faster and so that households can later have minds of their own
> (traits, memory and expectations deciding to sell or hoard, borrow, stay
> or leave), which numpy's one-rule-for-all columns can't do fast. The
> Python model stays as the **reference** the Rust engine is checked
> against; do not delete or rewrite it.
>
> Read first, in this order: `STATUS.md` (what is built, the state, the
> order of a month, randomness, invariants, conventions, known issues),
> `docs/RUST_HANDOFF.md` (this plan in detail), then `PLAN.md` where you
> need the reasoning or measurements behind a rule, and the Python modules
> as you port them (`simulation.py` is the month; `config.py` every
> parameter; `world.py` the state and setup; `checks.py` the invariants).
>
> Work in milestones, each on its own branch (`claude/rust-<milestone>`,
> each built on the last, branched from `claude/project-status`), each
> verified before it is called done, and each written up with its
> measurements in a "Rust engine" section of `PLAN.md`:
>
> 1. **Scaffolding:** a Cargo workspace in `rust/`; the parameters exported
>    from Python's `Config` to JSON and read with serde (Python stays the
>    one source of numbers); golden fixtures of Python's deterministic
>    functions.
> 2. **The state and setup**, matching Python's starting state in
>    distribution.
> 3. **One village's month**, rule by rule in the same order as
>    `Simulation.step`, with the invariants checked every month and
>    statistical parity with Python on the village settings.
> 4. **The map**: geography, weather, towns, the trade network, wool,
>    terrain, migration on a map; parity on the region settings.
> 5. **The runner and CLI**: paired what-ifs over many seeds in parallel
>    (rayon), the same CSV outputs as Python, sanity and stress checks.
> 6. **Speed**: benchmarks at 100,000 to 10,000,000 people against the
>    targets in the plan.
> 7. **Households with minds**, as an option next to the ported rules,
>    compared with them by paired runs.
>
> Then carry on with Phase 3 (step 5, regional migration, and on) in Rust.
>
> Rules that hold throughout: exact, number-for-number agreement with
> Python is not the goal (numpy's random draws can't be reproduced
> cheaply), **statistical** agreement is, plus exact agreement on
> deterministic functions and on invariants (land, debts, coins and people
> conserved; nothing negative or NaN). Keep one named random stream per
> rule, so a paired run shares its baseline's luck until the event. Results
> must be identical whatever the number of threads. Keep the project's
> style: plain everyday names (families, grain, rations, coins, carts),
> comments that say why, tests named as sentences, parameters only in the
> config. Run `cargo fmt`, `cargo clippy -- -D warnings` and `cargo test`
> before every push, and the Python tests too when you touch Python. Push
> with `git push -u origin <branch>` (retry up to 4 times, 2/4/8/16 s, on
> network errors), keep `claude/busy-mendel-edwts2` fast-forwarded to the
> latest work, end commit messages with the trailer lines your session
> gives, put no model names in commits or code, and open no pull request
> unless asked. When a choice is the user's to make (dropping a feature,
> changing what a result means), ask; otherwise choose, say what you chose
> in PLAN.md, and carry on. Report results faithfully, including parity
> failures you could not close.

## The plan in detail

### Why Rust, and what was measured

A toy decision rule per person (an expected price learned from what was
seen, fear from hunger, then sell, hoard or flee), one core:

| | ns per person-month |
|---|---|
| Python, a loop over objects | 770 |
| numpy columns (how the model is written) | 32 |
| Rust, a loop over structs | 8.5 |

numpy is fine while everyone follows one rule on whole columns. Decisions
that differ by person, with branches and memory, are where Python loses,
and they are what "fewer people, each with a mind" needs. Rust over C:
memory safety, `rayon`, `serde`, `cargo test`, and PyO3 if Python ever has
to call it. The model today: a month of 1,150,000 people (1,000 villages,
40 towns) takes 2.1 s on one core in Python.

### Layout

```
rust/
  Cargo.toml              # workspace
  econ-core/              # library: config, state, rules, the month, checks
    src/config.rs         # serde mirror of Python's Config
    src/rng.rs            # named streams
    src/population.rs     # people, struct of arrays
    src/households.rs ... # one module per Python module, same names
    src/simulation.rs     # the month, same steps in the same order
    src/checks.rs         # invariants
    tests/                # fixtures, parity smoke tests, invariants
  econ-cli/               # binary: run, what-if, sanity, stress, bench
  configs/                # JSON exported from Python (checked in)
  fixtures/               # golden inputs/outputs of deterministic functions
scripts/export_config.py  # Python Config (or a generated region) -> JSON
scripts/export_fixtures.py
scripts/parity.py         # runs both engines, compares, prints a table
```

Keep Python's module names and function names where the Rust allows, so
anyone can read the two side by side. A PyO3 crate (`econ-py`) is optional
and can wait until Python needs to drive the Rust engine.

### 1. Scaffolding
- `scripts/export_config.py`: `dataclasses.asdict(Config())` serialises to
  JSON as it is (about 11 KB; a generated 100-village region is about
  30 KB: villages with x, y, terrain, kind and border, plus roads and
  rivers). Export the default village, the settings the sanity and stress
  checks use, and regions from `geography.region(RegionSettings(...))`.
- Rust config structs with `#[serde(deny_unknown_fields)]`, so a parameter
  added in Python and not in Rust fails loudly. A Python test that the
  checked-in JSON equals a fresh export, so they never drift.
- **Read generated regions from JSON** rather than porting the region
  generator first (its layout comes from numpy's draws); port it later if
  needed.
- `scripts/export_fixtures.py`: inputs and outputs of the deterministic
  functions, for Rust tests to match to a relative 1e-9:
  `geography.carriage`, `geography.town_routes`, `towns.delivered_price`,
  `towns.from_neighbours`, `towns.store_markup`, `towns.carts`,
  `towns.settle`, `towns.between_towns`, `towns.border_trade`,
  `migration.appeal`, `lords.sell_in_town`, `lords.pay_servants`, the
  harvest sharing in `farms.py`, the market clearing in `market.py`, the
  credit rules in `credit.py`, and whatever else turns out to be pure.
  The existing tests in `tests/test_towns.py`, `test_network.py`,
  `test_geography.py` and the rest are also a list of behaviours to port as
  Rust tests.

### 2. The state and setup
- Struct of arrays, as in Python: people (`count`, `age_months`, `female`,
  `health`, `skill`, `location`, `household`, `job`, `married`, `couple`),
  households (`location`, `money`, `land`, `grain`, `animals`, `debt`,
  `lent`, `kin`), per-place and place x product arrays. `f64` everywhere at
  first (Python is float64); `u32` indices. Keep `count` (the weighted
  sample of step 12 needs it). Drop emptied rows once a month, as Python
  does.
- `create_world` ported; compare starting distributions with Python's over
  many seeds: ages, household sizes, holdings, animals, coins, jobs,
  prices, wages, woods.

### 3. One village's month
- Port `Simulation.step` in order (STATUS.md lists the eleven steps).
  Port the no-map path first; keep `if has_towns` branches as they are in
  Python so the map can follow.
- **Randomness:** one stream per rule, seeded from `(seed, crc32(name))`
  as Python does, with a good generator (`rand_pcg` or `rand_xoshiro`) and
  `rand_distr` for binomial, normal and the rest. Keep Python's stream
  names. Draws will differ from numpy's; that is expected.
- **Invariants** (`checks.rs`) every month in tests and in debug runs, and
  coins and people conserved, as in `tests/test_towns.py`'s "keeps the
  books" tests.
- **Statistical parity** (`scripts/parity.py`): for each setting, run both
  engines over the same number of seeds (at least 100 for a village) and
  compare per-run totals and monthly means: population at the end,
  births, deaths, emigrants and immigrants, the grain price, wage,
  ration, the poorest fifth's ration, underfed share, health, treasury,
  reserve; and for paired settings the event's effect (extra deaths, the
  price peak). Pass when each difference is within 3 standard errors
  (Welch) and, for the large ones, within a few percent. Settings: the
  default village; `drought@4` paired; no council; no lord; no town; no
  migration; a small village; a 40-year run (drift). Print a table and keep
  it in PLAN.md.
- Where parity fails, find the rule (compare month-by-month means, then
  the rule's own inputs and outputs) rather than tuning parameters.

### 4. The map
- `Geography` (read from the exported region; carriage and routes by
  Dijkstra, checked against fixtures), the weather field (random Fourier
  features, 256 waves, scale `map.weather_km`), events by reach (village,
  weather, region), `MAP_ONLY_EFFECTS`, scheduled events `near` a point,
  towns, consignment carts, trade between towns, the border, lords in town,
  wool in June, terrain, migration by appeal with distance and towns.
- Parity on: a region of 20 villages, 10 years; a region with a city and 3
  towns; a 100-village region with a drought over the west
  (`drought@4@0,12,11`-style); per place as well as in total (deaths in
  struck villages, in towns, elsewhere; grain sent to town by terrain; wool
  coins).

### 5. The runner and CLI
- `econ-cli` with the Python CLI's flags where they make sense
  (`--months --runs --seed --event NAME@MONTH[@X,Y,KM] --region --towns
  --map-seed --no-council ... --workers --out`), writing the same
  `run1.csv`, `run1_with_event.csv`, `summary.csv`, `places.csv` and
  `log.txt`, with the same columns, so Python's charts and analysis read
  them unchanged.
- Runs and both arms of a pair in parallel with rayon; a run itself may
  later go parallel over places (then draw from per-place streams so
  results don't depend on the number of threads; test that 1 and 4 threads
  give identical records).
- `sanity` and `stress` subcommands with the settings and thresholds of
  `scripts/sanity_check.py` and `scripts/stress_test.py` (the disaster
  allowance is a binomial test; read how it is done there).

### 6. Speed
- `bench` subcommand like `scripts/benchmark.py`. Targets: a month of
  1,000,000 people in under 0.2 s on one core (ten times Python); a paired
  three-year run of 10,000,000 people in under 5 minutes on 4 cores (the
  Phase 3 country target), without sampling if the memory allows (about
  300 MB per million people in Python; aim lower). Profile before
  optimising; keep the code readable.

### 7. Households with minds
- Give each household (not each person) a mind: traits drawn at setup
  (patience, caution, thrift, nerve, how fast it learns, how attached to
  its place), a memory (hunger, prices it has seen, debts, losses), and
  expectations (next months' grain price, next harvest). It decides how
  much grain to sell or hoard, whether to borrow or lend, buy or sell
  animals and land, and whether to stay or leave and where (by what it
  knows: kin, neighbours, word that travels with distance).
- Behind one interface: the ported Python rules are one kind of mind (the
  default), the new minds another, so a setting can switch between them
  and paired runs can compare them.
- Measure what minds change: hoarding in a dearth (did it raise prices in
  1315?), runs on lenders, flight; and what they cost in speed. Then decide
  with the user whether minds become the default.

### What the Python model teaches (pitfalls already met)
- Money must be conserved: every coin that leaves or enters the region is
  booked in the outside purse (`world.town.purse`); grain dealers' coins
  count as money. Tests check it every month.
- A town's grain is priced from its cheapest source (villages plus
  carriage, or a neighbouring town plus the route), with a mark-up as the
  dealers' store runs low; without the ramps on cart loads, towns boom and
  bust.
- Families leaving a village can't host newcomers in the same month, or
  people are counted twice.
- Townsfolk never farm; farmers who move to a town lose their job.
- Divisions by zero are guarded everywhere in Python with `np.divide(...,
  where=...)`; Rust must guard them explicitly.
- Events that exist only on a map must not be rolled without one, or
  village runs change.
