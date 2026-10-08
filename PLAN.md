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

## Phase 2: a village economy (built)

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

### 1. A village of 1,000 in families (built)

- People get two new columns: `household` and `job`.
- **Families at the start:** each married woman aged 18–64 heads a
  household; men pair with women of similar age; children, and young
  people not yet married, join a woman old enough to be their mother;
  older people live with a family. About 200 households of around 5.
- **Marriage:** each month a single woman of 17–34 marries with a chance
  of 1 in 48 (on average at about 21), to a single man of 19–39 from
  another family. The first of a family to marry stays with their spouse
  as its heir; later brothers and sisters set up a household of their own,
  taking their share of the family's savings. People wait to marry when a
  wage can barely feed a family (see births below).
- Newborns join their mother's household. When a household has nobody
  left, its savings pass to the remaining families (or the council, once
  there is one).
- **Jobs:** working-age people (15–59) have a job; children and the elderly
  don't. Young people pick a trade when they turn 15.
- Land is scaled so 1,000 people sit at the same food margin as the
  100-person village.

### 2. Money and a food market (built)

- **Families hold coins.** Each month they are paid wages and buy food at
  the market price.
- **Farms** (all farmers together, as one business for now) own the
  harvest store. Each month they offer what the existing rationing plan
  allows, so a poor harvest means less on the market. They pay out their
  takings as wages, by skill and health.
- **Families buy what they need if they can afford it.** If food is short,
  everyone gets the same share of what they asked for; if a family runs
  out of money, it eats less. Each person's health follows their family's
  food, so in the same drought some families go hungry and others don't.
- **Neighbours help:** families with more than two months of food money
  saved give a quarter of the excess each month to families who can't
  afford food. Without this, families with many children and few earners
  went hungry next to full purses even in normal years.
- Money is never created or destroyed: the total is checked every month.

### 3. Businesses and products (built)

Products and business types are data, like events:

| Business | Makes | Notes |
|---|---|---|
| Farming | food | land, seasons, tools raise output |
| Woodcutting | firewood | tools raise output |
| Weaving | clothing | |
| Smithing | tools | burns firewood |

- **Needs:** food every month; firewood for cooking and, mostly, winter
  heating (cold harms health without it); clothing is a comfort.
- **Families spend in order:** food, then firewood; they keep about three
  months of those costs as savings and spend 30% of anything above that
  on clothing each month. Comforts are open-ended: the better off the
  village, the more it buys.
- **Tools** make farmers and woodcutters more productive and wear out, so
  those businesses buy new ones from smiths. Businesses set aside a
  month's cost of supplies and tools before paying wages.
- **Prices** are a fair price (a customary wage over output per worker,
  plus supplies) times a mark-up between half and triple. For most goods
  the mark-up rises when orders outrun what is made and falls when goods
  pile up. Grain follows how well stores plus expected harvests cover the
  coming year, by the King–Davenant law (a harvest 10% short raises grain
  prices ~30%, 20% short ~80%).
- **Who works where:** each trade aims for workers in proportion to the
  work its orders need, nudged up when its goods sell dear and down when
  cheap; necessities (food, firewood, tools) are staffed first and
  comforts share whoever is left. A few percent of the gap moves each
  month. Businesses also work less when unsold goods pile up.
