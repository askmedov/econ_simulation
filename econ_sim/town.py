"""The town: a regional grain market, and where coins come from and go.

Grain has a regional price in the town, which moves with the seasons, with
the region's harvests (a drought in the village is usually a dearth in the
region too) and at random. Carting grain to or from the town costs
`transport` of its price, so merchants buy the village's grain when it is
cheaper than the town price less transport, and bring grain to sell when the
village price is above the town price plus transport, up to `capacity`
(a share of the village's monthly need) either way.

Coins are metal (silver, or shells): nobody prints them. They come into the
village when it sells grain to the merchants and leave when it buys grain
from them, pays taxes in coin or sells to its lord; and each year a little is
lost or buried. A ruler can debase the coinage: the town then asks more
coins for the same grain. The town's purse counts the coins it has paid out
(negative) or taken in, so that every coin is accounted for.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np

from econ_sim.config import Config


@dataclass
class Town:
    level: float  # coins per ration of grain in the town in a normal year, at the start
    swing: float  # the region's random price swing (log), slowly reverting to 0
    purse: float  # coins the town has taken in from the villages, net (negative: paid out)
    coins_lost: float  # coins lost, worn away or buried, since the start

    @classmethod
    def at(cls, level: float) -> Town:
        return cls(level=level, swing=0.0, purse=0.0, coins_lost=0.0)


def price(town: Town, month_of_year: int, farm_weather: np.ndarray, config: Config) -> np.ndarray:
    """The town's grain price this month, per village (the region's harvest
    follows each village's weather in part)."""
    cfg = config.town
    season = 1.0 + cfg.seasonal * np.cos(2 * np.pi * (month_of_year - cfg.dearest_month) / 12)
    dearth = 1.0 + cfg.regional_share * np.maximum(1.0 / np.maximum(farm_weather, 0.2) - 1.0, 0.0)
    return town.level * season * np.exp(town.swing) * dearth


def drift(town: Town, config: Config, rng: np.random.Generator) -> None:
    """A month of the region's random price swing."""
    cfg = config.town
    town.swing = cfg.swing_memory * town.swing + rng.normal(0.0, cfg.swing_sd)


@dataclass
class Merchants:
    exports: np.ndarray  # rations merchants would buy this month, per village (0 if none)
    imports: np.ndarray  # rations merchants offer this month, per village (0 if none)


def merchants(village_price: np.ndarray, town_price: np.ndarray, need: np.ndarray, config: Config) -> Merchants:
    """What merchants will carry this month: grain out when the village is
    cheap, grain in when it is dear."""
    cfg = config.town
    if not cfg.enabled:
        zeros = np.zeros_like(need)
        return Merchants(exports=zeros, imports=zeros.copy())
    cart = cfg.capacity * need
    cheap = village_price < town_price * (1.0 - cfg.transport)
    dear = village_price > town_price * (1.0 + cfg.transport)
    return Merchants(exports=np.where(cheap, cart, 0.0), imports=np.where(dear, cart, 0.0))


def lose_coins(money: np.ndarray, town: Town, config: Config) -> float:
    """A month's share of the coins lost, worn away or buried for good."""
    rate = 1.0 - (1.0 - config.town.coins_lost_a_year) ** (1 / 12)
    lost = money * rate
    money -= lost
    town.coins_lost += float(lost.sum())
    return float(lost.sum())
