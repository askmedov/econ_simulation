"""The monthly rules of the village economy.

Each rule works on whole columns of the population table at once and on
headcounts: "how many of this row's people die" is one binomial draw, which is
the same as rolling once per person. With count 1 it is an ordinary roll.
"""

from __future__ import annotations

import numpy as np

from econ_sim.config import Config, SkillConfig
from econ_sim.population import Population


def spouse_alive(population: Population) -> np.ndarray:
    """Married people whose husband or wife is still living (per row). In a
    row standing for many people, married people count as couples."""
    paired = population.couple >= 0
    alive = (population.count > 1) & ~paired
    if paired.any():
        living = np.bincount(population.couple[paired], weights=population.count[paired])
        alive[paired] = living[population.couple[paired]] >= 2
    return alive & population.married


def monthly_chance(annual: float | np.ndarray) -> float | np.ndarray:
    return 1.0 - (1.0 - np.asarray(annual)) ** (1.0 / 12.0)


def by_location(values: np.ndarray, population: Population, n_locations: int) -> np.ndarray:
    """Sum a per-row value for each village."""
    sums = np.bincount(population.location, weights=values, minlength=n_locations)
    return sums.astype(np.float64)  # bincount gives ints when there are no rows


def draw_skill(counts: np.ndarray, cfg: SkillConfig, rng: np.random.Generator) -> np.ndarray:
    """Average skill of each row. Larger groups average out closer to the mean."""
    sd = cfg.sd / np.sqrt(np.maximum(counts, 1))
    return np.clip(rng.normal(cfg.mean, sd), cfg.minimum, cfg.maximum)


def food_need(population: Population, config: Config) -> np.ndarray:
    """Rations each row needs this month."""
    child = population.age_years < config.demography.adult_age
    per_person = np.where(child, config.food.child_need, config.food.adult_need)
    return population.count * per_person


def is_working_age(population: Population, config: Config) -> np.ndarray:
    age = population.age_years
    return (age >= config.demography.adult_age) & (age < config.demography.retirement_age)


def season_factors(config: Config) -> np.ndarray:
    """Output multiplier for each calendar month (index 0 = January), averaging 1."""
    seasonality = np.asarray(config.food.seasonality, dtype=np.float64)
    return seasonality / seasonality.mean()


def harvest_outlook(
    normal_capacity: np.ndarray, event_outlook: np.ndarray, month_of_year: int, config: Config
) -> np.ndarray:
    """Food each village expects to grow in each of the coming months.

    What today's workers grow in normal health, by season, times the effect
    of events already under way for as long as they last. Villagers can see a
    drought ruining this year's crop, but don't expect one next year.
    """
    upcoming = (month_of_year + np.arange(event_outlook.shape[1])) % 12  # next months, 0 = January
    return normal_capacity[:, None] * season_factors(config)[upcoming][None, :] * event_outlook


def plan_ration(
    stock: np.ndarray, need: np.ndarray, outlook: np.ndarray, config: Config, most: float = 1.0
) -> np.ndarray:
    """Share of the full ration each village allows itself this month.

    The village eats the largest steady ration that keeps it from running out
    over the coming months, counting the stock (which keeps spoiling) and the
    harvests in `outlook` (one column per coming month). When a poor harvest
    leaves the granary short, it tightens belts early and spreads the
    shortage instead of eating well until it is empty. The result is capped
    at `most` (above 1: how much more than needed could safely be sold).
    """
    if config.food.planning_months <= 0:
        return np.ones_like(need)
    keep = 1.0 - config.food.spoilage
    # Food on hand at the start of each month, and food eaten up to and
    # including it, both shrinking by spoilage as they are carried forward.
    available, required = stock.astype(np.float64), need.astype(np.float64)
    affordable = np.divide(available, required, out=np.ones_like(available), where=required > 0)
    for month in range(outlook.shape[1]):
        available = available * keep + outlook[:, month]
        required = required * keep + need
        ratio = np.divide(available, required, out=np.ones_like(available), where=required > 0)
        affordable = np.minimum(affordable, ratio)
    return np.clip(affordable, 0.0, most)