- **Births** come from married women, so they follow marriage, and
  marriage follows the real wage: how much food a month's pay buys,
  against the food need per worker. Dear grain or crowded land lowers it,
  and at worst a fifth as many people marry. (Europe's "preventive check".)
- Events can hit one business: a drought hits farming, a new forest fire
  hits woodcutting, a harsh winter raises the need for firewood.

Simpler rules were tried first and failed the sanity checks: pure
supply-and-demand prices compounded into 20x swings and collapses, and
workers chasing the best pay stampeded into whichever trade was briefly
dear (once leaving the village to starve). The rules above are the stable
ones.

### 4. A council forms (built)

- **When:** after the village has had 500 or more people for 6 months. A
  village already that big at the start has had a council for years:
  officials in place, a full reserve and a working treasury. A 100-person
  village never forms one; a 520-person village forms one in its sixth month.
- **Officials:** about 4 per 1,000 people, hired from the trades and paid
  20% above the average wage.
- **Taxes:** 10% of wages, only while the treasury holds less than six
  months of running costs, and never in a famine year (grain on hand plus
  expected harvests short of the year's need).
- **Food reserve:** filled by a grain levy (a tithe in kind) of 5% of each
  harvest, until it holds 2 months of the village's needs; no levy in a
  famine.
- **Famine relief:** families who can't buy enough food get the rest free
  from the reserve; families who can't afford food and firewood get coins
  from whatever the treasury holds beyond two months of running costs.
- **Savings** of families who die out go to the council.
- What-if levers: `--no-council`, `--no-relief`, `--council-forms-later`
  (start without one and watch it form), and in config the tax rate, levy
  and reserve size.

First versions failed instructively: a council that kept taxing through a
famine to fill its reserve (valued at famine prices) hoarded half the
village's money while the poor starved, and one that bought its reserve
with coins sat on a third of the money for months. Suspending taxes in
famine and filling the reserve in kind fixed both.

**What it does** (April drought, 36 months, 30 runs): with the council,
about 2 extra deaths, 5 fewer births, the poorest fifth's worst month at
92% of need and 2 hungry months; without it, about 6 extra deaths, 17 fewer
births (people put off marrying), 69% and 16 months.

### 5. Basic healthcare (built)

- **Healers** are hired and paid by the council: about 5 per 1,000 people,
  paid like officials. No council, no healers.
- **Triage:** each healer can see about 30 patients a month, and they see
  the people most likely to die first: infants, the old, the starving and
  anyone hit by an outbreak. (Seeing the sickest by health first missed
  most outbreak victims, whose risk rises before their health falls.)
- **Treatment** adds 10 health and cuts that month's risk of dying by 15%
  (it was 40%, which made a pre-industrial village nearly modern; healers
  before vaccines and clean water saved few lives).
- What-if lever: `--no-healthcare`.

**What it does** (24 months, 40 runs): deaths of about 25.5 against 27.6 per
1,000 a year in normal years, and 29.3 against 32.0 with an epidemic.

### New measures (built)

Every month records, besides Phase 1's: the price of each product, workers
in each trade, the average wage, money held by families, businesses and
the council, taxes, the grain reserve and relief given, healers and people
treated, how warm families kept, clothing bought, how much food a wage
buys, how well stores cover the coming year, and how the poorest fifth of
people ate compared with the village. The overview chart shows food in
store, food price, health, food need met for the village and for its
poorest fifth, and deaths; comparisons report the peak food price, the
poorest fifth's worst month, relief given and people treated.

### Kept simple for now

- Each business type is one business per village (individual firms later).
- No lending or rents (land holding came in Phase 2b).
- The council's rules are fixed policies, not decisions.

## Stress test: does anything drift over decades? (built)

The what-ifs look 1–5 years ahead, but a slow drift there is a fast one
somewhere else, so `scripts/stress_test.py` runs 55 scenarios (baselines,
repeated shocks, policies, starting conditions, and every key setting
halved and doubled) for 40 years with 6 seeds each (`--full`: 100 years, 16
seeds) and flags any measure that leaves a plausible range, ends far from
where it started, or keeps trending: population, births, deaths and
weddings per 1,000, prices in days of work, how much food a wage buys,
rations for the village and its poorest fifth, warmth, health, the mix of
trades, how money is spread, food stores and family sizes.

**First run: 52 of 53 scenarios drifted.** What it found, and the fixes:

| Drift | Cause | Fix |
|---|---|---|
| Births and deaths of 12–19 per 1,000: a modern village, not an old one | Mortality too low, and healers cut the risk of the infants and old people they saw by 40% every month | A pre-industrial life table (a fifth of babies die in their first year, life expectancy at birth about 35); treatment cuts risk by 15% |
| Households never split: 195 families became 100, averaging 7 and up to 34 people | Nobody ever left home | Marriage: young people marry (women at about 21) and the first of a family to marry stays as its heir; the rest set up new households. Births come from married women |
| Population halves when the village starts with little money | Prices were reckoned in a fixed customary wage, so they could never fall to fit the money there was | The customary wage moves with the money: up while families hold more than 3 months of the village's earnings, down while they hold less |
| Families short of firewood every late winter, worse as the village grows | Woodcutters worked to this month's orders, and the summer's growing stock looked like a glut, so they cut back before winter | Woodcutters stock up ahead of winter (a seasonal buffer), plan by the year's average need, and price by whether the stock is on track; families who can't buy firewood gather up to half their need |
| Tool prices swinging between 1.3 and 13 | Smiths could spend only half the cash they had set aside for firewood, and farms counted the same missing tools as new orders every month | Businesses may spend what they set aside; farms order replacements plus a quarter of any gap |

Two fixes needed a second try. Letting the customary wage follow what work
actually paid made prices climb forever once land got crowded (grain sells
above its cost, so pay outruns the wage, so prices rise, so pay rises); and
tying it to families' savings against their food bill made prices sink
forever as the village got poorer, since no price level can make a real
shortage go away. Tying it to money against earnings (the quantity of money
sets the price level) has neither problem.

**Now: 0 of 55 scenarios drift.** Four flags are expected and reported as
such: without neighbourly help large poor families can't afford firewood;
with neither help nor a council they also go hungry and the village shrinks
by 40% over 40 years; on crowded land people marry late (births 24 per
1,000); with plenty of land the village grows 65%. The near-term sanity
sweep still passes.

**What the long runs show.** The village now behaves like a
pre-industrial one. Births of about 30 and deaths of about 25 per 1,000
make it grow just under 1% a year until land runs short, about 1,400 people
on today's 350 plots. Then grain gets dear, people marry later, more
people farm and the weavers all but disappear, as nobody has a coin to
spare. With repeated droughts or crowded land, deaths overtake births. Bad
years bring a mortality crisis: in a century-long run a third of seeds see
rations below half at least once, decades in.

This growth matters for what-ifs too: the baseline village grows about 4%
over 5 years, so an event's effect should always be read against the
paired baseline, as the CLI does.

## What a real village of 1,000 had that this one doesn't

Villages of this size have existed for some 9,000 years: a Neolithic
farming village in the Levant, a Mesopotamian or Egyptian village, a
Roman-era or Han Chinese village, a medieval English manor. Next to any of
them ours is a small modern market town in costume: everyone works for a
business for wages and buys everything with coins.

The big miss is structural. A real village was a set of farming households
inside a larger political economy. What decided how a bad year played out
over the next few years was **who held what** (land, seed, animals, grain,
debts) and **who took what** (rent, tax, tribute), more than prices. The
gaps below are ranked by how much they would change a 12–60 month what-if.

### 1. Families farmed their own land and ate their own grain

Most output never reached a market. A peasant family farmed its own (or
rented) plot with its own labour, kept its grain in its own store, spun
and wove its own cloth and gathered its own fuel; perhaps a tenth to a
third of output was sold. Markets mattered for the landless, the
craftsmen and the surplus.

Why it matters for a what-if: it decides **who goes hungry**. In a drought
a smallholder eats less of their own harvest whatever the price; the
landless labourer and the craftsman are hit twice, as grain gets dear and
demand for their work falls (Sen's "entitlement failure": the 1943 Bengal
famine happened with food in the region). In our model everyone meets a
drought through prices, so the vulnerable are simply the poorest in coins.
The stress test shows where a pure wage economy leads: without a council
or neighbours' help, families with many children and few earners starve
next to full granaries, and the village shrinks by 40% in 40 years.

### 2. Land: who holds it, who inherits it, who rents it

Land was the main asset, held by families, lords, temples or the
community (open fields; periodic redistribution in the Russian commune).
Rents or shares (a third to a half of the harvest for sharecroppers),
inheritance rules (one heir or split between sons), and "no holding, no
marriage" decided how many households there could be. Our land is one
number per village, and our marriage brake is a wage threshold standing
in for "a couple needs a holding".

### 3. Most of the surplus left the village

Tribute, rent, tithes, taxes in kind and labour service (corvée) to a
lord, temple, palace or state commonly took a fifth to half of the
harvest, and it left: to a town, an army, a court. Our council is a benign
local body taking 10% of wages and 5% of the grain and spending it all at
home. The real levers in a crisis were outside the village: rent
remission, tax postponement, a lord or temple opening its granary, or not.

### 4. Seed grain and livestock make shocks last

In rain-fed Europe yields were 3–6 grains harvested per grain sown, so a
fifth to a third of every harvest had to be kept as seed (irrigated
Mesopotamia did far better). A hungry family that eats its seed, or sells
its oxen, shrinks next year's harvest: one reason famines often ran two
or three years. Livestock were also the plough team, the manure that
kept fields fertile, and the family's savings account. Our harvest does
not depend on last year's choices, so recovery is too quick.

### 5. Debt, distress sales and bondage

In a bad year families borrowed grain (at a third interest, the legal
limit in Hammurabi's laws), then sold animals, then land, then labour, and
sometimes themselves or their children into debt bondage. Kings
periodically cancelled debts (Hammurabi's edicts, Solon's
"shaking off of burdens"). This is the main way a one-year shock turns
into lasting inequality. We have no borrowing; neighbours simply give.

### 6. The village was never alone

Metal, salt and pottery came from outside; surplus grain went to a town
market; prices were regional. A local drought raises prices less if grain
can come in, and more if roads are bad or soldiers requisition it. Young
people left to serve in other households or move to town; raids, wars and
disease came in from outside. Our villages trade with nobody and nobody
migrates.

### 7. The year had a shape, and so did the family's work

Harvest needed every hand, women and children included; winter was for
crafts, repairs and building. Women's work (spinning, weaving, brewing,
food processing, childcare) was as large as men's and almost entirely
unpaid. Children worked from about seven. Our people hold one job all
year, children don't work and men and women are interchangeable. One
result: when land gets crowded every worker goes to farming and our
village stops making cloth altogether, where a real one kept spinning and
weaving at home.

### 8. Insurance was social, not financial

Kin networks, reciprocity between neighbours, patrons, temple or church
charity, scattered strips of land in several fields (so hail never
ruined one family completely) and household grain stores of a year or
more. We have a community granary run by "the farming business",
neighbourly gifts in coin, and a council.

### 9. Money was scarce and came from outside

Many ancient villages used little coin: barley or silver by weight as the
unit of account, payments in kind, tallies of credit. Coin came in when
surplus was sold to a town and left as taxes; a tax demanded in coin
forced peasants to sell grain at harvest when it was cheapest. Our money
is a fixed, closed stock: prices now adjust to it, but coin never arrives
by selling grain to a town or leaves as taxes.

### 10. Environment

Soil fertility and fallow, irrigation and salt (Mesopotamia), woods that
shrink when cut (ours regrow without limit), multi-year droughts and
volcanic winters (our weather has no memory).

### Already closer than before

The stress test pushed the demography toward a pre-industrial one: a fifth
of babies die in their first year, life expectancy at birth is about 35,
births run about 30 and deaths about 25 per 1,000 in ordinary years with
crisis years far higher, people marry around 21 and later when times are
hard, and healers save few lives. Ancient villages were often harsher
still (life expectancy 20–30), which the life table can be set to.

### What to build next

Being built as Phase 2b, below. Two of these change near-term what-ifs the most and fit together:
**peasant households that farm their own plots** (with land held by
households, seed kept back, and the landless working for wages) and
**debt and distress sales** (grain loans, then selling animals and land).
Then a **lord or state** that takes rent and tax out of the village, and
**a regional market** for grain with a transport cost. Choosing a time and
place would let parameters come from records: medieval or early modern
England has the best data (prices and wages, manorial accounts, parish
registers), Roman Egypt has census returns and contracts, Old Babylonian
Mesopotamia has loan and land contracts.

## Phase 2b: a peasant village (in progress)

The gaps above, built into the Phase 2 village one branch at a time, each
tested, sanity-checked and stress-tested:

| Step | Branch | Adds |
|---|---|---|
| 1 | `claude/peasant-farms` | families hold land and grain; the harvest is shared out in kind; a grain market among families (built) |
| 2 | `claude/seed-and-livestock` | seed kept back from each harvest; plough animals, herds and their losses; a high-pressure demography (built) |
| 3 | `claude/debt-and-distress` | grain and coin loans, then sales of animals and land, in bad years |
| 4 | `claude/lord-and-state` | a lord's land and rent in kind, taxes in coin, the lord's granary |
| 5 | `claude/regional-market` | a town grain price with a transport cost; people leave and arrive |
| 6 | `claude/household-work` | everyone at harvest, women's spinning and weaving at home, kin who help |
| 7 | `claude/environment` | woods that shrink when cut, soil that tires, droughts that come in runs |

### 1. Families hold land and grain (built)

- **Land:** at the start 30% of families hold none; the others' holdings
  are spread log-normally (a few big farms, many small ones), larger for
  families with more adults. The heir keeps the holding
  (`LandConfig.partible` splits it among children instead). A holding left
  vacant goes to a landless family, usually a young couple.
- **The harvest is shared out in kind.** The fields are still farmed as
  one, but each month's harvest goes into families' own grain stores: the
  labour share (70%) to the families of those who worked the fields, by
  how much they worked, and the land share (30%) to the families holding
  the land, by their plots. The council's levy comes off the top, and the
  farms keep back enough to sell for new tools.
- **Families live off their store.** Each family plans its own ration
  from its store and its expected share of the coming harvests (the same
  planning the village did, family by family), eats from it, and keeps
  enough to eat fully until its harvests come in plus a month's margin
  (less when grain is dear). It sells the rest on the village market;
  families short of grain buy there with coins. A smith's or a weaver's
  family buys all its food.
- **Coins:** families keep about a year of their coin earnings, since
  most of their income is grain; the customary wage follows that. Taxes
  fall on wages and grain sales.
- **New measures:** food eaten from families' own stores, grain sold,
  people in landless families and how well they ate; the stress test adds
  land and wealth inequality.

**What it changes.** Families eat about 80% of their food from their own
stores. The landless share grows from 30% to about 40% over 40 years as
the village grows and only heirs inherit land. In a drought, who goes
hungry now depends on who holds grain: without a council, the village's
worst month is 71% of need and landless families' 54%, with about 37 extra
deaths in three years. Families with grain keep it for themselves as the
outlook darkens, and grain dries up on the market. With the council's
reserve and relief the worst month is 91% (landless 88%), with about 13
extra deaths.

### 2. Seed grain and livestock (built)

- **Seed.** A quarter of the grain harvest (6 rations a plot) is picked
  as seed at harvest (August to October) before anything is shared out,
  and sown at the end of October; next year's harvests follow the share
  sown. Seed comes first, so a 40% harvest failure leaves families about
  half their grain. But no more than 40% of a month's harvest goes to seed
  (people sow less rather than starve now), and landholders make up a
  short store from their own grain at sowing if they can. A village sows
  only the plots that repay their seed, so a village thinned by famine
  farms less land, more intensively. Half the seed gives about 80% of a
  harvest (the labour goes on the plots that were sown), so sowing
  recovers within a year or two.
- **Livestock.** Families (landholders, at the start) hold plough
  animals: a full set (a quarter of a livestock unit a plot) raises farm
  output 30%, and the owners get that share of the harvest (hiring out a
  plough team). Herds grow 15% a year, are thinned each November to what
  can be fed through winter (the meat goes into the owners' stores), and
  die off in droughts and hard winters. Families with no food and no
  coins slaughter their animals; families short of coins sell them to
  families with coins to spare, and when many must sell at once the price
  collapses (to a fifth of an animal's worth at worst).
- **Famine foods.** Families still hungry find roots, greens, nuts and
  fish for up to 15% of their need (more in summer, less in a drought).

**A high-pressure demography.** As in most villages of this size before
modern times (and unlike north-western Europe's late marriage), women
marry young (about 18) and nearly all do; married women bear about six
children; more than a quarter of babies die in their first year and life
expectancy at birth is about 25. Births run about 45 and deaths about 40
per 1,000 in ordinary years. People put off marriage only a little in hard
times; what holds the village to its land is deaths: hunger, epidemics,
and now **plague** (about once a generation, killing a tenth to a fifth).
Couples are tracked, so widows and widowers are known: only women whose
husband is alive bear children, and widows up to 45 and widowers up to 55
remarry, the new spouse moving in with them and their children.

**What it changes.** Bad years now cast a shadow: a drought that eats
into the seed shrinks the next harvest too. Without a council, an April
drought costs about 23 extra deaths and 15 fewer births in three years;
landless families fall to 54% of their need in the worst month. With the
council, about 7 extra deaths and 91% for the landless. Three droughts in
a row cost a quarter to two-fifths of the village, which recovers slowly
over decades; a few runs in a hundred years see a third of the village
die in a year. Some villages never recover, as historically.

**Bugs the stress test caught on the way:** seed set aside from every
month's harvest until the store was full took the whole spring harvest
while people starved; seed sized by the village's full labour force grew
as weak survivors harvested less; and seed for all the land meant the
survivors of a famine could never sow enough, so villages died out to the
last person. Each was a rule no farmer would follow.

### Money is metal, not paper

Coins here are silver (or shells, or barley by weight): nobody prints
them. The village's coins are a fixed stock and prices adjust to it (the
customary wage drifts with how many months of earnings families hold).
Still to come with the state and the regional market: coins flow in when
the village sells grain to the town and out with imports and taxes in
coin; some are lost or buried each year; and a ruler can debase the
coinage.

## Roadmap after Phase 2

3. **Phase 2b** above, then individual firms instead of one business per
   trade.
4. **Many villages → a country.** Regions, trade between them, migration;
   people batched into groups (`count` > 1) with split and merge; national
   money and inflation.
5. **Calibration and starting state.** Start from realistic data (age
   structure, stores, prices) so near-term forecasts mean something.
6. **Interactive dashboard** to pick events and compare scenarios.

## Open questions

- Which events and outcomes matter most for the near-term questions?
- At what point should the starting state come from real data?
- Which time and place should the village stand for? Medieval or early
  modern England has the richest records; Roman Egypt and Old Babylonian
  Mesopotamia are the best-documented ancient cases.
