"""Businesses: what they make, what they buy, and who works in them.

Each village has one business per trade (farming, woodcutting, ...), made up
of everyone working in it. Arrays are (villages x businesses) or
(villages x products).
"""

from __future__ import annotations

import numpy as np

from econ_sim.config import Config
from econ_sim.population import NO_JOB, Population
from econ_sim.rules import is_working_age


def work_factor(population: Population, config: Config) -> np.ndarray:
    """How much of a full day's work each row's people manage, given their health."""
    floor = config.health.work_at_zero_health
    return floor + (1.0 - floor) * population.health / config.health.maximum


def in_business(population: Population, config: Config) -> np.ndarray:
    """Rows working in a business (not children, the elderly or council staff)."""
    return (population.job >= 0) & (population.job < len(config.businesses))


def headcount(population: Population, n_locations: int, config: Config) -> np.ndarray:
    """Workers in each business (villages x businesses)."""
    return _by_business(population.count.astype(np.float64), population, n_locations, config)


def labor(
    population: Population,
    n_locations: int,
    config: Config,
    rng: np.random.Generator | None = None,
    at_full_health: bool = False,
) -> np.ndarray:
    """Effective workers in each business: headcount x skill x health (x luck).

    Without an `rng` there are no good or bad days; `at_full_health` ignores
    how weak workers are right now (both used for estimates).
    """
    effort = population.count * population.skill
    if not at_full_health:
        effort = effort * work_factor(population, config)
    if rng is not None:
        noise_sd = config.food.output_noise / np.sqrt(np.maximum(population.count, 1))
        effort = effort * np.maximum(0.0, 1.0 + rng.normal(0.0, noise_sd))
    return _by_business(effort, population, n_locations, config)


def _by_business(values: np.ndarray, population: Population, n_locations: int, config: Config) -> np.ndarray:
    n_business = len(config.businesses)
    working = in_business(population, config)
    index = population.location[working].astype(np.int64) * n_business + population.job[working]
    sums = np.bincount(index, weights=values[working], minlength=n_locations * n_business)
    return sums.astype(np.float64).reshape(n_locations, n_business)


def household_effort(population: Population, business: int, n_households: int, config: Config) -> np.ndarray:
    """Work each household puts into one business: headcount x skill x health."""
    effort = population.count * population.skill * work_factor(population, config)
    mine = in_business(population, config) & (population.job == business)
    return np.bincount(population.household[mine], weights=effort[mine], minlength=n_households).astype(np.float64)


def tool_factor(tools: np.ndarray, workers: np.ndarray, config: Config) -> np.ndarray:
    """Output multiplier from tools: 1 + boost x share of workers with a tool."""
    boost = np.array([b.tool_boost for b in config.businesses])
    coverage = np.clip(np.divide(tools, workers, out=np.ones_like(tools), where=workers > 0), 0.0, 1.0)
    return 1.0 + boost * coverage


def capacity(
    labor_now: np.ndarray, tools: np.ndarray, workers: np.ndarray, land: np.ndarray,
    production_mult: np.ndarray, config: Config, farm_mult: np.ndarray | None = None,
    sown: np.ndarray | None = None,
) -> np.ndarray:
    """What each business could make in an average month, before supplies run short.

    Farming has diminishing returns on the land in use (`land_in_use`) that
    was sown (the share of the seed `sown`), and its output (gross of seed)
    is scaled by `farm_mult` per village (plough animals); other trades make
    the same amount per worker however many there are.
    """
    output = np.array([b.output for b in config.businesses])
    made = output * tool_factor(tools, workers, config) * production_mult * labor_now
    a = config.food.labor_share
    farm = config.farming
    boost = tool_factor(tools, workers, config)[:, farm] * (1.0 if farm_mult is None else farm_mult)
    farmed = farm_land(land, tools, workers, config, farm_mult)
    if sown is not None:  # plots left unsown: the farmers work the rest harder
        farmed = farmed * sown
    made[:, farm] = output[farm] * boost * production_mult[:, farm] * labor_now[:, farm] ** a * farmed ** (1.0 - a)
    return made


