"""All model parameters in one place.

Every number the rules use lives here, so experiments mean editing config,
not logic. Rates that people think about yearly (births, deaths) are given
per year and converted to monthly chances by the rules.
"""

from __future__ import annotations

from dataclasses import dataclass, field

# A range (low, high) is drawn uniformly when an event starts.
EffectValue = float | tuple[float, float]


@dataclass(frozen=True)
class VillageConfig:
    name: str = "Village"
    population: int = 1000
    # Farmland in plots: one plot is what one worker farms at base output.
    land: float = 350.0
    # Starting granary, as months of the starting population's food need.
    initial_food_months: float = 5.0
    # Where it lies (km east and north, on the map: see MapConfig) and the
    # country around it (a TerrainSpec name).
    x: float = 0.0
    y: float = 0.0
    terrain: str = "plain"
    # On a map, a place is a "village" or a market "town" (no farmland: its
    # people work in crafts and buy their grain; see TownConfig). A
    # `border` town also trades with the world beyond the region: a port,
    # or a town on the road or river out.
    kind: str = "village"
    border: bool = False
    # What a trade needs that only some places have (BusinessSpec.resource):
    # "iron" in part of the hills. (Rust only.)
    resources: tuple[str, ...] = ()
    # The region's stronghold, where the state keeps its garrison and
    # officials (see StateConfig.garrison): its city.
    stronghold: bool = False


@dataclass(frozen=True)
class DemographyConfig:
    adult_age: int = 15  # children (< adult_age) eat less and don't work
    retirement_age: int = 60  # people this age and older don't work
    fertile_ages: tuple[int, int] = (15, 45)  # inclusive, women only
    # Yearly chance a married woman of fertile age in full health, with her
    # husband alive, gives birth, times (from age, factor): fewer births
    # from the mid-thirties. Natural fertility: about six children for a
    # woman married at 18 whose husband lives until she is 45.
    annual_birth_chance: float = 0.26
    fertility_by_age: tuple[tuple[int, float], ...] = ((15, 1.0), (30, 0.9), (35, 0.75), (40, 0.4))
    female_share_at_birth: float = 0.5
    # Marriage is early and nearly universal: each month a single woman of
    # `bride_ages` marries with this chance (on average at about 18), to a
    # single man of `groom_ages` from another family. The first of a family
    # to marry stays with their spouse as its heir; later ones set up a
    # household of their own. Widows up to `widow_ages[0]` and widowers up
    # to `widow_ages[1]` remarry too; the new spouse moves in with them.
    marriage_chance: float = 1 / 24
    bride_ages: tuple[int, int] = (15, 35)
    groom_ages: tuple[int, int] = (18, 45)
    widow_ages: tuple[int, int] = (45, 55)
    # People put off marrying a little when a wage can barely feed a family.
    # "Wage cover" is how much food a typical month's pay buys, over the
    # village's food need per worker: weddings are at their fewest
    # (`crowded_marriage_factor` of the usual chance) at the first cover,
    # and at the usual rate from the second up.
    crowded_marriage_factor: float = 0.5
    wage_cover_for_marriage: tuple[float, float] = (1.0, 1.4)
    # (from age in years, yearly chance of dying) before any hunger or events:
    # a high-pressure pre-industrial village, where more than a quarter of
    # babies die in their first year, half of children before five, and life
    # expectancy at birth is about 25.
    annual_mortality: tuple[tuple[int, float], ...] = (
        (0, 0.28),
        (1, 0.07),
        (5, 0.02),
        (10, 0.01),
        (15, 0.012),
        (25, 0.016),
        (40, 0.022),
        (50, 0.035),
        (60, 0.07),
        (70, 0.14),
        (80, 0.3),
    )
    # Starting ages follow a pyramid: weight of age a is exp(-decay * a).
    initial_age_decay: float = 0.03
    max_initial_age: int = 75


@dataclass(frozen=True)
class FoodConfig:
    # Food is counted in rations: one ration feeds one adult for one month.
    adult_need: float = 1.0
    child_need: float = 0.6
    # Share of farm output due to labour; the rest is due to land. Below 1
    # means diminishing returns: more workers on the same land each produce
    # less. (How much one farmer grows is set in the farming business.)
    labor_share: float = 0.7
    # Output by calendar month (Jan..Dec). Rescaled to average 1.0.
    seasonality: tuple[float, ...] = (
        0.2, 0.2, 0.4, 0.6, 0.8, 1.0, 1.2, 2.0, 2.6, 1.8, 0.6, 0.6,
    )
    spoilage: float = 0.02  # share of stored food lost each month
    # The village rations its food so it lasts this many months ahead.
    # 0 means no planning: eat full rations until the granary is empty.
    planning_months: int = 12
    # Farms aim to grow this much more food than the village eats, to cover
    # spoilage and bad years.
    reserve_margin: float = 0.1
    # Grain prices follow how well stores plus expected harvests cover the
    # coming year's needs, compared with a normal year (`normal_cover`):
    # the price mark-up heads toward (normal / cover) ** `price_elasticity`.
    # 2.5 matches the King-Davenant law of the 1690s: a harvest 10% short
    # raised grain prices about 30%, 20% short about 80%, 30% short about 160%.
    normal_cover: float = 1.3
    price_elasticity: float = 2.5
    # Each month the mark-up closes this share of the gap to that level.
    price_adjustment: float = 0.3
    # Random good/bad days: spread of each worker's monthly output.
    output_noise: float = 0.1
    # Seed: `seed_per_plot` rations of grain a year for each plot farmed,
    # about a quarter of the grain harvest (a yield of 4 grains per grain
    # sown; the rest of the year's food, from gardens, dairy and the like,
    # needs none). It is picked from the harvest in `seed_months`, before
    # anything is shared out, but never more than `max_seed_share` of a
    # month's harvest (people sow less rather than starve now), and sown at
    # the end of the last of them. The next year's harvests scale with the
    # share of the seed that was sown. Farming output is gross of seed.
    seed_per_plot: float = 6.0
    seed_months: tuple[int, ...] = (8, 9, 10)
    max_seed_share: float = 0.4
    # Hungry families find famine foods (roots, greens, nuts, fish) for up to
    # this share of their need, in summer and autumn twice what they find
    # in winter and spring; fewer in a drought.
    foraging: float = 0.15


