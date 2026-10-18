"""Market towns: their grain dealers, what merchants carry between each
village and its town, and the border with the world beyond.

On a map, market towns are places of their own, with people but no
farmland: they work in crafts and buy their grain. Each town's grain
dealers keep a store (apart from the farms' grain, which is the harvest
waiting to be shared out) and coins. They buy grain from the villages that
trade with the town when it is cheap enough there to be worth carting,
sell it in the town, and send it back out to villages in a dearth. What
grain costs in a town is what it costs to bring in (the villages' price
plus carriage), times how short its store runs against what its people
eat, so the town price is set by what the villages bring and the town eats.

Merchants carry cloth, tools and firewood the same way, wherever the gap in
price beats the cost of carting (much less for cloth and tools than for
grain, more for firewood): the town's craftsmen are more skilled, so
villagers buy their cloth and tools in town, and that is how coins go back
to the villages. Goods brought to a village are sold for the town's
business that made them; what is not sold goes back.

Only a border town trades with the world beyond, at the outside price
(`town.price`): it buys grain there when the town is dear and sells when
it is cheap.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np

from econ_sim.config import Config
from econ_sim.geography import Geography

# A village whose carriage costs more than this share of the price still
# reckons its grain worth at least the rest (see `world.MIN_PRICE_LEVEL`).
MIN_GETS = 0.3
# Weight of each month in what a town expects to come in, and what a
# village expects to send (about the last half year).
MEMORY = 1 / 6


def gets(geo: Geography) -> np.ndarray:
    """Share of the town price a place's grain fetches, after carting it there."""
    return np.clip(1.0 - geo.transport, MIN_GETS, 1.0)


def store_markup(markup: np.ndarray, store: np.ndarray, need: np.ndarray, config: Config) -> np.ndarray:
    """Move a town's grain mark-up toward the level set by how its dealers'
    store compares with the usual (`dealers_months` of its need)."""
    usual = config.town.dealers_months * need
    short = np.divide(usual, np.maximum(store, 1e-9), out=np.ones_like(store), where=usual > 0)
    target = np.clip(short ** config.town.store_elasticity, *config.trade.markup_range)
    return markup + config.food.price_adjustment * (target - markup)


def delivered_price(village_price: np.ndarray, geo: Geography, weights: np.ndarray) -> np.ndarray:
    """What grain costs to bring into each town from the villages that trade
    with it (their price, plus carriage), averaged by `weights` (their need);
    a village's own price elsewhere."""
    price = village_price.copy()
    villages = ~geo.town & (geo.market >= 0)
    for town in np.flatnonzero(geo.town):
        mine = villages & (geo.market == town)
        if not mine.any():
            mine = ~geo.town
        if not mine.any():
            continue
        w = np.maximum(weights[mine], 1e-9)
        price[town] = float(np.average(village_price[mine] / gets(geo)[mine], weights=w))
    return price


@dataclass
class Carts:
    """What merchants carry this month. Grain (per place): what the town's
    dealers send `grain_in` to each village (and `grain_held` back from
    each town's own market for it), and what they would buy `grain_out` of
    a village if it is left unsold. Other goods go on consignment, each
    move from place `src` to place `dst` (`goods`, moves x products, the
    food column empty): offered at the price where they arrive and sold
    for the business that made them; what is not sold goes back."""

    grain_in: np.ndarray
    grain_held: np.ndarray
    grain_out: np.ndarray
    src: np.ndarray
    dst: np.ndarray
    goods: np.ndarray

    def incoming(self, n: int) -> np.ndarray:
        """Goods arriving at each place (places x products)."""
        out = np.zeros((n, self.goods.shape[1]))
        np.add.at(out, self.dst, self.goods)
        return out

    def outgoing(self, n: int) -> np.ndarray:
        """Goods leaving each place (places x products)."""
        out = np.zeros((n, self.goods.shape[1]))
        np.add.at(out, self.src, self.goods)
        return out


def carts(prices: np.ndarray, geo: Geography, wanted: np.ndarray, stock: np.ndarray, dealers_cash: np.ndarray,
          weights: np.ndarray, food: int, config: Config) -> Carts:
    """Merchants' loads for this month, between each village and its market
    town, wherever a good is dearer at one end than at the other by more
    than the carting (`transport` x the product's `carriage`), a full load
    once the gap beats it by `full_load_margin`. Into a village: up to
    `MapConfig.capacity` of what it `wanted` of a good last month, from the
    town's `stock` (shared out among its villages if short; for grain, the
    dealers'). Out of a village to its town: as much again, plus its share
    (by `weights`) of what the town wanted, from what it has beyond a month
    of its own orders; grain only as far as the dealers' coins go."""
    n, k = prices.shape
    empty = np.zeros(0, dtype=np.int64)
    villages = np.flatnonzero(~geo.town & (geo.market >= 0) & np.isfinite(geo.transport))
    if not len(villages):
        return Carts(np.zeros(n), np.zeros(n), np.zeros(n), empty, empty, np.zeros((0, k)))
    town = geo.market[villages]
    carriage = np.array([p.carriage for p in config.products])
    cost = np.minimum(geo.transport[villages, None] * carriage[None, :], 0.9)
    here, there = prices[villages], prices[town]
    ramp = config.map.full_load_margin
    with np.errstate(divide="ignore", invalid="ignore"):
        bring = np.clip(np.nan_to_num(here / (there * (1.0 + cost)) - 1.0) / ramp, 0.0, 1.0)
        send = np.clip(np.nan_to_num(there * (1.0 - cost) / here - 1.0, posinf=1.0) / ramp, 0.0, 1.0)
    send = np.where(here > 0, send, 0.0)
    full = config.map.capacity * np.maximum(wanted[villages], 0.0)
    # Into villages: each town shares out what it has among those asking.
    asked = bring * full
    asked_of = np.zeros((n, k))
    np.add.at(asked_of, town, asked)
    can = np.minimum(1.0, np.divide(stock, asked_of, out=np.ones((n, k)), where=asked_of > 0))
    into = asked * can[town]
    # Out of villages: their share of the town's wants too.
    w = np.maximum(weights[villages], 0.0)
    around = np.bincount(town, weights=w, minlength=n)
    share = np.divide(w, around[town], out=np.zeros(len(w)), where=around[town] > 0)
    town_wants = config.map.capacity * np.maximum(wanted[town], 0.0) * share[:, None]
    out = np.minimum(send * (full + town_wants), np.maximum(stock[villages] - wanted[villages], 0.0))
    # Grain out: as far as each town's dealers can pay.
    grain_out = send[:, food] * full[:, food]
    offered_to = np.bincount(town, weights=grain_out * here[:, food], minlength=n)
    afford = np.minimum(1.0, np.divide(dealers_cash, offered_to, out=np.ones(n), where=offered_to > 0))
    grain_in, grain_held, grain_sold = np.zeros(n), np.zeros(n), np.zeros(n)
    grain_in[villages] = into[:, food]
    grain_held = np.bincount(town, weights=into[:, food], minlength=n)
    grain_sold[villages] = grain_out * afford[town]
    into[:, food] = 0.0
    out[:, food] = 0.0
    src = np.concatenate([town, villages])
    dst = np.concatenate([villages, town])
    return Carts(grain_in, grain_held, grain_sold, src, dst, np.concatenate([into, out]))


