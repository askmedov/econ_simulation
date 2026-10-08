"""Credit and distress: loans, repayment, foreclosure and land sales.

In a bad year families short of coins for their food borrow from families
with coins to spare, at high interest. Loans are pooled per village: each
borrower owes the village's lenders, and each lender holds a claim on the
pool, so every coin repaid (or every animal and plot seized) is shared out
among the lenders by their claims. A borrower whose debt outgrows what
their animals and land are worth loses them to the lenders; families still
short sell land outright, and when many must sell at once its price
collapses. This is how a one-year shock turns into lasting inequality.

Debts and claims are in coins: `households.debt` (owed) and
`households.lent` (claims). In every village they add up to the same total.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np

from econ_sim.config import Config
from econ_sim.households import Households


def by_village(values: np.ndarray, location: np.ndarray, n_locations: int) -> np.ndarray:
    return np.bincount(location, weights=values, minlength=n_locations).astype(np.float64)


def _pro_rata(weights: np.ndarray, totals: np.ndarray, location: np.ndarray) -> np.ndarray:
    """Each household's share of its village's total, by `weights`."""
    n = len(totals)
    village = by_village(weights, location, n)
    return np.divide(weights, village[location], out=np.zeros_like(weights), where=village[location] > 0) * totals[location]


def _biggest_first(capacity: np.ndarray, totals: np.ndarray, location: np.ndarray) -> np.ndarray:
    """Share out each village's `totals` to its households, the largest
    `capacity` first, each taking up to its capacity: the richest buyers and
    the biggest creditors get the land and animals that change hands."""
    got = np.zeros_like(capacity)
    for village in np.flatnonzero(totals > 0):
        here = np.flatnonzero((location == village) & (capacity > 0))
        order = here[np.argsort(-capacity[here], kind="stable")]
        before = np.cumsum(capacity[order]) - capacity[order]
        got[order] = np.clip(totals[village] - before, 0.0, capacity[order])
    return got


def land_price(rent_per_plot: np.ndarray, config: Config) -> np.ndarray:
    """Coins a plot fetches: `years_purchase` years of its rent."""
    return config.credit.years_purchase * rent_per_plot


def collateral(households: Households, plot_price: np.ndarray, animal_price: np.ndarray) -> np.ndarray:
    """What a family's land and animals are worth, in coins."""
    loc = households.location
    return households.land * plot_price[loc] + households.animals * animal_price[loc]


def accrue_interest(households: Households, config: Config) -> np.ndarray:
    """A month's interest on every debt (and claim). Returns interest per household."""
    rate = (1.0 + config.credit.interest) ** (1 / 12) - 1.0
    interest = households.debt * rate
    households.debt += interest
    households.lent *= 1.0 + rate
    return interest


def borrow(
    households: Households, short: np.ndarray, spare: np.ndarray, worth: np.ndarray, personal: np.ndarray,
    config: Config,
) -> np.ndarray:
    """Families `short` of coins borrow up to their credit limit (`loan_to_value`
    of their land and animals' `worth`, plus `personal` credit, less what
    they owe) from families with coins to `spare`, who lend up to
    `lend_share` of it. Returns coins lent per village."""
    cfg = config.credit
    n = int(households.location.max()) + 1 if len(households) else 0
    loc = households.location
    room = np.maximum(cfg.loan_to_value * worth + personal - households.debt, 0.0)
    asked = np.minimum(np.maximum(short, 0.0), room)
    offered = cfg.lend_share * np.maximum(spare, 0.0) * (asked <= 0)
    wanted, available = by_village(asked, loc, n), by_village(offered, loc, n)
    lent = np.minimum(wanted, available)
    got = _pro_rata(asked, lent, loc)
    gave = _pro_rata(offered, lent, loc)
    households.money += got - gave
    households.debt += got
    households.lent += gave
    return lent


def repay(
    households: Households, income: np.ndarray, config: Config, spare: np.ndarray | None = None
) -> np.ndarray:
    """Borrowers pay `repay_share` of this month's income toward their debts,
    plus any coins they have to `spare` (beyond what they keep); lenders get
    it by their claims. Changes the households; returns coins repaid per
    village."""
    n = int(households.location.max()) + 1 if len(households) else 0
    loc = households.location
    offered = config.credit.repay_share * np.maximum(income, 0.0)
    if spare is not None:
        offered = offered + np.maximum(spare, 0.0)
    paying = np.minimum(households.debt, np.minimum(offered, households.money))
    paying = np.maximum(paying, 0.0)
    repaid = by_village(paying, loc, n)
    households.money -= paying
    households.debt -= paying
    received = _pro_rata(households.lent, repaid, loc)
    households.money += received
    households.lent -= received
    return repaid


def repay_in_grain(
    households: Households, need: np.ndarray, food_price: np.ndarray, config: Config
) -> np.ndarray:
    """Debtors pay in grain too: `grain_repay_share` of whatever grain they
    hold beyond `grain_kept_months` of their `need` (mostly after harvest),
    valued at the village price; the lenders get the grain by their claims.
    Returns rations paid per village."""
    cfg = config.credit
    n = len(food_price)
    loc = households.location
    beyond = np.maximum(households.grain - cfg.grain_kept_months * need, 0.0)
    owed = np.divide(households.debt, food_price[loc], out=np.zeros_like(need), where=food_price[loc] > 0)
    paying = np.where(households.debt > 0, np.minimum(cfg.grain_repay_share * beyond, owed), 0.0)
    households.grain -= paying
    households.debt -= paying * food_price[loc]
    rations = by_village(paying, loc, n)
    value = rations * food_price
    received = _pro_rata(households.lent.copy(), value, loc)
    households.lent -= received
    households.grain += np.divide(received, food_price[loc], out=np.zeros_like(need), where=food_price[loc] > 0)
    np.maximum(households.debt, 0.0, out=households.debt)
    return rations