@dataclass(frozen=True)
class HealthConfig:
    initial: tuple[float, float] = (80.0, 100.0)
    maximum: float = 100.0
    # Health drifts toward a level set by the ration: full health on full
    # rations, 0 on `starvation_ration` or less, in between linearly.
    starvation_ration: float = 0.4
    recovery: float = 10.0  # most health regained in a month
    # Most health lost in a month with no food; scaled by the share missing.
    hunger_damage: float = 40.0
    # Below this health, extra monthly death risk rises to the max at 0.
    danger_threshold: float = 50.0
    max_extra_mortality: float = 0.08
    # Work output at health 0, as a share of output at full health.
    work_at_zero_health: float = 0.5
    # Fertility is zero at or below the first value, full at the second:
    # in lean times people put off having children.
    fertility_health: tuple[float, float] = (60.0, 95.0)


@dataclass(frozen=True)
class SkillConfig:
    mean: float = 1.0
    sd: float = 0.15
    minimum: float = 0.5
    maximum: float = 1.5


@dataclass(frozen=True)
class ProductSpec:
    """Something made and sold. `use` says who wants it and why:

      food     everyone, every month (see FoodConfig); health follows it
      heating  everyone, mostly in winter (see NeedsConfig); cold harms health
      comfort  families with spare money (see NeedsConfig)
      tool     businesses, to make their workers more productive
      material businesses, as supplies for what they make (Rust only: see
               CraftsConfig)
    """

    name: str
    use: str
    # Cost of carting a coin's worth of it, against grain: bulky firewood
    # costs more, cloth and tools much less.
    carriage: float = 1.0


@dataclass(frozen=True)
class BusinessSpec:
    """A trade: its workers make one product.

    Each village has one business of each kind, made up of everyone working
    in that trade. Output per worker in full health, in an average month:
    `output`, times (1 + `tool_boost`) with a full set of tools (one per
    worker). Farming also depends on land and the seasons (FoodConfig).
    """

    name: str
    product: str
    output: float
    tool_boost: float = 0.0
    inputs: tuple[tuple[str, float], ...] = ()  # (product, units used per unit made)
    uses_land: bool = False  # diminishing returns on village land, by season
    initial_share: float = 0.0  # share of workers in this trade at the start
    # Worked only where a place has this resource (VillageConfig.resources),
    # and its `initial_share` is of the whole region's workers, all there; a
    # resource no place has is had by every place. (Rust only.)
    resource: str | None = None


DEFAULT_PRODUCTS: tuple[ProductSpec, ...] = (
    ProductSpec("food", use="food"),
    ProductSpec("firewood", use="heating", carriage=2.0),
    ProductSpec("clothing", use="comfort", carriage=0.25),
    ProductSpec("tools", use="tool", carriage=0.25),
)

DEFAULT_BUSINESSES: tuple[BusinessSpec, ...] = (
    # Farming output is gross of seed and before plough animals and the help
    # of everyone at harvest: with a full set of tools and animals, all that
    # help, and after seed, it is about 2 rations a farmer, enough to feed the
    # village and its lord.
    BusinessSpec("farming", "food", output=1.72, tool_boost=0.25, uses_land=True, initial_share=0.80),
    BusinessSpec("woodcutting", "firewood", output=6.0, tool_boost=0.3, initial_share=0.10),
    BusinessSpec("weaving", "clothing", output=2.0, initial_share=0.07),
    BusinessSpec("smithing", "tools", output=1.0, inputs=(("firewood", 2.0),), initial_share=0.03),
)


@dataclass(frozen=True)
class NeedsConfig:
    # Firewood per person in each calendar month (Jan..Dec): heating in
    # winter, cooking all year.
    firewood: tuple[float, ...] = (0.6, 0.6, 0.45, 0.3, 0.15, 0.1, 0.1, 0.1, 0.15, 0.3, 0.45, 0.6)
    # Health lost in a month with no firewood at all in the coldest month;
    # milder months and partial shortages do proportionally less.
    cold_damage: float = 10.0
    # Families keep this many months of food and firewood costs as savings
    # and spend `spare_spending` of anything above that on comforts (clothing)
    # each month: the better off they are, the more they buy.
    savings_months: float = 3.0
    spare_spending: float = 0.3
    # Families who can't buy all the firewood they need gather up to this
    # share of it themselves: wood, furze, dung and peat from the commons.
    # `other_fuels` of what they can gather isn't wood, so burned or thinned
    # woods cut only the rest.
    gathering: float = 0.8
    other_fuels: float = 0.5


@dataclass(frozen=True)
class TradeConfig:
    tool_wear: float = 0.03  # share of tools worn out each month
    # Share of its cash a business may spend on supplies or tools at each
    # market. It buys before paying wages, so its cash is then mostly what it
    # set aside last month for supplies and tools.
    buying_budget: float = 1.0
    # Businesses (other than farms) aim to keep this many months of orders in
    # stock: they work less as stock piles up beyond it, stopping at twice it.
    stock_target_months: float = 4.0
    # Weight of this month's orders in the running average businesses plan by.
    orders_memory: float = 0.1
    # Prices are a fair price (the going wage over output per worker, plus
    # supplies) times a mark-up that rises when buyers want more than is on
    # offer and falls when goods go unsold: `markup_speed` times the gap, at
    # most `max_markup_change` a month, within `markup_range`.
    markup_speed: float = 0.5
    max_markup_change: float = 0.1
    markup_range: tuple[float, float] = (0.5, 3.0)
    # Trades making necessities (food, firewood, tools) get the workers their
    # orders need first; comforts share whoever is left. Each month this share
    # of the gap moves: trades with too many workers let some go to trades
    # that are short.
    hiring_rate: float = 0.05
    # Trades whose goods sell above their fair price want more workers, and
    # fewer below it: work needed is scaled by mark-up to this power.
    hiring_price_response: float = 0.5
    # Weight of this month's pay in the running averages of pay.
    pay_memory: float = 0.2
    # The customary wage (that fair prices are reckoned in) moves with the
    # money there is: each month by up to `wage_adjustment` of itself, up
    # while families hold more than `money_months` of their monthly coin
    # earnings (averaged over a year) and down while they hold less: the
    # quantity of money sets the price level. Much of a peasant family's
    # income is grain, so its coins last long: a village starts near a year.
    wage_adjustment: float = 0.01
    money_months: float = 12.0