def stock_to_keep(need: np.ndarray, outlook: np.ndarray, config: Config) -> np.ndarray:
    """The smallest store that lets each family (or village) eat its full
    need this month and every month of `outlook` (its expected harvest
    income, one column per coming month), allowing for spoilage: anything
    beyond it is spare. The inverse of `plan_ration` at a full ration."""
    keep = 1.0 - config.food.spoilage
    smallest = need.astype(np.float64).copy()
    if config.food.planning_months <= 0:
        return smallest
    income = np.zeros_like(smallest)
    required = need.astype(np.float64).copy()
    for month in range(outlook.shape[1]):
        income = income * keep + outlook[:, month]
        required = required * keep + need
        smallest = np.maximum(smallest, (required - income) / keep ** (month + 1))
    return np.maximum(smallest, 0.0)


def plan_ration_realistically(
    stock: np.ndarray, need: np.ndarray, normal_outlook: np.ndarray, config: Config, rounds: int = 3, most: float = 1.0
) -> np.ndarray:
    """Plan rations knowing that hungry workers grow less.

    `normal_outlook` assumes workers in full health. A smaller ration weakens
    them and shrinks future harvests, so re-plan with the work they could do
    on the planned ration, a few times until the plan and the harvest agree.
    """
    ration = plan_ration(stock, need, normal_outlook, config, most)
    for _ in range(rounds):
        effort = expected_work_factor(np.minimum(ration, 1.0), config) ** config.food.labor_share
        ration = plan_ration(stock, need, normal_outlook * effort[:, None], config, most)
    return ration


def target_health(food_share: np.ndarray, config: Config) -> np.ndarray:
    """The health people settle at on a given share of their food need."""
    cfg = config.health
    starving = cfg.starvation_ration
    return cfg.maximum * np.clip((food_share - starving) / (1.0 - starving), 0.0, 1.0)


def expected_work_factor(food_share: np.ndarray, config: Config) -> np.ndarray:
    """Work output, as a share of full, of people settled on a given ration."""
    cfg = config.health
    floor = cfg.work_at_zero_health
    return floor + (1.0 - floor) * target_health(food_share, config) / cfg.maximum


def spoil(granary: np.ndarray, rate: float) -> np.ndarray:
    spoiled = granary * rate
    granary -= spoiled
    return spoiled


def update_health(
    population: Population,
    food_share: np.ndarray,
    health_delta: np.ndarray,
    config: Config,
) -> None:
    """Health drifts toward a level set by how well people eat; events and cold add their toll.

    `food_share` and `health_delta` are per row (each person eats and keeps
    warm with their family). Full rations lead to full health;
    `starvation_ration` or less leads to 0. Health rises at most `recovery` a
    month and falls at most `hunger_damage` times the share of food missing,
    so mild shortages wear people down slowly.
    """
    cfg = config.health
    share = food_share
    gap = target_health(share, config) - population.health
    change = np.where(
        gap > 0,
        np.minimum(gap, cfg.recovery),
        np.maximum(gap, -cfg.hunger_damage * (1.0 - share)),
    )
    change = change + health_delta
    population.health = np.clip(population.health + change, 0.0, cfg.maximum)


def death_chance(
    population: Population, mortality_mult: np.ndarray, config: Config, care: np.ndarray | None = None
) -> np.ndarray:
    """Monthly chance of dying for each row: age risk (scaled by events) plus
    hunger risk, both scaled by `care` (per row; below 1 for people treated)."""
    table = config.demography.annual_mortality
    from_ages = np.array([age for age, _ in table])
    rates = monthly_chance(np.array([rate for _, rate in table]))
    band = np.searchsorted(from_ages, population.age_years, side="right") - 1
    age_risk = rates[band] * mortality_mult[population.location]

    cfg = config.health
    weakness = np.clip((cfg.danger_threshold - population.health) / cfg.danger_threshold, 0.0, 1.0)
    hunger_risk = cfg.max_extra_mortality * weakness**2
    risk = age_risk + hunger_risk
    if care is not None:
        risk = risk * care
    return np.clip(risk, 0.0, 1.0)