def settle(cart: Carts, product: int, stock: np.ndarray, cash: np.ndarray, demand: np.ndarray, offer: np.ndarray,
           sold: np.ndarray, spent: np.ndarray, asked: np.ndarray) -> None:
    """After a market in `product` where each place had `offer` (its own
    `stock` plus goods carted in), `sold` and took `spent` coins, with
    buyers having `asked` for as much as `demand`: each unit on offer was as
    likely to sell as any other, so each business (the place's own, and
    each that sent goods) is paid and counts the demand for its share, and
    unsold goods go back. Changes `stock`, `cash` (that product's
    businesses) and `demand` (all per place)."""
    goods = cart.goods[:, product]
    with np.errstate(invalid="ignore"):
        part = np.divide(goods, offer[cart.dst], out=np.zeros(len(goods)), where=offer[cart.dst] > 0)
        local = np.clip(np.divide(stock, offer, out=np.ones_like(offer), where=offer > 0), 0.0, 1.0)
    stock -= np.minimum(sold * local, stock)
    cash += spent * local
    demand[:] = asked * local
    np.add.at(stock, cart.src, goods - sold[cart.dst] * part)
    np.add.at(cash, cart.src, spent[cart.dst] * part)
    np.add.at(demand, cart.src, asked[cart.dst] * part)


def town_of(geo: Geography) -> np.ndarray:
    """Each place's market town (a town is its own), or the place itself if it has none."""
    return np.where(geo.market >= 0, geo.market, np.arange(len(geo.market)))


def border_trade(dealers_grain: np.ndarray, dealers_cash: np.ndarray, town_price: np.ndarray, outside: np.ndarray,
                 need: np.ndarray, geo: Geography, config: Config) -> np.ndarray:
    """Grain a border town's dealers buy from the world beyond (positive) or
    sell to it (negative) this month, at the `outside` price plus or minus
    carting: up to `outside_capacity` of the town's monthly `need`, as
    their coins allow, keeping a month of the town's need in store. Changes
    `dealers_grain` and `dealers_cash`; returns rations per place."""
    cfg = config.town
    moved = np.zeros_like(need)
    if not cfg.enabled:
        return moved
    border = geo.border & geo.town
    buy_at = outside * (1.0 + cfg.outside_transport)
    sell_at = outside * (1.0 - cfg.outside_transport)
    ramp = config.map.full_load_margin
    with np.errstate(divide="ignore", invalid="ignore"):
        buy = np.where(border & (buy_at > 0), np.clip(np.nan_to_num(town_price / buy_at - 1.0) / ramp, 0.0, 1.0), 0.0)
        sell = np.where(border & (town_price > 0), np.clip(np.nan_to_num(sell_at / town_price - 1.0) / ramp, 0.0, 1.0), 0.0)
    load = cfg.outside_capacity * need
    bought = np.minimum(buy * load, np.divide(dealers_cash, buy_at, out=np.zeros_like(need), where=buy_at > 0))
    sold = np.minimum(sell * load, np.maximum(dealers_grain - need, 0.0))
    dealers_grain += bought - sold
    dealers_cash += sold * sell_at - bought * buy_at
    moved = bought - sold
    return moved


def outside_coins(moved: np.ndarray, outside: np.ndarray, config: Config) -> float:
    """Coins the world beyond takes in (net) for the grain of `border_trade`."""
    cfg = config.town
    bought, sold = np.maximum(moved, 0.0), np.maximum(-moved, 0.0)
    return float((bought * outside * (1.0 + cfg.outside_transport) - sold * outside * (1.0 - cfg.outside_transport)).sum())


def dealers_payout(dealers_cash: np.ndarray, need: np.ndarray, price: np.ndarray, config: Config) -> np.ndarray:
    """Coins the grain dealers pay out this month (merchants' and carters'
    earnings): `dealers_payout` of what they hold beyond `dealers_months`
    of the town's need at its price. Changes `dealers_cash`; returns coins
    per place."""
    cfg = config.town
    working = cfg.dealers_months * need * price
    paid = cfg.dealers_payout * np.maximum(dealers_cash - working, 0.0)
    dealers_cash -= paid
    return paid
