"""The map: where villages lie, the country around them, the roads and
rivers to their market towns, and the weather that falls on it.

Each village trades grain with its market town (the one it can reach most
cheaply). Carting grain costs a share of its price for loading, tolls, dues
and the merchant's margin, plus so much a km: by road, more across hills
and through woods; by boat on a river, about half as much. So villages near
a town or on the river sell their grain and buy in a dearth; remote ones
live off their own.

The weather is a smooth random field over the map, drawn afresh each season:
neighbouring villages share their droughts, good years and hard winters,
and the weather changes over a few hundred km, so a region suffers a dearth
together while a province may have a drought in the east and a good year in
the west. Each village still has its usual chance of each kind of season.

`region` lays out a region from a few settings: villages on a rough grid,
hills on one side, a river across it with a market town on its bank, woods,
and roads to each village's nearest neighbours and towns.
"""

from __future__ import annotations

import heapq
import math
import zlib
from dataclasses import dataclass, replace

import numpy as np

from econ_sim.config import Config, MapConfig, VillageConfig

WEATHER_FEATURES = 256  # waves summed to make a weather field


@dataclass
class Geography:
    """Where each village lies and what its country is like."""

    x: np.ndarray  # km east
    y: np.ndarray  # km north
    terrain: np.ndarray  # index into MapConfig.terrains
    transport: np.ndarray  # share of the grain price it costs to cart grain to or from its market town
    market: np.ndarray  # each village's market town (-1: none it can reach)
    pasture: np.ndarray  # animals it can winter, against the usual
    woods: np.ndarray  # woods it can hold, against the usual
    forage: np.ndarray  # famine foods to be found, against the usual
    on_map: bool  # False: no map (every village the same distance from the town, with its own weather)

    def distances(self, origins: np.ndarray) -> np.ndarray:
        """Km as the crow flies from each of `origins` to every village (origins x villages)."""
        return np.hypot(self.x[None, :] - self.x[origins, None], self.y[None, :] - self.y[origins, None])

    def near(self, point: tuple[float, float, float]) -> np.ndarray:
        """Villages within `point[2]` km of (point[0], point[1])."""
        x, y, km = point
        return np.hypot(self.x - x, self.y - y) <= km


def build(config: Config) -> Geography:
    villages, cfg = config.villages, config.map
    names = [t.name for t in cfg.terrains]
    for village in villages:
        if village.terrain not in names:
            raise ValueError(f"{village.name}: unknown terrain {village.terrain!r}; choose from: {', '.join(names)}")
    x = np.array([v.x for v in villages], dtype=np.float64)
    y = np.array([v.y for v in villages], dtype=np.float64)
    terrain = np.array([names.index(v.terrain) for v in villages], dtype=np.int64)
    specs = cfg.terrains
    if cfg.enabled:
        transport, market = carriage(config)
    else:
        transport, market = np.full(len(villages), config.town.transport), np.zeros(len(villages), dtype=np.int64)
    return Geography(
        x=x, y=y, terrain=terrain, transport=transport, market=market,
        pasture=np.array([specs[t].pasture for t in terrain]),
        woods=np.array([specs[t].woods for t in terrain]),
        forage=np.array([specs[t].forage for t in terrain]),
        on_map=cfg.enabled,
    )


