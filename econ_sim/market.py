"""Money and markets: families buy with coins, businesses pay wages.

All money is held by families and businesses; it changes hands but is never
created or destroyed. Each village has its own market and price.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np

from econ_sim.config import Config
from econ_sim.economy import in_business, work_factor
from econ_sim.population import Population


def by_household(values: np.ndarray, population: Population, n_households: int) -> np.ndarray:
    """Sum a per-row value for each household."""
    sums = np.bincount(population.household, weights=values, minlength=n_households)
    return sums.astype(np.float64)


def by_village(values: np.ndarray, location: np.ndarray, n_locations: int) -> np.ndarray:
    """Sum a per-household value for each village."""
    return np.bincount(location, weights=values, minlength=n_locations).astype(np.float64)


def share_with_neighbours(
    money: np.ndarray, cost: np.ndarray, location: np.ndarray, n_locations: int, config: Config
) -> np.ndarray:
    """Families with plenty help families who can't afford their food.

    Savings above `sharing_threshold_months` of a family's own food cost are
    given at `sharing_rate` a month into a village pot, which goes to families
    short of money for this month's food, in proportion to the shortfall.
    Changes `money` in place; returns the amount shared per village.
    """
    cfg = config.money
    surplus = np.maximum(0.0, money - cfg.sharing_threshold_months * cost)
    shortfall = np.maximum(0.0, cost - money)
    offered = by_village(cfg.sharing_rate * surplus, location, n_locations)
    wanted = by_village(shortfall, location, n_locations)
    pot = np.minimum(offered, wanted)
    give = np.divide(pot, offered, out=np.zeros_like(pot), where=offered > 0)[location]
    get = np.divide(pot, wanted, out=np.zeros_like(pot), where=wanted > 0)[location]
    money -= cfg.sharing_rate * surplus * give
    money += shortfall * get
    return pot


@dataclass
class Sale:
    bought: np.ndarray  # per household
    spent: np.ndarray  # per household
    sold: np.ndarray  # per village
    demand: np.ndarray  # per village, what families asked for at the going price


def buy(
    money: np.ndarray, want: np.ndarray, price: np.ndarray, offer: np.ndarray, location: np.ndarray
) -> Sale:
    """Families buy up to what they want and can afford; if too little is on
    offer, everyone gets the same share of what they asked for.

    `price` and `offer` are per village. Takes the payment out of `money`.
    """
    n = len(offer)
    family_price = price[location]
    asked = np.minimum(want, np.divide(money, family_price, out=np.zeros_like(money), where=family_price > 0))
    asked = np.maximum(asked, 0.0)
    demand = by_village(asked, location, n)
    filled = np.minimum(1.0, np.divide(offer, demand, out=np.ones_like(offer), where=demand > 0))
    bought = asked * filled[location]
    spent = np.minimum(bought * family_price, money)
    money -= spent
    return Sale(bought=bought, spent=spent, sold=by_village(bought, location, n), demand=demand)


def adjust_markup(markup: np.ndarray, demand: np.ndarray, supply: np.ndarray, config: Config) -> np.ndarray:
    """Mark-ups rise when buyers want more than is on offer and fall when goods go unsold."""
    cfg = config.trade
    gap = np.divide(demand - supply, supply, out=np.zeros_like(supply), where=supply > 0)
    gap = np.where((supply <= 0) & (demand > 0), 1.0, gap)  # nothing on offer at all
    change = np.clip(cfg.markup_speed * gap, -cfg.max_markup_change, cfg.max_markup_change)
    return np.clip(markup * (1.0 + change), *cfg.markup_range)


def seasonal_buffer(profile: tuple[float, ...]) -> np.ndarray:
    """Stock to hold going into each calendar month, in months of average
    need, so that making the average need every month lasts through the
    coming season: the largest running shortfall of need over that average,
    from that month on. Index 0 is January."""
    buffer = np.zeros(len(profile))
    if np.mean(profile) <= 0:
        return buffer
    season = np.asarray(profile, dtype=np.float64) / np.mean(profile)
    for month in range(len(season)):
        ahead = np.roll(season, -month) - 1.0
        buffer[month] = max(np.cumsum(ahead).max(), 0.0)
    return buffer


def grain_markup(markup: np.ndarray, cover: np.ndarray, config: Config) -> np.ndarray:
    """Move the grain mark-up toward the level set by how short the year's supply is."""
    food = config.food
    target = (food.normal_cover / np.maximum(cover, 1e-6)) ** food.price_elasticity
    target = np.clip(target, *config.trade.markup_range)
    return markup + food.price_adjustment * (target - markup)


def pay_wages(
    cash: np.ndarray, population: Population, n_households: int, config: Config, keep: np.ndarray | None = None
) -> tuple[np.ndarray, np.ndarray]:
    """Each business pays out part of its cash to its workers, by skill and health.

    It first sets aside `keep` (villages x businesses), e.g. next month's
    supplies. `cash` has the wages taken out. Returns (income per household,
    wages paid per village and business).
    """
    n_locations, n_business = cash.shape
    effort = population.count * population.skill * work_factor(population, config)
    working = in_business(population, config)
    index = population.location.astype(np.int64) * n_business + np.where(working, population.job, 0)
    total = np.bincount(index[working], weights=effort[working], minlength=n_locations * n_business)
    total = total.reshape(n_locations, n_business)
    spare = cash if keep is None else np.maximum(cash - keep, 0.0)
    payout = np.where(total > 0, config.money.wage_payout * spare, 0.0)
    rate = np.divide(payout, total, out=np.zeros_like(payout), where=total > 0)
    earned = np.where(working, effort * rate.reshape(-1)[index], 0.0)
    cash -= payout
    return by_household(earned, population, n_households), payout
