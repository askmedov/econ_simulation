"""The lord and the state: what leaves the village.

A lord holds part of the village's land (his demesne): its share of the
harvest goes to his barn, and tenants owe him labour days, so part of the
labour share goes to him too. Part of the barn is carted away to his hall
each month; his steward sells some of the rest in the village, and the coins
go into his purse, outside the village (though he spends some of them on the
village's cloth). The state takes a tax in coin each year after the harvest,
a hearth tax on every household and a land tax on families' land; families
without the coins have grain seized. Both purses are outside the village:
coins come back only as the lord spends them here (and, with a regional
market, as the village sells grain to the town).
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np

from econ_sim.config import Config
from econ_sim.households import Households


def by_village(values: np.ndarray, location: np.ndarray, n_locations: int) -> np.ndarray:
    return np.bincount(location, weights=values, minlength=n_locations).astype(np.float64)


@dataclass
class Lords:
    land: np.ndarray  # demesne plots, per village
    barn: np.ndarray  # rations in the lord's barn, per village
    purse: np.ndarray  # coins the lord has taken out of the village
    state_purse: np.ndarray  # coins the state has taken out of the village

    @classmethod
    def none(cls, n_locations: int) -> Lords:
        zeros = np.zeros(n_locations)
        return cls(land=zeros.copy(), barn=zeros.copy(), purse=zeros.copy(), state_purse=zeros.copy())


def take_demesne(village_land: np.ndarray, config: Config) -> np.ndarray:
    """Plots the lord holds in each village at the start."""
    cfg = config.lord
    return village_land * (cfg.demesne if cfg.enabled else 0.0)


def into_barn(lords: Lords, grain: np.ndarray) -> None:
    lords.barn += grain


def cart_away(lords: Lords, config: Config) -> np.ndarray:
    """A share of the barn goes to the lord's hall each month. Returns rations taken away."""
    away = lords.barn * config.lord.carted_away
    lords.barn -= away
    return away


def offer(lords: Lords, config: Config) -> np.ndarray:
    """Grain the lord's steward offers on the village market this month."""
    return lords.barn * config.lord.sells


def sold(lords: Lords, rations: np.ndarray, price: np.ndarray) -> None:
    lords.barn -= rations
    lords.purse += rations * price


def local_spending(lords: Lords, config: Config) -> np.ndarray:
    """Coins the lord's household spends in the village this month (on its cloth)."""
    return lords.purse * config.lord.spends_locally


def charity(lords: Lords, gap: np.ndarray, location: np.ndarray, famine: np.ndarray, config: Config) -> np.ndarray:
    """A charitable lord opens his barn to families still hungry in a famine
    (`gap` per household). Returns rations given per household."""
    n = len(lords.barn)
    if not config.lord.charity_in_famine:
        return np.zeros_like(gap)
    wanted = by_village(gap, location, n)
    given = np.where(famine, np.minimum(wanted, lords.barn), 0.0)
    share = np.divide(given, wanted, out=np.zeros(n), where=wanted > 0)
    lords.barn -= given
    return gap * share[location]


@dataclass
class TaxTake:
    coins: np.ndarray  # per village
    grain: np.ndarray  # rations seized from families without the coins, per village


def collect_state_tax(
    households: Households, lords: Lords, wage: np.ndarray, rent_per_plot: np.ndarray, food_price: np.ndarray,
    famine: np.ndarray, config: Config, living: np.ndarray | None = None,
) -> TaxTake:
    """The state's yearly tax: `hearth_tax_months` of the customary wage on
    every living household and `land_tax` of a year's rent on every plot
    families hold. Paid in coins; from families without enough, the
    collector seizes grain at the market price; what they still can't pay
    is let go. In a famine it is remitted if `remit_in_famine`. Households
    with nobody left (`living` False) pay nothing."""
    cfg = config.state
    n = len(wage)
    loc = households.location
    if not cfg.enabled:
        return TaxTake(coins=np.zeros(n), grain=np.zeros(n))
    due = cfg.hearth_tax_months * wage[loc] + cfg.land_tax * households.land * rent_per_plot[loc]
    if living is not None:
        due = np.where(living, due, 0.0)
    if cfg.remit_in_famine:
        due = np.where(famine[loc], 0.0, due)
    paid = np.minimum(due, np.maximum(households.money, 0.0))
    households.money -= paid
    short = due - paid
    seized = np.minimum(np.divide(short, food_price[loc], out=np.zeros_like(short), where=food_price[loc] > 0),
                        households.grain)
    households.grain -= seized
    coins = by_village(paid, loc, n)
    lords.state_purse += coins
    return TaxTake(coins=coins, grain=by_village(seized, loc, n))


def requisition(
    households: Households, lords: Lords, farm_grain: np.ndarray, share: np.ndarray
) -> tuple[np.ndarray, np.ndarray]:
    """Soldiers or raiders take `share` (per village) of all grain and
    animals. Returns (rations taken from families and farms, rations taken
    from the lord's barn) per village."""
    n = len(share)
    loc = households.location
    from_families = households.grain * share[loc]
    households.grain -= from_families
    households.animals *= 1.0 - share[loc]
    from_barn = lords.barn * share
    lords.barn -= from_barn
    from_farms = farm_grain * share
    farm_grain -= from_farms
    return by_village(from_families, loc, n) + from_farms, from_barn