def deaths(
    population: Population,
    mortality_mult: np.ndarray,
    config: Config,
    rng: np.random.Generator,
    n_locations: int,
    care: np.ndarray | None = None,
) -> np.ndarray:
    """Remove the people who die this month; returns deaths per village."""
    died = rng.binomial(population.count, death_chance(population, mortality_mult, config, care))
    by_village = by_location(died, population, n_locations)
    population.count -= died
    population.remove_empty()
    return by_village


def wage_cover(average_pay: np.ndarray, food_price: np.ndarray, need: np.ndarray, workers: np.ndarray) -> np.ndarray:
    """Food a typical month's pay buys, over the village's food need per worker."""
    bought = np.divide(average_pay, food_price, out=np.zeros_like(average_pay), where=food_price > 0)
    per_worker = np.divide(need, workers, out=np.full_like(need, np.inf), where=workers > 0)
    return np.divide(bought, per_worker, out=np.zeros_like(bought), where=np.isfinite(per_worker) & (per_worker > 0))


def marriage_factor(cover: np.ndarray, config: Config) -> np.ndarray:
    """How much being able to feed a family encourages people to marry."""
    demo = config.demography
    low, high = demo.wage_cover_for_marriage
    room = np.clip((cover - low) / (high - low), 0.0, 1.0)
    return demo.crowded_marriage_factor + (1.0 - demo.crowded_marriage_factor) * room


def births(
    population: Population,
    fertility_mult: np.ndarray,
    config: Config,
    rng: np.random.Generator,
    n_locations: int,
) -> np.ndarray:
    """Add this month's newborns; returns births per village.

    Married women of fertile age whose husband is alive give birth with `annual_birth_chance`
    times their age's `fertility_by_age`, lowered by poor health and scaled
    by `fertility_mult` per village (from events). Newborns join their
    mother's household, with her health and a fresh skill draw.
    """
    demo, health = config.demography, config.health
    age = population.age_years
    youngest, oldest = demo.fertile_ages
    fertile = population.female & spouse_alive(population) & (age >= youngest) & (age <= oldest)
    from_ages = np.array([a for a, _ in demo.fertility_by_age])
    by_age = np.array([f for _, f in demo.fertility_by_age])[np.maximum(np.searchsorted(from_ages, age, side="right") - 1, 0)]
    low, high = health.fertility_health
    health_factor = np.clip((population.health - low) / (high - low), 0.0, 1.0)
    chance = monthly_chance(demo.annual_birth_chance * by_age) * health_factor
    chance = np.where(fertile, chance * fertility_mult[population.location], 0.0)

    babies = rng.binomial(population.count, chance)
    by_village = by_location(babies, population, n_locations)

    mothers = np.flatnonzero(babies)
    girls = rng.binomial(babies[mothers], demo.female_share_at_birth)
    boys = babies[mothers] - girls
    counts = np.concatenate([girls, boys])
    has_people = counts > 0
    counts = counts[has_people]
    population.append(
        Population(
            count=counts,
            age_months=np.zeros(len(counts)),
            female=np.repeat([True, False], len(mothers))[has_people],
            health=np.tile(population.health[mothers], 2)[has_people],
            skill=draw_skill(counts, config.skill, rng),
            location=np.tile(population.location[mothers], 2)[has_people],
            household=np.tile(population.household[mothers], 2)[has_people],
        )
    )
    return by_village


def grow_older(population: Population) -> None:
    population.age_months += 1
