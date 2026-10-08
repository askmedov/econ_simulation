"""Work beyond people's trades: everyone at harvest, cloth made at home,
and kin who help each other.

At harvest every hand is needed: smiths, weavers and woodcutters spend part
of their days in the fields, and children and the old glean, bind and
carry. Their families earn a share of the harvest for it, and the trades
make less those months. The rest of the year women spin and weave at home,
so families wear homespun when they can't buy cloth (and buy less when they
can). Kin, a family and the families its sons founded, give each other
grain before anyone turns to the neighbours or to a lender.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np

from econ_sim import rules
from econ_sim.config import Config
from econ_sim.economy import in_business, work_factor
from econ_sim.households import Households
from econ_sim.population import NO_JOB, Population


def harvest_time(month_of_year: int, config: Config) -> bool:
    cfg = config.work
    return cfg.enabled and month_of_year in cfg.harvest_months


@dataclass
class HarvestHelp:
    farm: np.ndarray  # effort added to the fields, per village
    kept: np.ndarray  # share of their effort each business keeps (businesses,)
    households: np.ndarray  # effort each household adds to the fields


def harvest_help(
    population: Population, n_locations: int, n_households: int, month_of_year: int, config: Config
) -> HarvestHelp:
    """Who helps in the fields this month, and how much the trades lose."""
    cfg = config.work
    n_business = len(config.businesses)
    kept = np.ones(n_business)
    if not harvest_time(month_of_year, config):
        return HarvestHelp(farm=np.zeros(n_locations), kept=kept, households=np.zeros(n_households))
    kept[:] = 1.0 - cfg.craft_help
    kept[config.farming] = 1.0
    effort = population.count * population.skill * work_factor(population, config)
    crafts = in_business(population, config) & (population.job != config.farming)
    age = population.age_years
    young = (age >= cfg.helper_ages[0]) & (age <= cfg.helper_ages[1])
    old = (age >= config.demography.retirement_age) & (age <= cfg.old_helper_age)
    helpers = (population.job == NO_JOB) & (young | old)
    helping = np.where(crafts, cfg.craft_help, 0.0) + np.where(helpers, cfg.helper_effort, 0.0)
    help_effort = effort * helping
    households = np.bincount(population.household, weights=help_effort, minlength=n_households).astype(np.float64)
    farm = rules.by_location(help_effort, population, n_locations)
    return HarvestHelp(farm=farm, kept=kept, households=households)


def harvest_boost(farm_effort: np.ndarray, help_effort: np.ndarray, config: Config) -> np.ndarray:
    """How much more the fields yield in a harvest month with everyone's
    help, per village (diminishing returns on the land)."""
    a = config.food.labor_share
    more = np.divide(help_effort, farm_effort, out=np.zeros_like(help_effort), where=farm_effort > 0)
    return (1.0 + more) ** a


def outlook_boost(boost: np.ndarray, month_of_year: int, months: int, config: Config) -> np.ndarray:
    """`boost` (per village) for each of the coming `months` that falls in
    the harvest (villages x months); next month first."""
    upcoming = (month_of_year + np.arange(months)) % 12 + 1
    harvest = np.isin(upcoming, config.work.harvest_months) if config.work.enabled else np.zeros(months, dtype=bool)
    return np.where(harvest[None, :], boost[:, None], 1.0)


def home_cloth(population: Population, n_households: int, month_of_year: int, config: Config) -> np.ndarray:
    """Cloth each family's women spin and weave at home this month (none at harvest)."""
    cfg = config.work
    if not cfg.enabled or month_of_year in cfg.harvest_months:
        return np.zeros(n_households)
    women = population.female & rules.is_working_age(population, config)
    made = np.where(women, population.count * cfg.home_cloth * work_factor(population, config), 0.0)
    return np.bincount(population.household, weights=made, minlength=n_households).astype(np.float64)


def kin_help(households: Households, alive: np.ndarray, short: np.ndarray, spare: np.ndarray, config: Config) -> np.ndarray:
    """Grain kin give each other this month. A family `short` of rations
    it can't afford gets grain from its kin (the family it came from, and
    the families its children founded) with grain to `spare`; each gives up
    to `kin_share` of its spare, shared among its needy kin by their need.
    Moves the grain; returns rations received per household (negative:
    given)."""
    n = len(households)
    moved = np.zeros(n)
    if not config.work.enabled:
        return moved
    child = np.flatnonzero((households.kin >= 0) & alive)
    parent = households.kin[child]
    keep = alive[parent]
    child, parent = child[keep], parent[keep]
    if not len(child):
        return moved
    # Help flows both ways along each link.
    giver = np.concatenate([parent, child])
    taker = np.concatenate([child, parent])
    asked = np.where(spare[giver] > 0, np.maximum(short[taker], 0.0), 0.0)
    asked_of = np.bincount(giver, weights=asked, minlength=n)
    can_give = np.minimum(config.work.kin_share * np.maximum(spare, 0.0), asked_of)
    offered = asked * np.divide(can_give, asked_of, out=np.zeros(n), where=asked_of > 0)[giver]
    # Nobody takes more than they are short of.
    offered_to = np.bincount(taker, weights=offered, minlength=n)
    taken = np.minimum(offered_to, np.maximum(short, 0.0))
    given = offered * np.divide(taken, offered_to, out=np.zeros(n), where=offered_to > 0)[taker]
    moved -= np.bincount(giver, weights=given, minlength=n)
    moved += np.bincount(taker, weights=given, minlength=n)
    households.grain += moved
    return moved