def links(config: Config) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """Every road and river leg as (from, to, cost as a share of the grain
    price); places are numbered villages first, then towns."""
    cfg, villages = config.map, config.villages
    x = np.array([v.x for v in villages] + [t[0] for t in cfg.towns], dtype=np.float64)
    y = np.array([v.y for v in villages] + [t[1] for t in cfg.towns], dtype=np.float64)
    carriage = [cfg.terrains[cfg.terrain_index(v.terrain)].carriage for v in villages] + [1.0] * len(cfg.towns)
    carriage = np.array(carriage)
    ends, costs = [], []
    for legs, per_km, by_road in ((cfg.roads, cfg.road_per_km, True), (cfg.rivers, cfg.river_per_km, False)):
        if not legs:
            continue
        pairs = np.array(legs, dtype=np.int64)
        if pairs.min() < 0 or pairs.max() >= len(x):
            raise ValueError(f"a road or river joins place {pairs.max()}, but there are only {len(x)} (villages, then towns)")
        a, b = pairs[:, 0], pairs[:, 1]
        km = np.hypot(x[a] - x[b], y[a] - y[b])
        cost = per_km * km * (0.5 * (carriage[a] + carriage[b]) if by_road else 1.0)
        ends.append(pairs)
        costs.append(cost)
    if not ends:
        return np.zeros(0, dtype=np.int64), np.zeros(0, dtype=np.int64), np.zeros(0)
    pairs, cost = np.concatenate(ends), np.concatenate(costs)
    return pairs[:, 0], pairs[:, 1], cost


