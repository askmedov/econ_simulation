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
  steady ration that keeps its stock (which keeps spoiling) plus expected
  harvests from running out over the next 12 months. Expected harvests are
  what today's workers would grow in normal health, times the effect of
  events already under way (villagers see a drought ruining this year's
  crop, but don't expect one next year). The plan also allows for hungry
  workers growing less.
- **Health** drifts toward a level set by the ration (full rations → 100,
  40% rations or less → 0), gaining up to 10 a month and losing faster the
  worse the shortage.
- **Deaths:** yearly risk by age, plus extra risk below health 50.
- **Births:** women 16–45, fewer when health is poor and when the land can
  barely feed everyone (people marry later: the "preventive check"). This
  keeps the village from growing until a drought tips it into famine.
- **Events:** good weather / drought (April, one or the other), harsh winter,
  disease, granary fire, personal accidents. Any village event can also be
  forced at a chosen month.
- **Default village:** 100 people on 35 plots with 5 months of food. In normal
  years it roughly holds steady and is rarely short of food, so shocks show
  clearly.

### What a drought does (default village, April drought, 200 runs)

Short rations start in summer and last about 13 months (lowest ration
about 86%), average health falls from about 99 to 76 and there are slightly
fewer births, but hardly any extra deaths: one bad harvest is hardship, not
famine. It takes a drought on top of disease, or two bad years in a row, to
push rations below half.

### Sanity checks

`scripts/sanity_check.py` runs hundreds (`--full`: thousands) of seeds
across settings: short and century-long runs, all twelve start months, tiny
and big villages, too little and too much land, every bad event at once,
empty granaries. It fails on impossible values (negative food, broken
accounting), on nonsense (hunger with a full granary, a year of hunger with
no cause, implausible birth rates, lopsided sex ratios), and on disasters
(starvation rations, 30% dying in a year) that happen more often than the
setting allows.

Fixed after the first sweep:
- Villagers judged next year's harvest by recent bad ones and kept
  themselves hungry long after a drought ended (a poverty trap).
- Planning ignored spoilage and how little weak workers grow, so granaries
  ran dry before the harvest.
- Population grew until people were already malnourished, so every century
  run ended in a famine that killed a third of the village.
- Starting villages could be lopsided between men and women by chance.

### Known simplifications

- The village shares food equally; no households or private stores yet.
- No coping beyond rationing (no wild foods, selling livestock or help
  from neighbours).
- Hunger risk doesn't depend on age (famines really hit infants and the old
  hardest).
- The starting state is generated, not taken from real data.
- Parameters are plausible guesses, not calibrated to historical data.

## Phase 2: a village economy (design)

A village of 1,000 people with families, money, several businesses, a
council that forms once the village is big enough, and basic healthcare.
The near-term what-if stays the main output, now with new questions: how
far do food prices rise after a drought, which families go hungry, does the
council's reserve help, does healthcare blunt an epidemic?

### Build order

Each step is its own branch on top of the previous one, and each leaves a
working, tested simulation. The order follows dependencies: markets need
families with money, taxes need money, healers need someone to pay them.

| Step | Branch | Adds |
|---|---|---|
| 1 | `claude/village-of-1000` | 1,000 people in families; jobs |
| 2 | `claude/money-and-markets` | coins, wages, a food market with a moving price |
| 3 | `claude/businesses-and-products` | firewood, clothing, tools; people move to better-paid trades |
| 4 | `claude/village-administration` | a council forms: taxes, officials, food reserve, famine relief |
| 5 | `claude/basic-healthcare` | healers paid by the council treat the sickest |
| 6 | `claude/phase2-reporting` | prices, wages, inequality and public finances in the CLI, chart and checks |

### 1. A village of 1,000 in families

- People get two new columns: `household` and `job`.
- **Families at the start:** each woman aged 18–49 heads a household;
  men pair with women of similar age; children join a woman old enough to
  be their mother; older people live with a family. About 220 households of
  around 4–5 people.
- Newborns join their mother's household. When a household has nobody
  left, its savings pass to the remaining families (or the council, once
  there is one).
- **Jobs:** working-age people (15–59) have a job; children and the elderly
  don't. Young people pick a trade when they turn 15.
- Land is scaled so 1,000 people sit at the same food margin as the
  100-person village.

### 2. Money and a food market

- **Families hold coins.** Each month they are paid wages and buy food at
  the market price.
- **Farms** (all farmers together, as one business for now) own the
  harvest store. Each month they offer what the existing rationing plan
  allows, so a poor harvest means less on the market. They pay out their
  takings as wages, by skill and health, keeping a small cash buffer.
- **The price** rises when families want more than is on offer and falls
  when food goes unsold, by at most 15% a month.
- **Families buy what they need if they can afford it.** If food is short,
  everyone gets the same share of what they asked for; if a family runs
  out of money, it eats less. Each person's health follows their family's
  food, so in the same drought some families go hungry and others don't.
- Money is never created or destroyed: the total is checked every month.

### 3. Businesses and products

Products and business types are data, like events:

| Business | Makes | Notes |
|---|---|---|
| Farming | food | land, seasons, tools raise output |
| Woodcutting | firewood | tools raise output |
| Weaving | clothing | |
| Smithing | tools | burns firewood |

- **Needs:** food every month; firewood mostly in winter (cold does
  damage health without it); clothing is a comfort bought with what's left
  over.
- **Families spend in order:** food, then firewood, then part of what's
  left on clothing, saving the rest.
- **Tools** make farmers and woodcutters more productive and wear out, so
  those businesses buy new ones from smiths.
- **People change trades** toward better pay: each month a few workers in
  a poorly paid trade move to the best-paid one. Specialisation follows
  prices instead of being set by hand.
- Events can hit one business: a drought hits farming, a new forest fire
  hits woodcutting, a harsh winter raises the need for firewood.

### 4. A council forms

- **When:** after the village has had 500 or more people for 6 months (so
  a 1,000-person village forms one early in a run, and a 100-person village
  never does).
- **Taxes:** 10% of wages, into a treasury.
- **Officials:** about 4 per 1,000 people, hired from the trades and paid
  a little above the average wage.
- **Food reserve:** in good times the council buys surplus grain until it
  holds about 2 months of the village's needs.
- **Famine relief:** families who can't buy enough food get it free from
  the reserve.
- What-if levers: tax rate, reserve size, relief on or off, or no council
  at all.

### 5. Basic healthcare

- **Healers** are hired and paid by the council: about 5 per 1,000 people.
- Each healer can see about 30 patients a month: the sickest first (health
  below 70), then infants.
- **Treatment** speeds recovery and lowers that month's risk of dying, which
  matters most in an epidemic or a famine.

### New measures

Prices of each product, average wage, workers in each trade, money held by
families, the treasury and the reserve, relief given, people treated, and
how the poorest fifth of families eat compared with the rest.

### Kept simple for now

- Each business type is one business per village (individual firms later).
- No new households form; families just grow.
- No lending, land ownership or rents.
- The council's rules are fixed policies, not decisions.

## Roadmap after Phase 2

3. **Land, credit and firms.** Land ownership and rents, lending and debt
   (who borrows or sells land in a bad year), individual firms instead of
   one business per trade.
4. **Many villages → a country.** Regions, trade between them, migration;
   people batched into groups (`count` > 1) with split and merge; national
   money and inflation.
5. **Calibration and starting state.** Start from realistic data (age
   structure, stores, prices) so near-term forecasts mean something.
6. **Interactive dashboard** to pick events and compare scenarios.

## Open questions

- Which events and outcomes matter most for the near-term questions?
- At what point should the starting state come from real data?
