# Plan

## Goal

A month-by-month economic simulation for asking **"what happens over the
next 12–36 months (60 at most) if this event hits?"** It starts as a village
of 100 people with basic needs, and is built so it can grow into a country of
millions with many kinds of events and businesses.

Long runs (decades) are only a sanity check that the model doesn't explode or
collapse. The product is the near-term what-if.

## Design principles

1. **People live in a table, not as objects.** One array per attribute (age,
   sex, health, skill, location). Each row has a `count`: today every row is
   1 person, later a row can stand for thousands of similar people. Every rule
   is written against counts, so batching is a data change, not a rewrite.
2. **Rules work on whole columns at once.** "How many of this row die" is a
   single binomial draw, which is statistically the same as rolling once per
   person. A row with count 1 is just an ordinary roll.
3. **Groups split when an event hits some of their members** (30 of 5,000
   fall ill → two rows). Merging similar rows back together comes later, when
   we actually batch people.
4. **One random stream per rule**, all derived from the seed. Adding a rule
   doesn't change other rules' draws, and paired what-if runs share the same
   luck until the event starts (see below).
5. **Events are data**: scope (village or person), chance, season, duration,
   group (mutually exclusive events) and effects as multipliers or deltas.
   The 50th event is a new entry, not new code.
6. **Everything has a location** (village index today; region and country
   later). Events and food stores are per village.
7. **Totals, not per-person logs.** Monthly statistics are sums and averages
   over the table.
8. **All numbers live in `econ_sim/config.py`.**

Later, at scale: trade goes through markets (everyone posts supply and
demand, a price clears it) rather than pairwise matching, and business types
are recipes (inputs → outputs) defined as data.

## How a what-if works

- A **scenario** forces events at given months (e.g. a drought in month 4).
- Each scenario run is **paired** with a baseline run on the same seed. Both
  share the same random draws until the forced event starts, so the
  difference between them is the event's effect, not luck. If the baseline
  happened to get the same event anyway, the pair shows no effect, which is
  the right counterfactual.
- Repeating over many seeds gives the **average effect and its range**
  (10th–90th percentile).
- The starting calendar month is configurable: a drought before the harvest
  is very different from one after it.

## Phase 1: a village that feeds itself (built)

- **People:** age, sex, health (0–100), skill, village.
- **One good, food,** counted in rations (one adult for one month); children
  eat 0.6.
- **Production:** working-age people (15–59) farm. Output = base × season ×
  events × labour^0.7 × land^0.3: diminishing returns on fixed land cap the
  population. Most food comes at the autumn harvest, so stores must last the
  winter. Weak people work less.
- **Shared granary per village,** 2% spoilage a month.
- **Rationing with foresight:** each month the village eats the largest
  steady ration that keeps stock plus expected harvests from running out
  over the next 12 months. Expectations are a slowly moving average of
  output (about 10 months of memory), so one bad month doesn't cause panic
  but a drought does tighten belts early.
- **Health** drifts toward a level set by the ration (full rations → 100,
  half rations or less → 0), gaining up to 10 a month and losing faster the
  worse the shortage.
- **Deaths:** yearly risk by age, plus extra risk below health 50. **Births:**
  women 16–45, less likely when health is poor.
- **Events:** good weather / drought (April, one or the other), harsh winter,
  disease, granary fire, personal accidents. Any village event can also be
  forced at a chosen month.
- **Default village:** 100 people on 35 plots with 5 months of food. In normal
  years it roughly holds steady and is rarely short of food, so shocks show
  clearly.

### What a drought does (default village, April drought, 200 runs)

Short rations start in July and last about 15 months (lowest ration about
80%), average health falls from 99 to about 76, about 1 fewer birth and 1–2
extra deaths over 36 months. By the third harvest rations and health are
nearly back to normal, but stores are still about a quarter lower.

### Known simplifications

- The village shares food equally; no households or private stores yet.
- Hunger risk doesn't depend on age (famines really hit infants and the old
  hardest).
- The starting state is generated, not taken from real data.
- Parameters are plausible guesses, not calibrated to historical data.

## Roadmap

Each phase adds one economic idea, keeping the near-term what-if as the main
output.

2. **Households and private property.** Families with their own stores;
   children and elders depend on them. Inequality: in the same drought some
   families go hungry and others don't. A second need (firewood in winter)
   and a first division of labour.
3. **Trade and prices.** Barter, then money; local markets where prices rise
   when demand exceeds supply. Near-term question: how fast do food prices
   spike after a drought, and who is hurt?
4. **Land, tools and labour.** Ownership, wages, saving and lending: who
   sells land or borrows in a bad year.
5. **Government.** Taxes, a famine reserve, relief policy: "a drought hits in
   month 4: does opening the reserve in month 7 prevent the deaths?"
6. **Many villages → a country.** Regions, trade between them, migration;
   people batched into groups (`count` > 1) with split and merge; national
   money and inflation.
7. **Calibration and starting state.** Start from realistic data (age
   structure, stores, prices) so near-term forecasts mean something.
8. **Interactive dashboard** to pick events and compare scenarios.

## Open questions

- Which events and outcomes matter most for the near-term questions?
- At what point should the starting state come from real data?