@dataclass
class Foreclosures:
    animals: np.ndarray  # taken, per village
    land: np.ndarray  # plots taken, per village
    value: np.ndarray  # coins of debt they settled, per village


def foreclose(
    households: Households, plot_price: np.ndarray, animal_price: np.ndarray, config: Config
) -> Foreclosures:
    """Debts beyond `foreclose_at` of what a borrower's animals and land are
    worth are settled by taking them: animals first, then land, at their
    price, as far as they cover the debt (what they don't cover stays owed).
    The biggest creditors get them, and the debt is cleared from their claims."""
    cfg = config.credit
    n = len(plot_price)
    loc = households.location
    worth = collateral(households, plot_price, animal_price)
    over = households.debt > cfg.foreclose_at * worth
    take = np.where(over, np.minimum(worth, households.debt), 0.0)
    animal_value = households.animals * animal_price[loc]
    from_animals = np.minimum(take, animal_value)
    from_land = take - from_animals
    animals = np.divide(from_animals, animal_price[loc], out=np.zeros_like(take), where=animal_price[loc] > 0)
    plots = np.divide(from_land, plot_price[loc], out=np.zeros_like(take), where=plot_price[loc] > 0)
    animals = np.minimum(animals, households.animals)
    plots = np.minimum(plots, households.land)
    households.animals -= animals
    households.land -= plots
    households.debt -= take
    settled = by_village(take, loc, n)
    taken_animals, taken_land = by_village(animals, loc, n), by_village(plots, loc, n)
    # The biggest creditors take them, each up to its claim.
    value = _biggest_first(households.lent.copy(), settled, loc)
    share = np.divide(value, settled[loc], out=np.zeros_like(value), where=settled[loc] > 0)
    households.lent -= value
    households.animals += share * taken_animals[loc]
    households.land += share * taken_land[loc]
    return Foreclosures(animals=taken_animals, land=taken_land, value=settled)


def default(households: Households, limit: np.ndarray, config: Config) -> np.ndarray:
    """Debt beyond `default_at` times a family's credit `limit` (per household)
    can never be repaid: the excess is lost to the lenders, by their claims.
    Returns coins written off per village."""
    n = int(households.location.max()) + 1 if len(households) else 0
    loc = households.location
    excess = np.maximum(households.debt - config.credit.default_at * limit, 0.0)
    households.debt -= excess
    lost = by_village(excess, loc, n)
    households.lent -= _pro_rata(households.lent.copy(), lost, loc)
    np.maximum(households.lent, 0.0, out=households.lent)
    return lost


@dataclass
class LandSales:
    plots: np.ndarray  # sold, per village
    price: np.ndarray  # coins per plot, per village


def sell_land(
    households: Households, short: np.ndarray, spare: np.ndarray, worth: np.ndarray, config: Config
) -> LandSales:
    """Families still `short` of coins for their food sell land to families
    with coins to `spare` (who put up to `buyers_spend` of it into land).
    With more land on offer than buyers can pay for at its `worth`, the
    price falls (not below `lowest_price` of it)."""
    cfg = config.credit
    n = len(worth)
    loc = households.location
    offer = np.minimum(households.land, np.divide(short, worth[loc], out=np.zeros_like(short), where=worth[loc] > 0))
    offer = np.maximum(offer, 0.0)
    budget = cfg.buyers_spend * np.maximum(spare, 0.0) * (offer <= 0)
    offered, coins = by_village(offer, loc, n), by_village(budget, loc, n)
    price = np.minimum(worth, np.divide(coins, offered, out=worth.copy(), where=offered > 0))
    price = np.maximum(price, cfg.lowest_price * worth)
    sold = np.minimum(offered, np.divide(coins, price, out=np.zeros(n), where=price > 0))
    filled = np.divide(sold, offered, out=np.zeros(n), where=offered > 0)
    selling = offer * filled[loc]
    # The richest buyers take it, each as far as its coins go.
    bought = _biggest_first(np.divide(budget, price[loc], out=np.zeros_like(budget), where=price[loc] > 0), sold, loc)
    households.land += bought - selling
    households.money += selling * price[loc] - bought * price[loc]
    return LandSales(plots=sold, price=price)


def cancel_debts(households: Households, share: np.ndarray) -> np.ndarray:
    """A ruler's decree cancels `share` (per village) of all debts and the
    claims on them. Returns coins of debt cancelled per village."""
    n = len(share)
    loc = households.location
    cancelled = households.debt * share[loc]
    households.debt -= cancelled
    households.lent *= 1.0 - share[loc]
    return by_village(cancelled, loc, n)


def write_off(households: Households, gone: np.ndarray) -> None:
    """Debts of families with nobody left are lost to their lenders; their
    own claims are cancelled (their borrowers are let off)."""
    n = int(households.location.max()) + 1 if len(households) else 0
    loc = households.location
    lost = by_village(np.where(gone, households.debt, 0.0), loc, n)
    households.debt = np.where(gone, 0.0, households.debt)
    living_claims = np.where(gone, 0.0, households.lent)
    households.lent -= np.where(gone, 0.0, _pro_rata(living_claims, lost, loc))
    let_off = by_village(np.where(gone, households.lent, 0.0), loc, n)
    households.lent = np.where(gone, 0.0, households.lent)
    households.debt -= _pro_rata(households.debt, let_off, loc)
    np.maximum(households.debt, 0.0, out=households.debt)
    np.maximum(households.lent, 0.0, out=households.lent)