@dataclass(frozen=True)
class MoneyConfig:
    # Food costs this many coins a ration at the start; other prices follow
    # from how long things take to make.
    food_price: float = 1.0
    # Families start with this many months of their food and firewood costs saved.
    initial_savings_months: float = 4.0
    # Each business starts with this many coins per worker.
    initial_business_cash_per_worker: float = 1.0
    # Share of a business's cash paid out as wages each month, after setting
    # aside a month's cost of supplies and tool replacement.
    wage_payout: float = 0.9
    # Families with savings above this many months of their food cost give
    # `sharing_rate` of the excess each month to families who can't afford food.
    sharing_threshold_months: float = 2.0
    sharing_rate: float = 0.25
    # Gifts cover at most this share of a family's shortfall; for the rest it
    # must borrow, or sell animals or land.
    gift_share: float = 0.5


@dataclass(frozen=True)
class LandConfig:
    """Who holds the farmland, and how families live off it.

    The village's fields are farmed as one, but each month's harvest is
    shared out in kind: workers get the labour share (FoodConfig.labor_share)
    by how much they worked, landholders the rest by the plots they hold.
    It goes into each family's own grain store.
    """

    # Share of households holding no land at the start: they live by their
    # labour (in the fields or a trade) and buy what they don't earn in grain.
    landless_share: float = 0.3
    # The others' holdings are spread log-normally with this sigma (a few big
    # farms, many small ones), larger for families with more adults.
    holding_spread: float = 0.8
    # Families keep enough grain to eat fully until expected harvests cover
    # them, plus this many months of their need; they sell the rest.
    keep_months: float = 1.0
    # A younger son or daughter setting up a household of their own takes a
    # share of the family's land and animals (partible inheritance, as in
    # most of the old world: Roman, Chinese, Islamic and Frankish law), or
    # none (the heir keeps it all, as in England).
    partible: bool = True


@dataclass(frozen=True)
class LivestockConfig:
    """Plough animals and herds, held by families, in livestock units (an
    ox or a cow; a few sheep or pigs count as one)."""

    # Animals per plot for full ploughing and manuring; farms grow up to
    # `farm_boost` more with a full set, and the owners get that part of
    # the harvest (hiring out a plough team).
    per_plot: float = 0.25
    farm_boost: float = 0.3
    # Herds grow this much a year with enough fodder. Each November they are
    # thinned to what the village can feed through winter; the meat goes
    # into the owners' stores.
    growth: float = 0.15
    winter_capacity_per_plot: float = 0.3
    meat_rations: float = 5.0
    # On a map with a border town, herds are shorn in `shearing_month` and the
    # wool sold through the towns to the world beyond (as English wool went
    # to Flanders and Italy): a year's wool from an animal fetches `wool`
    # rations' worth at the outside grain price, less the carting to the
    # border. Hill villages, with more pasture, keep more animals.
    wool: float = 3.0
    shearing_month: int = 6
    # Extra yearly losses per unit of farm output lost to bad weather
    # (pasture and hay fail too), and per unit of extra heating a hard
    # winter needs.
    drought_losses: float = 0.6
    winter_losses: float = 0.3
    # An animal is worth this many months of the customary wage. Families
    # who can't afford their food sell animals to families with coins to
    # spare, who put up to `buyers_spend` of their spare coins into them;
    # with many sellers the price drops, to no less than `lowest_price` of its worth.
    value_months: float = 6.0
    buyers_spend: float = 0.5
    lowest_price: float = 0.2
    # Herds at the start, as a share of the usual (a starting point: see
    # `starts.py`). (Rust only.)
    initial_herds: float = 1.0


@dataclass(frozen=True)
class CreditConfig:
    """Loans between families, and what happens when they can't repay."""

    enabled: bool = True
    # A year's interest: grain loans in Hammurabi's Babylon were capped at a
    # third; medieval rates ran from a tenth to a half, secured loans lower.
    interest: float = 0.2
    # Families with coins beyond their savings target lend up to this share
    # of the excess to families who can't afford their food.
    lend_share: float = 0.5
    # A family can borrow up to this share of what its land and animals are
    # worth, plus `personal_months` of the customary wage on its word alone.
    loan_to_value: float = 0.5
    personal_months: float = 2.0
    # Borrowers pay this share of each month's coin income toward their
    # debts (and sell the grain they would otherwise keep as a margin), and
    # `grain_repay_share` of any grain beyond `grain_kept_months` of their
    # need in kind (mostly after harvest: borrow in spring, repay at harvest).
    repay_share: float = 0.5
    grain_repay_share: float = 0.5
    grain_kept_months: float = 2.0
    # When a debt passes this share of what a family's animals and land are
    # worth, the lenders take them (animals first) to settle it.
    foreclose_at: float = 0.9
    # Debt beyond this many times a family's credit limit can never be
    # repaid: the lenders write it off.
    default_at: float = 2.0
    # Land sells for this many years of its rent; families still short of
    # food sell land to families with coins to spare, who put up to
    # `buyers_spend` of their spare coins into it. With many sellers the
    # price falls, to no less than `lowest_price` of its worth.
    years_purchase: float = 15.0
    buyers_spend: float = 0.5
    lowest_price: float = 0.3
    # Landholding families start owing this many months of the customary
    # wage, to the place's biggest landholders (a starting point: see
    # `starts.py`). (Rust only.)
    initial_debt_months: float = 0.0


@dataclass(frozen=True)
class LordConfig:
    """A lord who holds part of the land and takes its harvest out of the village."""

    enabled: bool = True
    # Share of the village's land held by the lord (his demesne): its share
    # of each harvest goes to his barn. Tenants also owe labour days: this
    # share of the farm workers' labour share goes to him unpaid.
    demesne: float = 0.25
    labour_service: float = 0.1
    # Each month this share of his barn is carted away to his hall, his
    # steward sells up to `sells` of the rest in the village (the coins go
    # into his purse, outside the village), and his household spends
    # `spends_locally` of his purse on the village's cloth.
    carted_away: float = 0.2
    sells: float = 0.3
    spends_locally: float = 0.05
    # On a map with market towns, `to_town` of what is carted away is sold
    # to the grain dealers of the village's market town (the rest still goes
    # to a hall, a court or a war far away), and his household spends
    # `spends_in_town` of his purse a month in that town: the rents of the
    # countryside feed the towns.
    to_town: float = 0.4
    spends_in_town: float = 0.25
    # Of what he spends in town, this share pays townsfolk for their
    # service (servants, masons, carters, clerks); the rest buys cloth.
    servants: float = 0.5
    # A what-if lever: a lord who opens his barn to the hungry in a famine.
    charity_in_famine: bool = False


