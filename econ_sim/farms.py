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
    population: Population, households: Households, village_land: np.ndarray, config: Config
) -> np.ndarray:
    """Each household's share of its village's harvest (after deductions):
    the labour share by its farm work, the land share by its plots."""
    n, n_households = len(village_land), len(households)
    a = config.food.labor_share
    effort = economy.household_effort(population, config.farming, n_households, config)
    total_effort = by_village(effort, households.location, n)
    held = by_village(households.land, households.location, n)
    loc = households.location
    work = np.divide(effort, total_effort[loc], out=np.zeros(n_households), where=total_effort[loc] > 0)
    land = np.divide(households.land, held[loc], out=np.zeros(n_households), where=held[loc] > 0)
    # Where nobody holds land (or nobody farms), the other share goes to the workers (or holders).
    land_part = np.where(held[loc] > 0, 1.0 - a, 0.0)
    work_part = np.where(total_effort[loc] > 0, a, 0.0)
    total = land_part + work_part
    work_part = np.divide(work_part, total, out=np.zeros(n_households), where=total > 0)
    land_part = np.divide(land_part, total, out=np.zeros(n_households), where=total > 0)
    return work_part * work + land_part * land


def share_harvest(
    households: Households, farm_grain: np.ndarray, keep_for_tools: np.ndarray, shares: np.ndarray
) -> np.ndarray:
    """Move the farms' grain, beyond what they keep to sell for tools, into
    families' stores by their `shares`. Changes `farm_grain`; returns grain
    received per household."""
    n = len(farm_grain)
    to_share = np.maximum(farm_grain - keep_for_tools, 0.0)
    shared_out = by_village(shares, households.location, n)
    # Villages where nobody has a claim keep their grain in the farm store.
    to_share = np.where(shared_out > 0, to_share, 0.0)
    received = to_share[households.location] * np.divide(
        shares, shared_out[households.location], out=np.zeros_like(shares), where=shared_out[households.location] > 0
    )
    farm_grain -= to_share
    households.grain += received
    return received


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
    grain `markup`, per village). `after_deductions` (per village) is the
    share of the harvest left after the council's levy.
    """
    loc = households.location
    outlook = village_outlook[loc] * (shares * after_deductions[loc])[:, None]
    ration = rules.plan_ration(households.grain, need, outlook, config, most=1.0)
    own = np.minimum(ration * need, households.grain)
    dear = np.ones(len(loc)) if markup is None else np.maximum(markup[loc], 1.0)
    keep = rules.stock_to_keep(need, outlook, config) + config.land.keep_months * need / dear
    spare = np.maximum(households.grain - np.maximum(keep, own), 0.0)
    return FoodPlan(own=own, spare=spare, want=np.maximum(need - own, 0.0))


def sellers_share(sold: np.ndarray, offered_by_families: np.ndarray, farm_offer: np.ndarray,
                  location: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Split each village's grain sales between the farm store (sold first:
    it is grain kept back to pay for tools) and the families who offered
    grain, in proportion to what each offered. Returns (sold per household,
    sold by the farm store per village)."""
    n = len(farm_offer)
    by_farms = np.minimum(sold, farm_offer)
    offered = by_village(offered_by_families, location, n)
    filled = np.divide(sold - by_farms, offered, out=np.zeros(n), where=offered > 0)
    return offered_by_families * np.minimum(filled, 1.0)[location], by_farms
