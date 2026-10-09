"""Livestock: plough animals and herds held by families.

Animals (in livestock units: an ox or a cow; a few sheep or pigs count as
one) plough and manure the fields: the farms grow more the closer the village
comes to `per_plot` animals per plot, and the owners get that part of the
harvest. Herds grow in good years, are thinned each November to what the
village can feed through winter, die off in droughts and hard winters, and
are sold when families can't afford their food, for less the more are sold
at once.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np

from econ_sim.config import Config
from econ_sim.households import Households


def by_village(values: np.ndarray, location: np.ndarray, n_locations: int) -> np.ndarray:
    return np.bincount(location, weights=values, minlength=n_locations).astype(np.float64)


def farm_factor(animals: np.ndarray, land: np.ndarray, config: Config) -> np.ndarray:
    """Farm output multiplier per village from its plough animals."""
    cfg = config.livestock
    wanted = cfg.per_plot * land
    coverage = np.clip(np.divide(animals, wanted, out=np.ones_like(animals), where=wanted > 0), 0.0, 1.0)
    return 1.0 + cfg.farm_boost * coverage


def owners_part(factor: np.ndarray) -> np.ndarray:
    """Share of the harvest due to the animals (what they add over none)."""
    return (factor - 1.0) / factor


def place_herds(households: Households, village_land: np.ndarray, config: Config, rng: np.random.Generator) -> None:
    """Give landholding families a full village set of animals at the start,
    roughly by the land they hold."""
    n = len(village_land)
    weight = households.land * rng.lognormal(0.0, 0.5, size=len(households))
    total = by_village(weight, households.location, n)
    loc = households.location
    herd = config.livestock.per_plot * village_land
    households.animals = np.divide(weight * herd[loc], total[loc], out=np.zeros(len(households)), where=total[loc] > 0)


def grow_and_die(
    households: Households, farm_weather: np.ndarray, heating_mult: np.ndarray, config: Config
) -> np.ndarray:
    """A month of births and deaths in every herd. Bad weather for the farms
    (`farm_weather` below 1) and hard winters (`heating_mult` above 1) kill
    animals. Returns animals lost per village (net of births, if negative)."""
    cfg = config.livestock
    n = len(farm_weather)
    before = by_village(households.animals, households.location, n)
    losses = cfg.drought_losses * np.maximum(1.0 - farm_weather, 0.0) + cfg.winter_losses * np.maximum(heating_mult - 1.0, 0.0)
    change = (1.0 + cfg.growth) ** (1 / 12) - 1.0 - losses / 12
    households.animals *= np.maximum(1.0 + change[households.location], 0.0)
    return before - by_village(households.animals, households.location, n)


def winter_cull(
    households: Households, village_land: np.ndarray, config: Config, pasture: np.ndarray | None = None
) -> tuple[np.ndarray, np.ndarray]:
    """Thin each village's herds to what it can feed through winter (more
    with hill pasture or river meadows: `pasture` times the usual), every
    owner in proportion; the meat goes into their grain stores. Returns
    (animals slaughtered, rations of meat) per village."""
    cfg = config.livestock
    n = len(village_land)
    loc = households.location
    herd = by_village(households.animals, loc, n)
    room = cfg.winter_capacity_per_plot * village_land * (1.0 if pasture is None else pasture)
    cull = np.divide(np.maximum(herd - room, 0.0), herd, out=np.zeros(n), where=herd > 0)
    slaughtered = households.animals * cull[loc]
    households.animals -= slaughtered
    households.grain += slaughtered * cfg.meat_rations
    culled = by_village(slaughtered, loc, n)
    return culled, culled * cfg.meat_rations


def slaughter_in_hunger(
    households: Households, need: np.ndarray, food_price: np.ndarray, config: Config
) -> tuple[np.ndarray, np.ndarray]:
    """Families with less than a month's food in store and too few coins to
    buy it slaughter animals for up to two months of food; the meat goes
    into their stores. `need` is per household, `food_price` per village.
    Returns (animals slaughtered, rations of meat) per village."""
    cfg = config.livestock
    n = len(food_price)
    loc = households.location
    afford = np.divide(households.money, food_price[loc], out=np.zeros_like(need), where=food_price[loc] > 0)
    desperate = (households.grain < need) & (households.grain + afford < need)
    killed = np.where(desperate, np.minimum(households.animals, 2.0 * need / cfg.meat_rations), 0.0)
    households.animals -= killed
    households.grain += killed * cfg.meat_rations
    slaughtered = by_village(killed, loc, n)
    return slaughtered, slaughtered * cfg.meat_rations


@dataclass
class AnimalSales:
    sold: np.ndarray  # animals sold, per village
    price: np.ndarray  # coins per animal, per village


def distress_sales(
    households: Households, short: np.ndarray, spare: np.ndarray, worth: np.ndarray, config: Config
) -> AnimalSales:
    """Families `short` of coins for this month's food sell animals to
    families with coins to `spare`.

    Each seller offers enough animals to raise its shortfall at the animal's
    `worth` (per village), up to all it has. Buyers put up to
    `buyers_spend` of their spare coins into animals. With more animals on
    offer than buyers can pay for at their worth, the price falls to what
    the buyers' coins will pay (not below `lowest_price` of the worth, in
    which case only that many sell). Coins and animals change hands in
    proportion to what each offered or could spend.
    """
    cfg = config.livestock
    n = len(worth)
    loc = households.location
    offer = np.minimum(households.animals, np.divide(short, worth[loc], out=np.zeros_like(short), where=worth[loc] > 0))
    offer = np.maximum(offer, 0.0)
    budget = cfg.buyers_spend * np.maximum(spare, 0.0) * (offer <= 0)
    offered = by_village(offer, loc, n)
    coins = by_village(budget, loc, n)
    price = np.minimum(worth, np.divide(coins, offered, out=worth.copy(), where=offered > 0))
    price = np.maximum(price, cfg.lowest_price * worth)
    sold = np.minimum(offered, np.divide(coins, price, out=np.zeros(n), where=price > 0))
    filled = np.divide(sold, offered, out=np.zeros(n), where=offered > 0)
    selling = offer * filled[loc]
    # The families with the most coins to spare buy them, as far as their coins go.
    from econ_sim.credit import _biggest_first

    got = _biggest_first(np.divide(budget, price[loc], out=np.zeros_like(budget), where=price[loc] > 0), sold, loc)
    households.animals += got - selling
    households.money += (selling - got) * price[loc]
    return AnimalSales(sold=sold, price=price)