@dataclass(frozen=True)
class StateConfig:
    """A state that taxes the village in coin once a year, after harvest."""

    enabled: bool = True
    collection_month: int = 11
    # Every household pays this many months of the customary wage, and
    # families pay `land_tax` of a year's rent on every plot they hold.
    # Families without the coins have grain seized; the rest is let go.
    hearth_tax_months: float = 0.5
    land_tax: float = 0.1
    # A what-if lever: no tax in a famine year.
    remit_in_famine: bool = False
    # On a map with a stronghold (VillageConfig.stronghold), the state pays
    # its garrison and officials there `garrison` of the coins its tax brought
    # in the last year, a twelfth each month; the rest goes to a court or a
    # war far away. The pay is what keeps a city: towns live on their
    # markets, a city on what the state spends in it. (Rust only.)
    garrison: float = 0.75


@dataclass(frozen=True)
class TownConfig:
    """The regional grain market in the town, and the coins that flow
    to and from it."""

    enabled: bool = True
    # Carting grain to or from the town costs this share of its price, and
    # merchants carry at most `capacity` of the village's monthly need a
    # month either way (a cartload or two).
    transport: float = 0.3
    capacity: float = 0.15
    # The town's price is dearest in `dearest_month` (before the harvest),
    # by `seasonal` either way; it swings at random (a monthly log step of
    # `swing_sd`, keeping `swing_memory` of the last swing), and rises
    # `regional_share` as much as a village's own harvest shortfall would
    # suggest (a drought there is a dearth in the region too).
    seasonal: float = 0.1
    dearest_month: int = 6
    swing_sd: float = 0.03
    swing_memory: float = 0.97
    regional_share: float = 0.4
    # Coins are metal: this share is lost, worn away or buried each year.
    coins_lost_a_year: float = 0.005
    # Market towns as places, on a map (VillageConfig.kind "town"). Towns
    # die faster than they are born (crowding, dirt, fevers: `mortality`
    # times the village death rates) and live on migrants. Their craftsmen
    # make `craft_skill` times as much a day as a village's. Their grain
    # dealers start with `dealers_months` of the town's need in store, and
    # coins to buy as much again; each month they pay out `dealers_payout`
    # of their coins beyond that to the town's families (merchants' and
    # carters' earnings). A border town trades grain with the world beyond
    # at the outside price (`price`), carting it costing `outside_transport`
    # of its price, up to `outside_capacity` of the town's monthly need
    # either way.
    mortality: float = 1.25
    craft_skill: float = 1.3
    # A town has little woodland of its own (`woods` of what a village of its
    # size would have): it buys most of its firewood from the countryside.
    woods: float = 0.1
    # Townsfolk live on coins and hold fewer months of their coin earnings
    # than villagers, who are paid mostly in grain (TradeConfig.money_months):
    # a town's wage follows the money there is against this many.
    money_months: float = 2.5
    # A town's grain costs what the villages ask for it plus carriage, marked
    # up as the dealers' store runs below `dealers_months` of the town's need:
    # the mark-up heads toward (usual store / store) ** `store_elasticity`.
    store_elasticity: float = 0.5
    dealers_months: float = 3.0
    # Dealers send grain away (to other towns, or beyond) only from what they
    # hold beyond this many months of their own town's need.
    keep_months: float = 1.0
    dealers_payout: float = 0.5
    outside_transport: float = 0.15
    outside_capacity: float = 0.5


@dataclass(frozen=True)
class TerrainSpec:
    """What a kind of country does to the villages in it, against open,
    arable plain. Its grain is in the land itself (a plot is what one
    worker farms at base output), so hill villages simply hold more acres
    to the plot."""

    name: str
    carriage: float = 1.0  # cost of carting a load across it
    pasture: float = 1.0  # animals the village can feed through winter, per plot of its farmland
    woods: float = 1.0  # woods it can hold
    forage: float = 1.0  # famine foods to be found: roots, nuts, fish, game
    # Farmland per person a village has at the usual density (its fields
    # count as usual for it: no tired soil, no land to spare): hill
    # villages grow less grain and keep more animals, and buy grain with
    # their wool.
    arable: float = 1.0


DEFAULT_TERRAINS: tuple[TerrainSpec, ...] = (
    TerrainSpec("plain"),
    # River meadows give hay and fish; the river carries the grain.
    TerrainSpec("valley", pasture=1.3, woods=0.8, forage=1.2, arable=1.15),
    # Hill pasture for sheep and cattle; steep, slow tracks.
    TerrainSpec("upland", carriage=1.6, pasture=3.5, woods=0.7, forage=0.8, arable=0.6),
    # Clearings in the woods: wood, nuts, pigs and game, poor roads.
    TerrainSpec("forest", carriage=1.3, pasture=1.25, woods=2.5, forage=1.5, arable=0.8),
    # Fishing villages on the shore.
    TerrainSpec("coast", woods=0.8, forage=1.5, arable=0.9),
)


@dataclass(frozen=True)
class MapConfig:
    """Where the villages and market towns lie, and the roads and rivers
    between them (see `geography`; `geography.region` lays one out from a
    few settings).

    Without a map (`enabled` False) every village is the same `transport`
    from a town outside it (TownConfig) and has its own weather, as in
    Phase 2. With one, the towns are places of their own
    (VillageConfig.kind), carting grain between a village and its market town costs
    `handling` of its price (loading, tolls, market dues, the merchant's
    margin) plus `road_per_km` for each km of road (times the terrain's
    `carriage`) or `river_per_km` for each km by boat (overland carriage
    cost about twice the river rate in 14th-century England), along the
    cheapest way; and the weather falls on the map: neighbouring villages
    share their droughts, good years and hard winters, which change over
    about `weather_km`.
    """

    enabled: bool = False
    # Links between places, numbered as in `Config.villages` (villages and towns).
    roads: tuple[tuple[int, int], ...] = ()
    rivers: tuple[tuple[int, int], ...] = ()
    handling: float = 0.15
    road_per_km: float = 0.01
    river_per_km: float = 0.005
    weather_km: float = 250.0
    # Merchants carry at most this share of a village's monthly need (or
    # orders) of each good between it and its market town each month, either
    # way: carting it costs `transport` x the product's `carriage`. They
    # carry a full load once the gap in price beats the carting by
    # `full_load_margin` of the price, and less for less. In a dearth they
    # bring a village up to `relief_capacity` of its need of grain a month:
    # carts come to wherever people will pay.
    capacity: float = 0.2
    full_load_margin: float = 0.1
    relief_capacity: float = 0.5
    # Between neighbouring market towns, merchants carry at most
    # `town_capacity` a month of the monthly need of the receiving town's
    # market (the town and its villages; for other goods, the town's orders)
    # along a road; a route by river carries up to `water_capacity` times as
    # much (barges), in proportion to the share of the way by water.
    town_capacity: float = 0.3
    water_capacity: float = 3.0
    # Grain dealers trading between towns deal in bulk: their loading, tolls
    # and dues come to `wholesale_handling` of the price (petty carting
    # between a village and its market: `handling`), plus the way.
    wholesale_handling: float = 0.05
    terrains: tuple[TerrainSpec, ...] = DEFAULT_TERRAINS

    def terrain_index(self, name: str) -> int:
        return [t.name for t in self.terrains].index(name)


