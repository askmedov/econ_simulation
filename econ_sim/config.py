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
    population: int = 100
    # Farmland in plots: one plot is what one worker farms at base output.
    land: float = 35.0
    # Starting granary, as months of the starting population's food need.
    initial_food_months: float = 5.0


@dataclass(frozen=True)
class DemographyConfig:
    adult_age: int = 15  # children (< adult_age) eat less and don't work
    retirement_age: int = 60  # people this age and older don't work
    fertile_ages: tuple[int, int] = (16, 45)  # inclusive, women only
    annual_birth_chance: float = 0.17  # per fertile woman in full health
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
    # Rations per month from one worker on one plot, in an average month.
    base_output: float = 2.0
    # Share of output due to labour; the rest is due to land. Below 1 means
    # diminishing returns: more workers on the same land each produce less.
    labor_share: float = 0.7
    # Output by calendar month (Jan..Dec). Rescaled to average 1.0.
    seasonality: tuple[float, ...] = (
        0.2, 0.2, 0.4, 0.6, 0.8, 1.0, 1.2, 2.0, 2.6, 1.8, 0.6, 0.6,
    )
    spoilage: float = 0.02  # share of stored food lost each month
    # The village rations its food so it lasts this many months ahead.
    # 0 means no planning: eat full rations until the granary is empty.
    planning_months: int = 12
    # How fast villagers update their sense of a normal harvest: the weight
    # given to this month's output (0.1 is roughly a 10-month memory).
    expectation_weight: float = 0.1
    # Random good/bad days: spread of each worker's monthly output.
    output_noise: float = 0.1


@dataclass(frozen=True)
class HealthConfig:
    initial: tuple[float, float] = (80.0, 100.0)
    maximum: float = 100.0
    # Health drifts toward a level set by the ration: full health on full
    # rations, 0 on `starvation_ration` or less, in between linearly.
    starvation_ration: float = 0.5
    recovery: float = 10.0  # most health regained in a month
    # Most health lost in a month with no food; scaled by the share missing.
    hunger_damage: float = 40.0
    # Below this health, extra monthly death risk rises to the max at 0.
    danger_threshold: float = 50.0
    max_extra_mortality: float = 0.08
    # Work output at health 0, as a share of output at full health.
    work_at_zero_health: float = 0.5
    # Fertility is zero at or below the first value, full at the second.
    fertility_health: tuple[float, float] = (40.0, 90.0)


@dataclass(frozen=True)
class SkillConfig:
    mean: float = 1.0
    sd: float = 0.15
    minimum: float = 0.5
    maximum: float = 1.5


@dataclass(frozen=True)
class EventSpec:
    """A kind of random event, described as data.

    Effects apply every month the event is active:
      production_mult  multiplies food output
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
    message: str = ""


DEFAULT_EVENTS: tuple[EventSpec, ...] = (
    EventSpec(
        name="good_weather",
        scope="location",
        chance=0.15,
        months=(4,),
        duration=6,
        group="weather",
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
        effects={"production_mult": 0.6},
        message="Drought: harvests will be poor this season",
    ),
    EventSpec(
        name="harsh_winter",
        scope="location",
        chance=0.15,
        months=(12,),
        duration=3,
        effects={"health_delta": -3.0, "mortality_mult": 1.3},
        message="Harsh winter: cold weather weakens the village",
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
    events: tuple[EventSpec, ...] = DEFAULT_EVENTS
    random_events: bool = True  # False: only scheduled events happen
    scheduled_events: tuple[ScheduledEvent, ...] = ()
