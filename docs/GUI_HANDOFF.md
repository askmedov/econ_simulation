# Watching the country evolve: the hand-off for a viewer

**Built** on `claude/rust-viewer`: `econ-sim view`, a page that replays a
run and its pair (PLAN.md, "Rust engine", section 16). What follows is the
hand-off it was built from.

Two parts: **the prompt** to give the next agent (copy it as it is), and
**the detail** it points to. Read [STATUS.md](../STATUS.md) first for what
the model is and how it is checked.

## The prompt

> You are continuing the project in `askmedov/econ_simulation`: a
> month-by-month simulation of a pre-industrial economy (England c. 1300):
> peasant families with land, grain, animals, coins and debts, village
> trades, lords and the state, market towns, a city, trade between them and
> beyond, and resources (iron, lead, salt, clay) made in some places and
> carried to all. It runs in Rust (`rust/`, binary `econ-sim`) on regions
> of 100,000, a million (the standard) or ten million people; the Python
> model (`econ_sim/`) stays the reference and the one source of settings.
> Runs need not be longer than ten years, and they start from many states
> (`econ-sim starts`, `--start`, `--set`, `--scale`), with or without a
> forced event (`--event drought@4`).
>
> **Your task is a GUI to watch how the country evolves**: a run, month by
> month over up to ten years, on a map of its villages and towns, with the
> region's measures over time and its chronicle (the log ranked by
> importance) alongside, so the user can see where famine spreads, where
> people leave for, what grain, tools and salt cost, how the towns and the
> iron hills fare, and what a forced event or a starting point changes.
>
> Read first, in this order: `STATUS.md`, `docs/GUI_HANDOFF.md` (this task
> in detail: what the engine already exports, a suggested design, the
> sizes to plan for, how to check it), then `PLAN.md`, "Rust engine",
> sections 12 to 15 (starting points, the chronicle, the crafts) where you
> need what a measure means.
>
> **Ask the user first** where they want to look at it, since it decides
> the design: a page they open from this cloud session (published as a
> private claude.ai page with the Artifact tool, or a file they open), or
> an app on their own computer. A cloud container can't show a desktop
> window or serve a local port to them. Recommend a self-contained web
> page that replays finished runs. Also ask which measures matter most to
> them on the map.
>
> **The rules of the project still hold:**
> - The viewer must not change any result. New measures are read-only. A
>   run's records must stay the same, month for month: check against the
>   previous commit's binary, as PLAN.md sections 14 and 15 did.
> - Before every push, run `cargo fmt`,
>   `cargo clippy --release --all-targets -- -D warnings` and
>   `cargo test --release` in `rust/`. When Python is touched, also run
>   `.venv/bin/python -m pytest -q`.
> - Write plain names, comments that say why, and tests named as
>   sentences. Match the surrounding code.
> - Write the step up as PLAN.md "Rust engine" section 16, and update
>   STATUS.md.
> - Work on a step branch (e.g. `claude/rust-viewer`), push it and your
>   session's designated branch, and fast-forward
>   `claude/busy-mendel-edwts2`, the hand-off branch. Push with
>   `git push -u origin <branch>`, retrying after 2, 4, 8 and 16 s on
>   network errors.
> - End commits with the attribution lines your session gives you. Never
>   put a model's name in commits or code. Open no pull request unless
>   asked.
> - Check the page in a real browser before calling it done.
>   Chromium and Playwright are preinstalled (`require('playwright')`,
>   `chromium.launch()`). Take screenshots and look at them.
>
> Start from commit `c641152` (branch `claude/rust-more-crafts`, also on
> `claude/busy-mendel-edwts2`), where 162 Rust and 312 Python tests pass.

## The detail

### What the engine already gives a viewer

Everything below comes from `econ-sim` (build it with
`cargo build --release` in `rust/`; it embeds the configs in
`rust/configs/` when it is built, so rebuild after re-exporting them).

| Source | What it holds | Notes |
|---|---|---|
| `econ-sim config --size 100k` | The region as JSON. Each place (`villages`) has `name`, `kind` (village or town), `terrain`, `x` and `y` in km, `population` at the start, `resources`, `border` (trades beyond the region) and `stronghold` (the city). `map.roads` and `map.rivers` are pairs of place numbers. | The standard regions are `rust/configs/regions/region100k.json` and `region1m.json`. `region10m.json` (7 MB) is not in git: make it with `.venv/bin/python scripts/export_config.py --large`. |
| `econ-sim places` | Per run, one JSON line: `{"arm", "seed", "places": {series: [month][place]}}`. | Eight series per place a month (`PLACE_SERIES` in `simulation.rs`): population, births, deaths, ration (share of food need eaten), food_price, left (people who left), sent (grain sent to the town), wool. |
| `econ-sim records` | Every month of the region as CSV: 127 columns. | People, births, deaths, migration, food eaten and needed, prices of every good (`price_food`, `price_tools`, `price_salt`, `price_pottery`...), workers by trade (`jobs_*`), `wage` and `wage_cover`, `staples_bought` (share of their salt families got), `crafts_sold` and `wool_sold` (coins from beyond), woods, soil, and more. `metrics.rs`, `MonthRecord`, says what each is. |
| `log.json` (written by a plain run with `--out DIR`) | The chronicle, keyed by arm: entries with `month_number`, `year`, `month`, `rank` (1 most important to 4), `kind`, `what`, `people`, and `places` (the worst few, each with a line of detail) plus `more_places`. | `econ-sim logs` prints it, and `log.txt` holds it as text. Entries name places by name. |
| `econ-sim starts --out DIR` | Several starting points side by side; `starts.csv`. | `--only standard,crowded` picks them; `--list` lists all 17. |