@dataclass(frozen=True)
class EnvironmentConfig:
    """Woods that shrink when overcut, and soil that tires when the land
    is crowded. (Droughts that come in runs are in the events: see
    `EventSpec.repeat_chance`.)"""

    enabled: bool = True
    # Untouched, a village's woods would hold `woods_years` of its starting
    # firewood need; they regrow logistically, `regrowth` a year at their
    # fastest (coppice is cut again every 10-20 years). A village starts
    # with its woods in balance with its cutting and gathering; they can
    # bear about half again as much for good. As woods thin, wood takes
    # longer to cut and gather: a day's work goes as far as the wood
    # standing (against the start) to the power `reach_exponent`.
    woods_years: float = 40.0
    regrowth: float = 0.15
    reach_exponent: float = 0.5
    # Soil fertility (1 = normal) heads toward 1 - `overcropping` x (people
    # per plot / `people_per_plot` - 1), between `fertility_range`: crowded
    # land is cropped without enough fallow and tires; land left to rest
    # after a famine recovers. It closes `soil_recovery` of the gap a year.
    people_per_plot: float = 2.9
    overcropping: float = 0.2
    fertility_range: tuple[float, float] = (0.6, 1.1)
    soil_recovery: float = 0.1
    # Soil fertility at the start, and the woods standing as a share of what
    # is in balance with the place's use (starting points: see `starts.py`).
    # (Rust only.)
    initial_soil: float = 1.0
    initial_woods: float = 1.0


@dataclass(frozen=True)
class WorkConfig:
    """Work beyond people's trades: everyone at harvest, spinning and
    weaving at home, and kin who help each other."""

    enabled: bool = True
    # In the harvest months everyone who can helps in the fields: people in
    # other trades give `craft_help` of their time (their trades make that
    # much less), and children of `helper_ages` and the old up to
    # `old_helper_age` work at `helper_effort` of a full worker.
    harvest_months: tuple[int, ...] = (7, 8, 9, 10)
    craft_help: float = 0.3
    helper_ages: tuple[int, int] = (10, 14)
    old_helper_age: int = 69
    helper_effort: float = 0.3
    # The rest of the year, women of working age spin and weave at home in
    # the evenings and the slack months: `home_cloth` of cloth a month each
    # (a twentieth of a weaver's output), worn by their families in place of
    # cloth they would buy.
    home_cloth: float = 0.1
    # Kin (a family and the families its sons and daughters founded) help
    # each other first: up to `kin_share` of the grain a family could spare
    # goes to kin who can't afford their food, before neighbours' gifts and
    # loans. At the start, families are linked to a family a generation older.
    kin_share: float = 0.5


@dataclass(frozen=True)
class MigrationConfig:
    """Who leaves a village and who comes to it, by how good a place it is
    to live: its livelihood, its safety, its size and its lord's take."""

    enabled: bool = True
    # How good a place to live (1 = an ordinary village): what a wage or a
    # share of the land feeds, against `usual_prospects`; times
    # exp(-hunger_weight x remembered hunger - danger_weight x remembered
    # danger); times a small village's handicap below `small_village` people
    # (fewer hands for the plough, no church or market, nobody to marry:
    # to the power `size_exponent`); times the land per person against the
    # usual density (EnvironmentConfig.people_per_plot), to the power of the
    # land's share of the harvest (1 - FoodConfig.labor_share), kept within
    # `land_range`: land to spare after a famine or plague draws people,
    # crowding sends them away; times the lord's share of the harvest
    # against `usual_take` (to the power `burden_exponent`, kept within
    # `burden_range`). Hunger (the share of need not eaten) and danger
    # (deaths beyond `normal_mortality` a year, and grain plundered, a
    # share of which counts as `plunder_weight` of people lost) are
    # remembered over `memory_months`.
    usual_prospects: float = 1.1
    memory_months: float = 24.0
    hunger_weight: float = 4.0
    danger_weight: float = 6.0
    normal_mortality: float = 0.045
    plunder_weight: float = 0.25
    small_village: float = 50.0
    size_exponent: float = 0.5
    land_range: tuple[float, float] = (0.7, 1.5)
    usual_take: float = 0.11
    burden_exponent: float = 0.5
    burden_range: tuple[float, float] = (0.6, 1.25)
    # Leaving. Young single people (`young_ages`) leave with `leave_chance` a
    # year and families with `family_leave_chance` (landholders `rooted`
    # times that), more as the place is worse than ordinary: times
    # (1 + push x the shortfall); and times
    # (1 + chain x the share of the village that left in the past year):
    # people follow those who went before.
    young_ages: tuple[int, int] = (15, 29)
    leave_chance: float = 0.025
    family_leave_chance: float = 0.002
    rooted: float = 0.2
    push: float = 3.0
    chain: float = 10.0
    # In a famine, families eating less than `flee_below` of their need leave
    # together with this chance a month; while plague or raiders are about,
    # families flee with `flee_danger` a month per unit of the yearly share
    # of people the danger is taking (about 1% a month in a plague).
    flee_below: float = 0.5
    flee_chance: float = 0.05
    flee_danger: float = 0.03
    # Where they go: with several villages, `stay_in_region` of those who
    # leave settle in another of them that is a better place to live
    # (chosen by its people times how much better it is, and on a map,
    # times exp(-km / `distance_km`): most moved less than a day's walk),
    # bringing their coins and grain; the rest go to the town or beyond (on
    # a map with market towns, `to_town` of them to their own market town,
    # where any family can take in a servant or an apprentice).
    stay_in_region: float = 0.5
    distance_km: float = 15.0
    to_town: float = 0.5
    # Coming from beyond: to a place better than `welcome`, about
    # `arrive_rate` young people a month per 1,000 villagers per unit above
    # it, up to `max_pull` above it (at most about 6% of the village a
    # year), as servants of landholding families.
    welcome: float = 1.2
    arrive_rate: float = 5.0
    max_pull: float = 1.0
    # Settlers take up holdings a famine or plague left empty: each year
    # `settle_rate` of the room on the land (people it would feed at the
    # usual density, less those there), if the place is no worse than
    # `settle_appeal` (all of it from `welcome` up).
    settle_rate: float = 0.02
    settle_appeal: float = 0.9