def farm_land(
    land: np.ndarray, tools: np.ndarray, workers: np.ndarray, config: Config, farm_mult: np.ndarray | None = None
) -> np.ndarray:
    """Plots each village farms, given its farmers, tools and `farm_mult` (plough animals)."""
    farm = config.farming
    boost = tool_factor(tools, workers, config)[:, farm] * (1.0 if farm_mult is None else farm_mult)
    return land_in_use(land, workers[:, farm], config, 12.0 * config.businesses[farm].output * boost)


def land_in_use(land: np.ndarray, farmers: np.ndarray, config: Config, per_plot_output: np.ndarray | float = 1.0) -> np.ndarray:
    """Plots worth farming: all the village's land, unless there are so few
    farmers that the last plots would not repay their seed. Farmers stop
    adding plots where a plot's share of the harvest, (1 - labour share) x
    output per plot, falls to the seed it takes; `per_plot_output` is the
    yearly output per plot of one farmer on one plot (with tools and animals)."""
    a = config.food.labor_share
    worth = np.maximum((1.0 - a) * per_plot_output / config.food.seed_per_plot, 1e-9) ** (1.0 / a)
    return np.minimum(land, farmers * worth)


def input_needs(config: Config) -> np.ndarray:
    """Units of each product used per unit made (businesses x products)."""
    needs = np.zeros((len(config.businesses), len(config.products)))
    for b, spec in enumerate(config.businesses):
        for product, per_unit in spec.inputs:
            needs[b, config.product_index(product)] = per_unit
    return needs


def limit_by_supplies(planned: np.ndarray, supplies: np.ndarray, config: Config) -> np.ndarray:
    """Cut planned output to what the supplies on hand allow, and use them up.

    `supplies` is (villages x businesses x products) and is changed in place.
    """
    per_unit = input_needs(config)[None, :, :]
    possible = np.where(per_unit > 0, np.divide(supplies, per_unit, out=np.zeros_like(supplies), where=per_unit > 0), np.inf)
    made = np.minimum(planned, possible.min(axis=2))
    supplies -= made[:, :, None] * per_unit
    np.maximum(supplies, 0.0, out=supplies)  # rounding dust
    return made


def product_of(config: Config) -> np.ndarray:
    """Index of the product each business makes."""
    return np.array([config.product_index(b.product) for b in config.businesses])


def seller_of(config: Config) -> np.ndarray:
    """Index of the business that makes each product."""
    seller = np.full(len(config.products), -1)
    seller[product_of(config)] = np.arange(len(config.businesses))
    return seller


def productivity(
    population: Population, tools: np.ndarray, workers: np.ndarray, land: np.ndarray, config: Config,
    farm_mult: np.ndarray | None = None, sown: np.ndarray | None = None,
) -> np.ndarray:
    """Normal output per worker in each business: full health, no events, an
    average month; for farming, net of the seed its plots need. For a trade
    nobody works in, what a first worker would make."""
    n = len(land)
    full = labor(population, n, config, at_full_health=True)
    normal = np.ones_like(full)
    farm = config.farming
    made = capacity(full, tools, workers, land, normal, config, farm_mult, sown)
    made[:, farm] = _net_of_seed(made[:, farm], farm_land(land, tools, workers, config, farm_mult), config)
    per_worker = np.divide(made, workers, out=np.zeros_like(made), where=workers > 0)
    first = capacity(normal, np.zeros_like(tools), normal, land, normal, config, farm_mult, sown)
    first[:, farm] = _net_of_seed(first[:, farm], farm_land(land, np.zeros_like(tools), normal, config, farm_mult), config)
    return np.where(workers >= 1, per_worker, first)


def _net_of_seed(gross: np.ndarray, farmed: np.ndarray, config: Config) -> np.ndarray:
    """Monthly farm output less the seed its plots take (never below a tenth of it)."""
    return np.maximum(gross - config.food.seed_per_plot * farmed / 12.0, 0.1 * gross)