The engine is a library too (`econ_core::Simulation`): `Simulation::new`,
`step()` for one month, and the whole state in `sim.world` (per place:
`world.prices`, `world.stock`, `world.woods`, workers by trade from
`world.population`, `world.geo` with `x`, `y`, `market` (each village's
town), `town`, `border` and `routes` between towns). A new subcommand can
read whatever it needs from there each month.

### A suggested design (to settle with the user)

- **Replay finished runs** rather than simulate in the browser. A run of
  ten years at 100,000 takes 5 s and at a million about a minute, so the
  page needs no engine of its own. (`econ-core` could later be built for
  WebAssembly to run live, but it uses rayon; leave that for after.)
- **One export step** in the CLI, e.g. `econ-sim view --size 100k --months
  120 [--event ...] [--start ...] --out viewer.html`. It runs the
  simulation, gathers the map, the per-place series, the region's measures
  and the chronicle into one compact data object, and writes a
  self-contained HTML page with the data embedded. Keep the page's code
  in its own files in the repository (e.g. `rust/econ-cli/viewer/`,
  included with `include_str!`), and keep generated pages out of git.
- **What the page shows:**
  - **A map** of the places at their `x`, `y`: villages as dots sized by
    people, towns larger, the city and the border towns marked, roads and
    the river drawn, terrain shaded, and the resources marked (iron, lead,
    salt, clay).
  - **Colour by a chosen measure:** people against the start, food eaten
    (famine in red), grain price, deaths, people leaving, and, if you add
    them, woods, wages, salt got, or the price of tools.
  - **A month slider** with play and pause, and the year and month shown.
  - **The region's charts**, following the slider: people, grain, tools
    and salt prices, wages, exports, and workers by trade.
  - **The chronicle** of the month and the year in brief, ranked.
    Choosing an entry lights up its places on the map.
  - **Choosing a place** shows its own series and what it has (its
    resources, its town).
  - **Comparing:** a paired what-if (with and without the event, or the
    difference between them on the map), and two starting points side by
    side. The product of the project is the paired what-if, so this
    matters.
- **More per-place measures** would make the map tell more: woods, wage,
  salt got, tools price, workers in the resource trades, the famine flag.
  Add them as read-only series (extend `PLACE_SERIES`, or gather them in
  the new subcommand) and check that every run's records stay the same.

### Sizes to plan for

- At 100,000: 250 places, so ten years of 10 series is 300,000 numbers. That
  is small, even as plain JSON.
- At a million: 2,558 places, about 3 million numbers. Round or quantise
  them (to three significant figures, or 16-bit integers with a scale per
  series) to keep the page to a few MB. A claude.ai page may be at most
  16 MB.
- At ten million: 25,584 places. Show the 984 towns with their countryside
  added together, or sample, rather than every village.
- The map needs canvas or WebGL drawing beyond a few thousand points; SVG
  is fine at 100,000.

### How to check it

- **A Rust test** for the export: the data names every place, has a value
  for every month, and its region totals agree with the records. The
  records of a run stay the same with the viewer's measures added.
- **In the browser** with Playwright: load the page, play a few months,
  choose a measure, a place and a chronicle entry, check the console
  shows no errors, and take screenshots at a few months. Look at them; a
  page that loads can still draw nothing.
- **Publishing:** if the user wants a page they can open from here, use the
  Artifact tool. Load its `artifact-design` skill before writing the page.
  Its rules: scripts only from cdnjs, jsdelivr or unpkg, with exact
  versions; colours as tokens with a dark mode; and it must work at phone
  width.

### Pitfalls already met

- `econ-sim` embeds `rust/configs/*.json` at build time: after changing
  `econ_sim/config.py`, run `.venv/bin/python scripts/export_config.py`
  (and `scripts/export_fixtures.py` if a test fixture depends on it), then
  rebuild.
- Prices in the records are a plain mean over places. Weight by people
  when a mean should be over people.
- Resources are laid out in patches (`RegionSettings.deposits`). Each
  town that is nearest to a village with a resource has it too.
- `scripts/parity.py` turns off minds, the garrison and the crafts to
  compare with Python; the viewer has nothing to do with parity.
- Runs need not be longer than ten years (the user's direction), and the
  sizes are settled for now (100,000, a million, ten million).