@dataclass(frozen=True)
class CouncilConfig:
    enabled: bool = True  # False: the village never forms a council
    # A council forms once the village has had this many people for this long.
    forms_at_population: int = 500
    forms_after_months: int = 6
    # Villages already that big at the start have had a council for years:
    # officials in place, a full reserve and a working treasury.
    established_at_start: bool = True
    tax_rate: float = 0.1  # share of wages taken in tax
    # The council stops taxing while its treasury holds this many months of
    # its running costs. Neither taxes nor the grain levy are taken in a
    # famine year (grain on hand plus expected harvests short of the year's need).
    treasury_months: float = 6.0
    officials_per_1000: float = 4.0
    official_pay: float = 1.2  # times the average wage
    # A grain levy (a tithe in kind): this share of each harvest goes into the
    # council's reserve until it holds `reserve_months` of the village's need.
    grain_levy: float = 0.05
    reserve_months: float = 2.0
    # Famine relief: families who can't buy enough food get it free from the
    # reserve, and families who can't afford food and firewood get coins
    # from whatever the treasury holds beyond two months of running costs.
    relief: bool = True


@dataclass(frozen=True)
class HealthcareConfig:
    """Healers hired and paid by the village council."""

    enabled: bool = True
    healers_per_1000: float = 5.0
    healer_pay: float = 1.2  # times the average wage
    patients_per_healer: float = 30.0  # people a healer can see in a month
    # Healers see the people most likely to die this month first, down to
    # this monthly risk (0.003 is about 3.5% a year): infants, the old, the
    # starving and anyone hit by an outbreak.
    min_risk: float = 0.003
    # Treatment adds this much health and scales that month's risk of dying.
    treatment_recovery: float = 10.0
    treatment_mortality: float = 0.85


@dataclass(frozen=True)
class HarvestHandsConfig:
    """Townsfolk who go out to the villages at harvest (so far only in the
    Rust engine, `rust/`; this model ignores it).

    In a village everyone helps at harvest (WorkConfig); in a town nobody
    did. With this on, townsfolk go out to the villages of their market town
    in harvest months, more to the near ones, work in the fields and come
    home with their pay in grain: their share of the field work, as any
    family's. Their trades lose the days they are away.
    """

    enabled: bool = False
    # The share of the help they would give if they lived in a village
    # (WorkConfig.craft_help of a craftsman's days, helper_effort of the
    # young and old) that townsfolk give in the villages around.
    town_share: float = 0.5
    # Each `reach_km` farther from the town, a village gets e times fewer
    # hands (for the same fields).
    reach_km: float = 10.0


@dataclass(frozen=True)
class RefugeesConfig:
    """Families fleeing a famine to the towns, where fevers wait (so far
    only in the Rust engine; this model ignores it)."""

    enabled: bool = False
    # Of the families fleeing a famine (MigrationConfig.flee_below) who find
    # no better village, this share go to their market town; the rest go
    # beyond the region (ordinary leavers go to town at
    # MigrationConfig.to_town).
    to_town: float = 0.8
    # A town's death rate rises by `fevers` times the share of its people
    # who came as refugees over the last `memory_months` (crowding, typhus,
    # dysentery: "famine fever").
    fevers: float = 2.0
    memory_months: float = 6.0


@dataclass(frozen=True)
class CraftsConfig:
    """Crafts beyond the village's four (Rust only).

    Iron is dug and smelted where a place has it (`VillageConfig.resources`:
    part of the hills), and the smiths need it for their tools: a war in
    the iron hills reaches every farm that wants a ploughshare. The
    miner-smelters burn their own charcoal, `charcoal` units of wood a unit
    of iron, cut from the place's woods (so their work goes slower where
    the woods have thinned, as the woodcutters' does). Merchants sell the
    iron across the region from the makers' stock, at their price plus the
    way. At the border towns the world beyond buys the region's crafts
    (`exports`: rations' worth a unit at the outside grain price, less the
    carting), up to `export_share` of what the whole region orders of them
    a month, shared among the border towns: cloth and iron bring coins in,
    as wool does.

    With `enabled`, the Rust engine adds `products` and `businesses` to the
    config's, and smithing takes `smithing_inputs` instead of its own."""

    enabled: bool = True
    products: tuple[ProductSpec, ...] = (ProductSpec("iron", use="material", carriage=0.5),)
    businesses: tuple[BusinessSpec, ...] = (
        # A miner-smelter's month: ore dug, charcoal burned, blooms of iron
        # hammered out (enough for some thirty tools). About as many hands at
        # the start as the smiths' iron settles at: a worker in a thousand.
        BusinessSpec("ironworks", "iron", output=8.0, tool_boost=0.3, initial_share=0.001, resource="iron"),
    )
    charcoal: float = 1.0
    # A smith's iron for a tool, and less of the firewood than without it
    # (the iron comes smelted).
    smithing_inputs: tuple[tuple[str, float], ...] = (("iron", 0.25), ("firewood", 1.5))
    exports: tuple[tuple[str, float], ...] = (("clothing", 2.5), ("iron", 2.0))
    export_share: float = 0.1