def fair_prices(wage: np.ndarray, per_worker: np.ndarray, prices: np.ndarray, config: Config) -> np.ndarray:
    """Cost of each product: the going wage over output per worker, plus supplies.

    `wage` is per village; `prices` (last month's) value the supplies.
    """
    supplies = (input_needs(config)[None, :, :] * prices[:, None, :]).sum(axis=2)
    cost = wage[:, None] / np.maximum(per_worker, 1e-9) + supplies
    fair = np.zeros_like(prices)
    fair[:, product_of(config)] = cost
    return fair


def potential_pay(prices: np.ndarray, per_worker: np.ndarray, config: Config) -> np.ndarray:
    """What a worker could earn in each trade: their output's value less supplies."""
    product = product_of(config)
    cost = (input_needs(config)[None, :, :] * prices[:, None, :]).sum(axis=2)
    return per_worker * (prices[:, product] - cost)


def wanted_workers(work_needed: np.ndarray, workers: np.ndarray, config: Config) -> np.ndarray:
    """How many workers each trade should have, given the work its orders need.

    Trades making necessities get what they need first (shared out if there
    aren't enough workers); comfort trades share whoever is left, in
    proportion to their orders (or farming takes them if nobody wants comforts).
    `work_needed` is in workers: orders over output per worker.
    """
    uses = [config.products[config.product_index(b.product)].use for b in config.businesses]
    necessity = np.array([use != "comfort" for use in uses])
    total = workers.sum(axis=1)
    essential = np.where(necessity, work_needed, 0.0)
    scale = np.minimum(1.0, np.divide(total, essential.sum(axis=1), out=np.ones_like(total), where=essential.sum(axis=1) > 0))
    wanted = essential * scale[:, None]
    left = total - wanted.sum(axis=1)
    comfort = np.where(necessity, 0.0, work_needed)
    comfort_total = comfort.sum(axis=1)
    share = np.divide(comfort, comfort_total[:, None], out=np.zeros_like(comfort), where=comfort_total[:, None] > 0)
    nobody_wants = comfort_total <= 0
    share[nobody_wants, config.farming] = 1.0
    return wanted + share * left[:, None]


def move_workers(population: Population, wanted: np.ndarray, config: Config, rng: np.random.Generator) -> int:
    """Trades with more workers than they want let some go; they join trades
    that are short, in proportion to how short. Returns how many moved."""
    n, n_business = wanted.shape
    workers = headcount(population, n, config)
    surplus = np.maximum(workers - wanted, 0.0)
    shortfall = np.maximum(wanted - workers, 0.0)
    leave_share = np.divide(config.trade.hiring_rate * surplus, workers, out=np.zeros_like(surplus), where=workers > 0)
    working = in_business(population, config)
    job = np.where(working, population.job, 0)
    chance = np.where(working, np.clip(leave_share[population.location, job], 0.0, 1.0), 0.0)
    leaving = rng.binomial(population.count, chance)
    if not leaving.any():
        return 0
    rows = population.split(leaving)
    for location in np.unique(population.location[rows]):
        here = rows[population.location[rows] == location]
        short = shortfall[location]
        if short.sum() <= 0:
            continue
        population.job[here] = rng.choice(n_business, size=len(here), p=short / short.sum())
    return int(leaving.sum())


def assign_new_workers(population: Population, wanted: np.ndarray, config: Config) -> None:
    """Young people coming of age join the trade most short of workers; the old retire."""
    working_age = is_working_age(population, config)
    new = working_age & (population.job == NO_JOB)
    if new.any():
        workers = headcount(population, len(wanted), config)
        short = np.divide(wanted - workers, np.maximum(wanted, 1.0))
        population.job[new] = short.argmax(axis=1)[population.location[new]]
    population.job[~working_age] = NO_JOB


def assign_starting_jobs(population: Population, config: Config, rng: np.random.Generator) -> None:
    """Spread the starting workers over the trades by their starting shares."""
    working = is_working_age(population, config)
    shares = np.array([b.initial_share for b in config.businesses], dtype=np.float64)
    shares = shares / shares.sum() if shares.sum() > 0 else np.full(len(shares), 1 / len(shares))
    population.job[:] = NO_JOB
    population.job[working] = rng.choice(len(shares), size=int(working.sum()), p=shares)