def cheapest(n_places: int, a: np.ndarray, b: np.ndarray, cost: np.ndarray, sources: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Cheapest cost from any of `sources` to every place along the links
    (both ways), and which source it comes from (-1 if none reaches it)."""
    neighbours: list[list[tuple[int, float]]] = [[] for _ in range(n_places)]
    for i, j, c in zip(a.tolist(), b.tolist(), cost.tolist()):
        neighbours[i].append((j, c))
        neighbours[j].append((i, c))
    best = np.full(n_places, np.inf)
    origin = np.full(n_places, -1, dtype=np.int64)
    queue = []
    for k, s in enumerate(sources.tolist()):
        best[s], origin[s] = 0.0, k
        queue.append((0.0, s, k))
    heapq.heapify(queue)
    while queue:
        so_far, place, k = heapq.heappop(queue)
        if so_far > best[place]:
            continue
        for other, c in neighbours[place]:
            if so_far + c < best[other]:
                best[other], origin[other] = so_far + c, k
                heapq.heappush(queue, (so_far + c, other, k))
    return best, origin


def carriage(config: Config) -> tuple[np.ndarray, np.ndarray]:
    """(transport, market) per village: the share of the grain price it
    costs to cart grain between it and its cheapest market town, and that
    town (-1, and no trade, if no road or river reaches one)."""
    cfg = config.map
    n, m = len(config.villages), len(cfg.towns)
    a, b, cost = links(config)
    best, origin = cheapest(n + m, a, b, cost, np.arange(n, n + m))
    transport = np.where(np.isfinite(best[:n]), cfg.handling + best[:n], np.inf)
    return transport, origin[:n]


def weather(geo: Geography, scale_km: float, rng: np.random.Generator, features: int = WEATHER_FEATURES) -> np.ndarray:
    """A season's weather at each village, as a share between 0 (the worst
    season) and 1 (the best): uniform at each village, and alike at villages
    close together (a smooth Gaussian random field whose correlation falls
    to a half at about 1.2 x `scale_km`, from random Fourier features)."""
    waves = rng.normal(0.0, 1.0 / scale_km, size=(features, 2))
    phase = rng.uniform(0.0, 2.0 * np.pi, size=features)
    field = np.sqrt(2.0 / features) * np.cos(np.outer(geo.x, waves[:, 0]) + np.outer(geo.y, waves[:, 1]) + phase).sum(axis=1)
    return normal_share(field)


def normal_share(z: np.ndarray) -> np.ndarray:
    """The standard normal distribution function."""
    return 0.5 * np.vectorize(math.erfc, otypes=[np.float64])(-np.asarray(z) / math.sqrt(2.0))


# ---------------------------------------------------------------------------
# Laying out a region


@dataclass(frozen=True)
class RegionSettings:
    """A few settings for `region`."""

    villages: int = 30
    mean_size: float = 400.0  # people in a village of the plain, on average
    size_spread: float = 0.5  # spread of village sizes (sd of their log)
    smallest: int = 40
    spacing_km: float = 3.0  # between neighbouring villages
    towns: int | None = None  # market towns; None: one for every 25 villages
    upland: float = 0.2  # share of villages in the hills (to the west)
    forest: float = 0.15  # share of the rest in the woods
    river: bool = True  # a river across the region, west to east
    coast: bool = False  # a coast along the east side
    seed: int = 1


# Villages are bigger where the land is richest, smaller in hills and woods.
SIZE_BY_TERRAIN = {"plain": 1.0, "valley": 1.3, "upland": 0.6, "forest": 0.7, "coast": 0.9}


def region(settings: RegionSettings = RegionSettings(), base: Config | None = None) -> Config:
    """A region laid out from `settings`, on top of `base` (its other settings)."""
    base = base or Config()
    s = settings
    if s.villages < 1:
        raise ValueError("a region needs at least one village")
    rng = np.random.default_rng([s.seed, zlib.crc32(b"region")])
    n = s.villages
    k = math.ceil(math.sqrt(n))
    side = s.spacing_km * k
    cells = rng.choice(k * k, size=n, replace=False)
    x = (cells % k + 0.2 + 0.6 * rng.random(n)) * s.spacing_km
    y = (cells // k + 0.2 + 0.6 * rng.random(n)) * s.spacing_km

    # Hills to the west, a river from west to east, woods here and there,
    # perhaps a coast to the east.
    terrain = np.full(n, "plain", dtype=object)
    height = _smooth(x, y, side / 2, rng) + 1.5 * (1.0 - x / side)
    woods = _smooth(x, y, side / 3, rng)
    bend, wiggle = rng.uniform(0, 2 * np.pi), rng.uniform(0.8, 1.6)

    def river_y(at: np.ndarray) -> np.ndarray:
        return side / 2 + side / 6 * np.sin(2 * np.pi * wiggle * at / side + bend)

    on_river = (np.abs(y - river_y(x)) < 0.6 * s.spacing_km) if s.river else np.zeros(n, dtype=bool)
    hills = (height >= np.quantile(height, 1.0 - s.upland)) if s.upland > 0 else np.zeros(n, dtype=bool)
    terrain[hills] = "upland"
    rest = ~hills & ~on_river
    if s.forest > 0 and rest.any():
        terrain[rest & (woods >= np.quantile(woods[rest], 1.0 - s.forest))] = "forest"
    terrain[on_river] = "valley"
    if s.coast:
        terrain[x > side - s.spacing_km] = "coast"

    size_factor = np.array([SIZE_BY_TERRAIN.get(t, 1.0) for t in terrain])
    sizes = s.mean_size * size_factor * np.exp(s.size_spread * rng.normal(size=n) - s.size_spread**2 / 2)
    sizes = np.maximum(np.round(sizes), s.smallest).astype(int)

    # Market towns where the people are; the first on the river.
    m = s.towns or max(1, round(n / 25))
    towns = _centres(np.column_stack([x, y]), sizes.astype(np.float64), min(m, n), rng)
    if s.river:
        first = np.argmin(np.abs(towns[:, 1] - river_y(towns[:, 0])))
        towns[first, 1] = river_y(towns[first, :1])[0]
    tx, ty = towns[:, 0], towns[:, 1]

    # Roads to each village's nearest neighbours and from each town to its
    # nearest villages, joined up so every place can be reached.
    px, py = np.concatenate([x, tx]), np.concatenate([y, ty])
    roads = set()
    for i in range(n):
        d = np.hypot(x - x[i], y - y[i])
        d[i] = np.inf
        for j in np.argsort(d)[: min(3, n - 1)]:
            roads.add((min(i, int(j)), max(i, int(j))))
    for t in range(len(towns)):
        d = np.hypot(x - tx[t], y - ty[t])
        for j in np.argsort(d)[: min(4, n)]:
            roads.add((int(j), n + t))
    roads = _join_up(px, py, roads)
    # The river links the villages on its banks, and its town, in order downstream.
    rivers = []
    if s.river:
        river_town = np.abs(ty - river_y(tx)) < 0.6 * s.spacing_km
        banks = np.concatenate([np.flatnonzero(on_river), n + np.flatnonzero(river_town)])
        order = banks[np.argsort(px[banks])]
        rivers = [(int(a), int(b)) for a, b in zip(order[:-1], order[1:])]

    village = base.villages[0]
    per_person = village.land / village.population
    villages = tuple(
        VillageConfig(
            name=f"{str(terrain[i]).title()} {i + 1}", population=int(sizes[i]), land=round(per_person * sizes[i], 1),
            initial_food_months=village.initial_food_months, x=round(float(x[i]), 2), y=round(float(y[i]), 2),
            terrain=str(terrain[i]),
        )
        for i in range(n)
    )
    world_map = replace(
        base.map, enabled=True, towns=tuple((round(float(a), 2), round(float(b), 2)) for a, b in towns),
        roads=tuple(sorted(roads)), rivers=tuple(rivers),
    )
    return replace(base, villages=villages, map=world_map)


def _smooth(x: np.ndarray, y: np.ndarray, scale: float, rng: np.random.Generator) -> np.ndarray:
    """A smooth random surface over the villages (standard normal at each)."""
    waves = rng.normal(0.0, 1.0 / scale, size=(64, 2))
    phase = rng.uniform(0.0, 2.0 * np.pi, size=64)
    return np.sqrt(2.0 / 64) * np.cos(np.outer(x, waves[:, 0]) + np.outer(y, waves[:, 1]) + phase).sum(axis=1)


def _centres(points: np.ndarray, weights: np.ndarray, m: int, rng: np.random.Generator) -> np.ndarray:
    """`m` centres of the weighted `points` (k-means, started far apart)."""
    middle = points.mean(axis=0)
    chosen = [int(np.argmin(np.hypot(*(points - middle).T)))]
    for _ in range(m - 1):
        d = np.min(np.hypot(points[:, None, 0] - points[chosen, 0], points[:, None, 1] - points[chosen, 1]), axis=1)
        chosen.append(int(np.argmax(d)))
    centres = points[chosen].copy()
    for _ in range(20):
        d = np.hypot(points[:, None, 0] - centres[None, :, 0], points[:, None, 1] - centres[None, :, 1])
        nearest = d.argmin(axis=1)
        for c in range(m):
            mine = nearest == c
            if mine.any():
                centres[c] = np.average(points[mine], axis=0, weights=weights[mine])
    return centres


def _join_up(x: np.ndarray, y: np.ndarray, roads: set[tuple[int, int]]) -> set[tuple[int, int]]:
    """Add the shortest roads needed so that every place can be reached."""
    n = len(x)
    parent = list(range(n))

    def root(i: int) -> int:
        while parent[i] != i:
            parent[i] = parent[parent[i]]
            i = parent[i]
        return i

    for a, b in roads:
        parent[root(a)] = root(b)
    while True:
        groups = np.array([root(i) for i in range(n)])
        first = groups == groups[0]
        if first.all():
            return roads
        inside, outside = np.flatnonzero(first), np.flatnonzero(~first)
        d = np.hypot(x[inside, None] - x[None, outside], y[inside, None] - y[None, outside])
        i, j = np.unravel_index(np.argmin(d), d.shape)
        a, b = int(inside[i]), int(outside[j])
        roads.add((min(a, b), max(a, b)))
        parent[root(a)] = root(b)


def describe(config: Config, geo: Geography) -> str:
    """One line about the map: villages, people, towns, terrain, carriage."""
    cfg: MapConfig = config.map
    people = sum(v.population for v in config.villages)
    kinds = [cfg.terrains[t].name for t in geo.terrain]
    counts = ", ".join(f"{kinds.count(t.name)} {t.name}" for t in cfg.terrains if kinds.count(t.name))
    reachable = geo.transport[np.isfinite(geo.transport)]
    cost = f"{reachable.min():.0%}-{reachable.max():.0%}" if len(reachable) else "none reach a town"
    towns = len(cfg.towns)
    return (
        f"Region of {len(config.villages)} villages ({people:,} people) around {towns} market town{'s' * (towns != 1)}: "
        f"{counts}; carting grain to market costs {cost} of its price"
    )