@dataclass(frozen=True)
class ChronicleConfig:
    """What the chronicle, the run's log ranked by importance, tells. (Rust only.)

    Each month the chronicle notes the events that started, famines and
    shortages that began or ended, deaths and departures far above the
    usual, grain and tools grown dear, a hungry city, councils formed and
    villages emptied, each with the places it touched, worst first.
    Entries are of four ranks: 1, what touches `major_share` of the
    region's people or more, and every forced event; 2, `notable_share` or
    more, or a town; 3, a place or a few; 4, the small change of a month
    (food short for a while, a council formed). Good news ranks one lower
    than bad."""

    major_share: float = 0.1
    notable_share: float = 0.05
    # A place eating less than `famine_ration` of its need is in famine;
    # less than `short_ration`, short of food.
    famine_ration: float = 0.8
    short_ration: float = 0.97
    # Deaths `deaths_surge` times the usual (and at least `fewest_deaths`
    # more, in a place) are a surge; people leaving `leaving_surge` times the
    # usual, a flight; grain or tools `price_surge` times their usual price,
    # a dearth. The usual is a running average of the months without one.
    deaths_surge: float = 1.5
    fewest_deaths: float = 5.0
    leaving_surge: float = 2.0
    price_surge: float = 1.4
    # Places named under an entry; the rest are counted.
    places_listed: int = 5


@dataclass(frozen=True)
class MindsConfig:
    """Families with minds of their own: the Rust engine's way (`rust/`),
    on by default there. This model ignores it: its families all follow
    the rules, which minds reduce to with no spread of traits and no
    responses (spread, speculation and the four weights all 0).

    Each family has traits drawn at the start, remembers its hunger and its
    losses on loans, and expects a grain price from what it has paid; how
    much grain it holds back, how many coins it keeps, how much it lends
    and whether it leaves follow from those. They are made to be steady:
    the typical family decides as the rules do, hunger pushes a family away
    only beyond its neighbours', expected prices are anchored to the price
    a family thinks normal, and decisions change by habit, not at once.
    """

    enabled: bool = True
    # Traits (caution, thrift, nerve, learning, attachment, patience): each
    # family is z standard deviations from the typical one, z drawn normal
    # and kept within +-trait_limit. A decision multiplies the rules' by
    # exp(trait_spread x z) (or divides by it), scaled so that the average
    # over families is 1. A family founded by a member of another takes
    # that family's z, plus normal(0, inherited_spread / trait_spread).
    trait_spread: float = 0.25
    trait_limit: float = 2.0
    inherited_spread: float = 0.1
    # Hunger (the share of its need a family went without) and losses on
    # loans (the share of its claims written off) fade over `memory_months`.
    # The price a family thinks normal follows what it pays over
    # `normal_months`.
    memory_months: float = 12.0
    normal_months: float = 24.0
    # Each month a family moves the monthly price change it expects by
    # `learning` (times its learning trait) of the way to the change it has
    # just seen. Over `horizon_months` ahead (times its patience) it expects
    # that change to go on, pulled back toward the normal price by
    # `reversion` of the gap, and never more than `expectation_limit` (a
    # share) up or down.
    learning: float = 0.2
    horizon_months: float = 3.0
    reversion: float = 0.25
    expectation_limit: float = 0.25
    # Grain: a family holds back its usual margin times its caution times
    # (1 + the rise it expects) ** speculation.
    speculation: float = 1.0
    # How much grain it holds back and how much it lends move toward what
    # it wants by `inertia` of the way each month (habits change slowly).
    inertia: float = 0.3
    # Coins: a family keeps its usual months of savings times its thrift,
    # times (1 + hunger_saving x its remembered hunger).
    hunger_saving: float = 1.0
    # Loans: a family lends its usual share of spare coins over its caution,
    # times (1 - loss_fear x its remembered losses), and never less than
    # `least_lending` of that.
    loss_fear: float = 2.0
    least_lending: float = 0.5
    # Leaving: of a place's families, those hungrier than their neighbours
    # (by exp(hunger_leaving x the gap / their nerve)) and less attached to
    # it are likelier to be the ones who leave; as many leave on average as
    # by the rules. In a famine a family flees at the rules' chance over its
    # nerve.
    hunger_leaving: float = 4.0


@dataclass(frozen=True)
class EventSpec:
    """A kind of random event, described as data.

    Effects apply every month the event is active:
      production_mult  multiplies output (of `businesses`, or of all)
      heating_mult     multiplies the firewood people need
      health_delta     added to health
      mortality_mult   multiplies age-based death risk (hits young and old hardest)
      fertility_mult   multiplies birth chance
      granary_loss     share of stored food destroyed
      debt_cancel      share of debts cancelled by decree
      requisition      share of stored grain and animals taken by soldiers or raiders
      debase           the coinage loses this share of its worth: the town asks more coins for grain
      woods_burned     share of the village's woods burned each month

    Location events hit everyone in a village. Person events hit each person
    independently, last one month and support only health_delta for now.

    On a map (MapConfig), `reach` says how far an event spreads: "village"
    events strike each village on its own; "weather" events follow the
    season's weather, which neighbouring villages share (bad weather, `end`
    "bad", where the season is worst; good weather, `end` "good", where it
    is best); "region" events strike every village at once (a volcano's cold
    years, a ruler's decree). Without a map every village rolls on its own.
    """

    name: str
    scope: str  # "location" or "person"
    chance: float  # per month (per eligible month if `months` is set)
    effects: dict[str, EffectValue]
    duration: int | tuple[int, int] = 1  # months; a range is drawn at start
    months: tuple[int, ...] | None = None  # calendar months (1-12) it can start in
    group: str | None = None  # at most one event per group at a time per village
    # Chance (in an eligible month) when the same event started in the last
    # 12 months (last year's season included): droughts come in runs. `chance` stays the long-run share of
    # eligible months it starts in; the chance after a year without it is
    # lowered to match.
    repeat_chance: float | None = None
    businesses: tuple[str, ...] | None = None  # production_mult hits only these
    message: str = ""
    reach: str = "village"  # "village", "weather" or "region": see above
    end: str = "bad"  # for weather events: the bad or the good end of the season's weather


