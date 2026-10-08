"""Peasant farms: the harvest is shared out in kind and families live off
their own grain store, selling what they can spare and buying what they lack.

The village's fields are farmed as one (economy.capacity), but the grain
belongs to the people who grew it and the families who hold the land:
workers get the labour share by how much they worked, landholders the rest
by the plots they hold. A smallholder family that farms its own plot gets
both; a landless labourer only the first; a smith or a weaver neither, and
buys all their food with coins.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np

from econ_sim import economy, rules
from econ_sim.config import Config
from econ_sim.households import Households
from econ_sim.population import Population


def by_village(values: np.ndarray, location: np.ndarray, n_locations: int) -> np.ndarray:
    return np.bincount(location, weights=values, minlength=n_locations).astype(np.float64)


def income_shares(
    population: Population, households: Households, village_land: np.ndarray, config: Config,
    animals_part: np.ndarray | None = None, lord_land: np.ndarray | None = None,
) -> tuple[np.ndarray, np.ndarray]:
    """Each household's share of its village's harvest (after deductions),
    and the lord's share per village.

    `animals_part` (per village) goes to the owners of plough animals by
    their animals; of the rest, the labour share goes to families by their
    farm work (less the labour days owed to the lord, `labour_service`) and
    the land share to the holders of the land by their plots, the lord's
    demesne (`lord_land`) included.
    """
    n, n_households = len(village_land), len(households)
    loc = households.location
    a = config.food.labor_share
    lord = np.zeros(n) if lord_land is None else lord_land
    effort = economy.household_effort(population, config.farming, n_households, config)
    total_effort = by_village(effort, loc, n)
    all_land = by_village(households.land, loc, n) + lord
    work = np.divide(effort, total_effort[loc], out=np.zeros(n_households), where=total_effort[loc] > 0)
    land = np.divide(households.land, all_land[loc], out=np.zeros(n_households), where=all_land[loc] > 0)
    # Where nobody holds land (or nobody farms), the other share goes to the workers (or holders).
    land_part = np.where(all_land > 0, 1.0 - a, 0.0)
    work_part = np.where(total_effort > 0, a, 0.0)
    total = land_part + work_part
    land_part = np.divide(land_part, total, out=np.zeros(n), where=total > 0)
    work_part = np.divide(work_part, total, out=np.zeros(n), where=total > 0)
    service = np.where(lord > 0, config.lord.labour_service, 0.0)
    to_lord = land_part * np.divide(lord, all_land, out=np.zeros(n), where=all_land > 0) + work_part * service
    shares = work_part[loc] * (1.0 - service[loc]) * work + land_part[loc] * land
    if animals_part is not None:
        herd = by_village(households.animals, loc, n)
        owned = np.divide(households.animals, herd[loc], out=np.zeros(n_households), where=herd[loc] > 0)
        part = np.where(herd > 0, animals_part, 0.0)
        shares = part[loc] * owned + (1.0 - part[loc]) * shares
        to_lord = (1.0 - part) * to_lord
    return shares, to_lord


def share_harvest(
    households: Households, farm_grain: np.ndarray, keep_for_tools: np.ndarray, shares: np.ndarray,
    to_lord: np.ndarray | None = None,
) -> tuple[np.ndarray, np.ndarray]:
    """Move the farms' grain, beyond what they keep to sell for tools, into
    families' stores by their `shares` and to the lord by his. Changes
    `farm_grain`; returns (grain received per household, grain for the lord
    per village)."""
    n = len(farm_grain)
    loc = households.location
    lord = np.zeros(n) if to_lord is None else to_lord
    claims = by_village(shares, loc, n) + lord
    # Villages where nobody has a claim keep their grain in the farm store.
    to_share = np.where(claims > 0, np.maximum(farm_grain - keep_for_tools, 0.0), 0.0)
    per_claim = np.divide(to_share, claims, out=np.zeros(n), where=claims > 0)
    received = shares * per_claim[loc]
    farm_grain -= to_share
    households.grain += received
    return received, lord * per_claim


@dataclass
class FoodPlan:
    own: np.ndarray  # rations each family eats from its own store this month
    spare: np.ndarray  # rations it is willing to sell
    want: np.ndarray  # rations it would like to buy


def plan_family_food(
    households: Households, need: np.ndarray, village_outlook: np.ndarray, shares: np.ndarray,
    after_deductions: np.ndarray, config: Config, markup: np.ndarray | None = None,
) -> FoodPlan:
    """How each family uses its grain this month.

    It eats the steadiest ration its store plus its expected share of the
    coming harvests allows (`rules.plan_ration`, per family), and wants to
    buy the rest of its need. It sells only what is beyond the store it
    needs to eat fully until its harvests come in, plus `keep_months` of
    need as a margin, which it lets go when grain is dear (divided by the
    grain `markup`, per village) or when it is in debt. `after_deductions` (per village) is the
    share of the harvest left after the council's levy.
    """
    loc = households.location
    outlook = village_outlook[loc] * (shares * after_deductions[loc])[:, None]
    ration = rules.plan_ration(households.grain, need, outlook, config, most=1.0)
    own = np.minimum(ration * need, households.grain)
    dear = np.ones(len(loc)) if markup is None else np.maximum(markup[loc], 1.0)
    margin = np.where(households.debt > 0, 0.0, config.land.keep_months)  # debtors sell it to pay
    keep = rules.stock_to_keep(need, outlook, config) + margin * need / dear
    spare = np.maximum(households.grain - np.maximum(keep, own), 0.0)
    return FoodPlan(own=own, spare=spare, want=np.maximum(need - own, 0.0))


def sellers_share(sold: np.ndarray, offered_by_families: np.ndarray, farm_offer: np.ndarray,
                  location: np.ndarray, lord_offer: np.ndarray | None = None
                  ) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """Split each village's grain sales between the farm store (sold first:
    it is grain kept back to pay for tools), and the families and the lord's
    steward who offered grain, in proportion to what each offered. Returns
    (sold per household, sold by the farm store, sold by the lord) per village."""
    n = len(farm_offer)
    lord = np.zeros(n) if lord_offer is None else lord_offer
    by_farms = np.minimum(sold, farm_offer)
    offered = by_village(offered_by_families, location, n) + lord
    filled = np.minimum(np.divide(sold - by_farms, offered, out=np.zeros(n), where=offered > 0), 1.0)
    return offered_by_families * filled[location], by_farms, lord * filled


def seed_needed(farmed: np.ndarray, config: Config) -> np.ndarray:
    """Seed to sow the plots farmed for a year."""
    return config.food.seed_per_plot * farmed


def sowing_month(config: Config) -> int:
    return config.food.seed_months[-1]


def seed_plan(month_of_year: int, config: Config) -> float:
    """Share of the year's seed meant to be picked from this calendar
    month's harvest (by its share of the seed months' harvests)."""
    months = config.food.seed_months
    if month_of_year not in months:
        return 0.0
    season = rules.season_factors(config)
    return float(season[month_of_year - 1] / sum(season[m - 1] for m in months))


def keep_seed(
    farm_grain: np.ndarray, seed: np.ndarray, needed: np.ndarray, harvest: np.ndarray, month_of_year: int,
    config: Config,
) -> np.ndarray:
    """In the seed months, pick next year's seed from this month's `harvest`
    before it is shared out: this month's part of what is `needed`, plus
    anything a past seed month fell short, but never more than
    `max_seed_share` of the harvest. Changes both; returns grain kept."""
    share = seed_plan(month_of_year, config)
    if share <= 0:
        return np.zeros_like(seed)
    months = config.food.seed_months
    done = sum(seed_plan(m, config) for m in months if m < month_of_year)
    wanted = np.maximum(needed * (done + share) - seed, 0.0)
    kept = np.minimum(np.minimum(wanted, config.food.max_seed_share * harvest), farm_grain)
    farm_grain -= kept
    seed += kept
    return kept


def seed_gathered(month_of_year: int, config: Config) -> float:
    """Share of the year's seed picked by the start of this calendar month
    (for a run that starts in the middle of the seed months)."""
    return sum(seed_plan(m, config) for m in config.food.seed_months if m < month_of_year)


def seed_outlook(needed: np.ndarray, month_of_year: int, months: int, config: Config) -> np.ndarray:
    """Seed expected to be taken from each coming month's harvest (next month
    first), villages x months."""
    upcoming = (month_of_year + np.arange(months)) % 12 + 1
    plan = np.array([seed_plan(m, config) for m in upcoming])
    return needed[:, None] * plan[None, :]


def sow(households: Households, seed: np.ndarray, needed: np.ndarray, need: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Sow the seed store; if it is short, landholding families make up the
    rest from their own grain, by the land they hold, keeping at least two
    months of their `need`. Returns (share of the needed seed sown, grain
    families gave) per village."""
    n = len(seed)
    loc = households.location
    short = np.maximum(needed - seed, 0.0)
    can_give = np.where(households.land > 0, np.maximum(households.grain - 2.0 * need, 0.0), 0.0)
    held = by_village(households.land * (can_give > 0), loc, n)
    # By land held, but nobody gives more than they can.
    ask = np.divide(households.land * (can_give > 0), held[loc], out=np.zeros(len(loc)), where=held[loc] > 0) * short[loc]
    given = np.minimum(ask, can_give)
    households.grain -= given
    from_families = by_village(given, loc, n)
    sown = np.divide(seed + from_families, needed, out=np.ones(n), where=needed > 0)
    seed[:] = 0.0
    return np.clip(sown, 0.0, 1.0), from_families


def sown_outlook(sown: np.ndarray, month_of_year: int, months: int, config: Config) -> np.ndarray:
    """Multiplier on each coming month's harvest (next month first) from the
    seed sown: this year's sowing up to and including the next sowing month
    (fewer plots sown, worked harder: sown ** (1 - labour share)), a full
    sowing after it (villages x months)."""
    until_sowing = (sowing_month(config) - month_of_year) % 12
    before = np.arange(1, months + 1) <= until_sowing
    return np.where(before[None, :], sown[:, None] ** (1.0 - config.food.labor_share), 1.0)
