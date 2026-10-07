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


@dataclass(frozen=True)
class DemographyConfig:
    adult_age: int = 15  # children (< adult_age) eat less and don't work
    retirement_age: int = 60  # people this age and older don't work
    fertile_ages: tuple[int, int] = (16, 45)  # inclusive, women only
    annual_birth_chance: float = 0.17  # per fertile woman in full health
    # People marry later and have fewer children when a wage can barely feed
    # a family. "Wage cover" is how much food a typical month's pay buys,
    # over the village's food need per worker: births are at their lowest
    # (`crowded_birth_factor`) at the first cover, and at full rate from the
    # second up. Crowded land (less food per farmer) and dear grain both lower it.
    crowded_birth_factor: float = 0.2
    wage_cover_for_births: tuple[float, float] = (1.1, 1.5)
    female_share_at_birth: float = 0.5
    # (from age in years, yearly chance of dying) before any hunger or events.
    annual_mortality: tuple[tuple[int, float], ...] = (
        (0, 0.18),
        (1, 0.03),
        (5, 0.006),
        (15, 0.008),
        (50, 0.02),
        (60, 0.04),
        (70, 0.10),
        (80, 0.25),
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
    """

    name: str
    use: str


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


DEFAULT_PRODUCTS: tuple[ProductSpec, ...] = (
    ProductSpec("food", use="food"),
    ProductSpec("firewood", use="heating"),
    ProductSpec("clothing", use="comfort"),
    ProductSpec("tools", use="tool"),
)

DEFAULT_BUSINESSES: tuple[BusinessSpec, ...] = (
    BusinessSpec("farming", "food", output=1.8, tool_boost=0.25, uses_land=True, initial_share=0.80),
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


@dataclass(frozen=True)
class TradeConfig:
    tool_wear: float = 0.03  # share of tools worn out each month
    # Share of its cash a business may spend on supplies or tools at each
    # market (it buys before paying wages).
    buying_budget: float = 0.5
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
    treatment_mortality: float = 0.6


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

    Location events hit everyone in a village. Person events hit each person
    independently, last one month and support only health_delta for now.
    """

    name: str
    scope: str  # "location" or "person"
    chance: float  # per month (per eligible month if `months` is set)
    effects: dict[str, EffectValue]
    duration: int | tuple[int, int] = 1  # months; a range is drawn at start
    months: tuple[int, ...] | None = None  # calendar months (1-12) it can start in
    group: str | None = None  # at most one event per group at a time per village
    businesses: tuple[str, ...] | None = None  # production_mult hits only these
    message: str = ""


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
        message="Good weather: a rich growing season ahead",
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
        message="Drought: harvests will be poor this season",
    ),
    EventSpec(
        name="harsh_winter",
        scope="location",
        chance=0.15,
        months=(12,),
        duration=3,
        effects={"health_delta": -3.0, "mortality_mult": 1.3, "heating_mult": 1.5},
        message="Harsh winter: cold weather weakens the village",
    ),
    EventSpec(
        name="forest_fire",
        scope="location",
        chance=0.02,
        months=(6, 7, 8),
        duration=(2, 3),
        businesses=("woodcutting",),
        effects={"production_mult": 0.4},
        message="Forest fire: little wood can be cut",
    ),
    EventSpec(
        name="disease",
        scope="location",
        chance=0.02,
        duration=(2, 3),
        effects={"health_delta": -8.0, "mortality_mult": 2.5, "production_mult": 0.85},
        message="Disease outbreak",
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
    council: CouncilConfig = field(default_factory=CouncilConfig)
    healthcare: HealthcareConfig = field(default_factory=HealthcareConfig)
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