DEFAULT_EVENTS: tuple[EventSpec, ...] = (
    EventSpec(
        name="good_weather",
        scope="location",
        chance=0.15,
        months=(4,),
        duration=6,
        group="weather",
        businesses=("farming",),
        effects={"production_mult": 1.25},
        repeat_chance=0.3,
        message="Good weather: a rich growing season ahead",
        reach="weather",
        end="good",
    ),
    EventSpec(
        name="drought",
        scope="location",
        chance=0.10,
        months=(4,),
        duration=6,
        group="weather",
        businesses=("farming",),
        effects={"production_mult": 0.6},
        repeat_chance=0.3,
        message="Drought: harvests will be poor this season",
        reach="weather",
    ),
    EventSpec(
        # A great volcanic eruption veils the sun: cold, wet summers and
        # failed harvests for two seasons (as in 536, 1601 or 1816), once in
        # a couple of centuries.
        name="cold_years",
        scope="location",
        chance=0.005,
        months=(4,),
        duration=18,
        group="weather",
        effects={"production_mult": 0.7, "heating_mult": 1.2},
        businesses=("farming",),
        message="The sun is veiled: cold summers and failed harvests for two years",
        reach="region",
    ),
    EventSpec(
        name="harsh_winter",
        scope="location",
        chance=0.15,
        months=(12,),
        duration=3,
        effects={"health_delta": -3.0, "mortality_mult": 1.3, "heating_mult": 1.5},
        message="Harsh winter: cold weather weakens the village",
        reach="weather",
    ),
    EventSpec(
        name="forest_fire",
        scope="location",
        chance=0.02,
        months=(6, 7, 8),
        duration=(2, 3),
        businesses=("woodcutting",),
        effects={"production_mult": 0.4, "woods_burned": (0.01, 0.04)},
        message="Forest fire: little wood can be cut, and part of the woods is lost",
    ),
    EventSpec(
        name="disease",
        scope="location",
        chance=0.02,
        duration=(2, 3),
        group="epidemic",
        effects={"health_delta": -8.0, "mortality_mult": 2.5, "production_mult": 0.85},
        message="Disease outbreak",
    ),
    EventSpec(
        name="plague",
        scope="location",
        chance=0.003,
        duration=(4, 8),
        group="epidemic",
        effects={"health_delta": -10.0, "mortality_mult": (6.0, 10.0), "production_mult": 0.7},
        message="Plague: a deadly epidemic sweeps the village",
    ),
    EventSpec(
        name="army",
        scope="location",
        chance=0.003,
        duration=1,
        group="soldiers",
        effects={"requisition": (0.2, 0.4), "health_delta": -5.0},
        message="An army passes through: soldiers take grain and animals",
    ),
    EventSpec(
        name="raid",
        scope="location",
        chance=0.002,
        duration=1,
        group="soldiers",
        effects={"requisition": (0.1, 0.3), "mortality_mult": 1.5, "health_delta": -5.0},
        message="Raiders strike: grain and animals taken, people killed",
    ),
    EventSpec(
        name="debasement",
        scope="location",
        chance=0.0,  # only when scheduled: a what-if lever
        effects={"debase": 0.25},
        message="The ruler debases the coinage: coins are worth less",
        reach="region",
    ),
    EventSpec(
        # A war or an embargo closes the wool markets abroad (a what-if
        # lever, on a map: wool is sold beyond through its border towns).
        name="wool_slump",
        scope="location",
        chance=0.0,
        duration=12,
        effects={"wool_price": 0.4},
        message="The wool markets abroad are closed: wool fetches less than half its price",
        reach="region",
    ),
    EventSpec(
        name="debt_jubilee",
        scope="location",
        chance=0.0,  # only when scheduled: a what-if lever
        effects={"debt_cancel": 1.0},
        message="The ruler cancels all debts",
        reach="region",
    ),
    EventSpec(
        name="granary_fire",
        scope="location",
        chance=0.01,
        effects={"granary_loss": (0.1, 0.3)},
        message="Fire or pests in the granary: stored food lost",
    ),
    EventSpec(
        name="accident",
        scope="person",
        chance=0.005,
        effects={"health_delta": -30.0},
        message="Accident or illness",
    ),
)


@dataclass(frozen=True)
class ScheduledEvent:
    """An event forced to start in a given month, for what-if experiments."""

    event: str  # name of a location event in `Config.events`
    month: int  # months from the start; 1 is the first simulated month
    village: int | None = None  # village index; None means every village
    # Or every village within `near[2]` km of the point (near[0], near[1])
    # on the map: a drought over one part of a region.
    near: tuple[float, float, float] | None = None


@dataclass(frozen=True)
class Config:
    seed: int = 42
    months: int = 36
    start_month: int = 1  # calendar month (1-12) of the first simulated month
    villages: tuple[VillageConfig, ...] = (VillageConfig(),)
    demography: DemographyConfig = field(default_factory=DemographyConfig)
    food: FoodConfig = field(default_factory=FoodConfig)
    health: HealthConfig = field(default_factory=HealthConfig)
    skill: SkillConfig = field(default_factory=SkillConfig)
    money: MoneyConfig = field(default_factory=MoneyConfig)
    needs: NeedsConfig = field(default_factory=NeedsConfig)
    trade: TradeConfig = field(default_factory=TradeConfig)
    land: LandConfig = field(default_factory=LandConfig)
    livestock: LivestockConfig = field(default_factory=LivestockConfig)
    credit: CreditConfig = field(default_factory=CreditConfig)
    lord: LordConfig = field(default_factory=LordConfig)
    state: StateConfig = field(default_factory=StateConfig)
    town: TownConfig = field(default_factory=TownConfig)
    migration: MigrationConfig = field(default_factory=MigrationConfig)
    work: WorkConfig = field(default_factory=WorkConfig)
    environment: EnvironmentConfig = field(default_factory=EnvironmentConfig)
    council: CouncilConfig = field(default_factory=CouncilConfig)
    healthcare: HealthcareConfig = field(default_factory=HealthcareConfig)
    map: MapConfig = field(default_factory=MapConfig)
    harvest_hands: HarvestHandsConfig = field(default_factory=HarvestHandsConfig)
    refugees: RefugeesConfig = field(default_factory=RefugeesConfig)
    minds: MindsConfig = field(default_factory=MindsConfig)
    chronicle: ChronicleConfig = field(default_factory=ChronicleConfig)
    crafts: CraftsConfig = field(default_factory=CraftsConfig)
    products: tuple[ProductSpec, ...] = DEFAULT_PRODUCTS
    businesses: tuple[BusinessSpec, ...] = DEFAULT_BUSINESSES
    events: tuple[EventSpec, ...] = DEFAULT_EVENTS
    random_events: bool = True  # False: only scheduled events happen
    scheduled_events: tuple[ScheduledEvent, ...] = ()


    def product_index(self, name: str) -> int:
        return [p.name for p in self.products].index(name)

    def product_for(self, use: str) -> int:
        """Index of the product with this use (food, heating, comfort, tool)."""
        return [p.use for p in self.products].index(use)

    def business_index(self, name: str) -> int:
        return [b.name for b in self.businesses].index(name)

    @property
    def farming(self) -> int:
        """Index of the business that grows food on the land."""
        return next(i for i, b in enumerate(self.businesses) if b.uses_land)
