"""The monthly rules of the village economy.

Each rule works on whole columns of the population table at once and on
headcounts: "how many of this row's people die" is one binomial draw, which is
the same as rolling once per person. With count 1 it is an ordinary roll.
"""

from __future__ import annotations

import numpy as np

from econ_sim.config import Config, SkillConfig
from econ_sim.population import Population


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


def capacity(
    population: Population,
    land: np.ndarray,
    production_mult: np.ndarray,
    config: Config,
    rng: np.random.Generator | None,
) -> np.ndarray:
    """Food each village can grow in an average-season month with today's workers.

    Capacity = base x events x labour^a x land^(1-a). With a < 1, more workers
    on the same land each produce less, which caps how many people a village
    can feed. This month's output is capacity x the season's factor. Without
    an `rng` there are no good or bad days (used for estimates).
    """
    food, health = config.food, config.health
    floor = health.work_at_zero_health
    work_factor = floor + (1.0 - floor) * population.health / health.maximum
    noise = 1.0
    if rng is not None:
        noise_sd = food.output_noise / np.sqrt(np.maximum(population.count, 1))
        noise = np.maximum(0.0, 1.0 + rng.normal(0.0, noise_sd))
    labor = np.where(
        is_working_age(population, config),
        population.count * population.skill * work_factor * noise,
        0.0,
    )
    labor_by_location = by_location(labor, population, len(land))
    a = food.labor_share
    return food.base_output * production_mult * labor_by_location**a * land ** (1.0 - a)


def update_expectation(expected: np.ndarray, capacity: np.ndarray, config: Config) -> np.ndarray:
    """Villagers' sense of a normal month's output: a slowly moving average.

    One bad month shifts it a little; a long run of bad months shifts it a lot.
    """
    return expected + config.food.expectation_weight * (capacity - expected)


def plan_ration(
    stock: np.ndarray, need: np.ndarray, expected_capacity: np.ndarray, month_of_year: int, config: Config
) -> np.ndarray:
    """Share of the full ration each village allows itself this month.

    The village eats the largest steady ration that keeps it from running out
    over the next `planning_months`, counting the stock plus the harvests it
    expects. When a poor harvest leaves the granary short, it tightens belts
    early and spreads the shortage instead of eating well until it is empty.
    """
    months = config.food.planning_months
    if months <= 0:
        return np.ones_like(need)
    upcoming = (month_of_year + np.arange(months - 1)) % 12  # next months, 0 = January
    expected = expected_capacity[:, None] * season_factors(config)[upcoming][None, :]
    harvested_before = np.concatenate([np.zeros((len(stock), 1)), np.cumsum(expected, axis=1)], axis=1)
    available = stock[:, None] + harvested_before  # food on hand by month k (1..months)
    required = need[:, None] * np.arange(1, months + 1)[None, :]  # food eaten through month k
    affordable = np.divide(available, required, out=np.ones_like(available), where=required > 0)
    return np.clip(affordable.min(axis=1), 0.0, 1.0)


def consume(granary: np.ndarray, need: np.ndarray, ration: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Everyone eats from their village granary, all getting the same share.

    Eats up to `ration` x need (if the granary holds that much), takes the
    food out of `granary` and returns (eaten, share of need met).
    """
    eaten = np.minimum(granary, need * ration)
    share = np.divide(eaten, need, out=np.ones_like(need), where=need > 0)
    granary -= eaten
    return eaten, share


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
    """Health drifts toward a level set by how well people eat; events add their toll.

    Full rations lead to full health; `starvation_ration` or less leads to 0.
    Health rises at most `recovery` a month and falls at most `hunger_damage`
    times the share of food missing, so mild shortages wear people down slowly.
    """
    cfg = config.health
    share = food_share[population.location]
    starving = cfg.starvation_ration
    target = cfg.maximum * np.clip((share - starving) / (1.0 - starving), 0.0, 1.0)
    gap = target - population.health
    change = np.where(
        gap > 0,
        np.minimum(gap, cfg.recovery),
        np.maximum(gap, -cfg.hunger_damage * (1.0 - share)),
    )
    change = change + health_delta[population.location]
    population.health = np.clip(population.health + change, 0.0, cfg.maximum)


def death_chance(population: Population, mortality_mult: np.ndarray, config: Config) -> np.ndarray:
    """Monthly chance of dying for each row: age risk (scaled by events) plus hunger risk."""
    table = config.demography.annual_mortality
    from_ages = np.array([age for age, _ in table])
    rates = monthly_chance(np.array([rate for _, rate in table]))
    band = np.searchsorted(from_ages, population.age_years, side="right") - 1
    age_risk = rates[band] * mortality_mult[population.location]

    cfg = config.health
    weakness = np.clip((cfg.danger_threshold - population.health) / cfg.danger_threshold, 0.0, 1.0)
    hunger_risk = cfg.max_extra_mortality * weakness**2
    return np.clip(age_risk + hunger_risk, 0.0, 1.0)


def deaths(
    population: Population,
    mortality_mult: np.ndarray,
    config: Config,
    rng: np.random.Generator,
    n_locations: int,
) -> np.ndarray:
    """Remove the people who die this month; returns deaths per village."""
    died = rng.binomial(population.count, death_chance(population, mortality_mult, config))
    by_village = by_location(died, population, n_locations)
    population.count -= died
    population.remove_empty()
    return by_village


def births(
    population: Population,
    fertility_mult: np.ndarray,
    config: Config,
    rng: np.random.Generator,
    n_locations: int,
) -> np.ndarray:
    """Add this month's newborns; returns births per village.

    Newborns start with their mother's health and a fresh skill draw.
    """
    demo, health = config.demography, config.health
    age = population.age_years
    youngest, oldest = demo.fertile_ages
    fertile = population.female & (age >= youngest) & (age <= oldest)
    low, high = health.fertility_health
    health_factor = np.clip((population.health - low) / (high - low), 0.0, 1.0)
    chance = monthly_chance(demo.annual_birth_chance) * health_factor
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
        )
    )
    return by_village


def grow_older(population: Population) -> None:
    population.age_months += 1
