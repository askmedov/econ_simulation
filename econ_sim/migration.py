"""Migration: who leaves a village and who comes, by how good a place it is.

How good a place to live a village is (1 = an ordinary one) depends on its
livelihood (what a wage or a share of the land feeds), on what people
remember of it (recent hunger, and danger: deaths beyond the usual from
plague, famine or raiders, and grain plundered by soldiers), on its size (a
handful of families can't keep a plough team, a church or a market, and
have nobody to marry) and on how much its lord takes.

Young single people leave each year by chance, and more when the place is
worse than ordinary; families leave too, the landless more readily than
those with land, and people follow those who went before, so a village that
starts to empty can empty out. In a famine the hungriest families flee.
While plague or raiders are about, families flee them too. With several
villages, half of those who leave settle in another that is a better place,
bringing their coins and grain; the rest go to the town or beyond, taking
theirs. A family that leaves for good leaves its land behind
(it goes to a landless family) and its debts unpaid. Young people from
beyond come to a village that is clearly a better place than ordinary, as
servants of its landholders: never to one that is dangerous or hungry.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np

from econ_sim import rules
from econ_sim.config import Config
from econ_sim.households import Households, sizes
from econ_sim.population import NO_JOB, Population


@dataclass
class Memory:
    """What people in and around a village remember of it, per village."""

    hunger: np.ndarray  # recent share of need not eaten
    danger: np.ndarray  # recent deaths beyond the usual (a yearly share), and plunder
    leaving: np.ndarray  # recent emigration (a yearly share of the village)
    danger_now: np.ndarray  # this month's danger

    @classmethod
    def calm(cls, n_locations: int) -> Memory:
        zeros = np.zeros(n_locations)
        return cls(hunger=zeros.copy(), danger=zeros.copy(), leaving=zeros.copy(), danger_now=zeros.copy())


def remember(
    memory: Memory, ration: np.ndarray, deaths: np.ndarray, people: np.ndarray, plundered: np.ndarray,
    config: Config, mortality_mult: np.ndarray | None = None,
) -> None:
    """Add a month's hunger and danger to the memory (older months fade).
    Danger is the yearly share of people lost beyond the usual: deaths
    beyond what the usual mortality and chance would bring (in a hamlet one
    death more is luck, not famine), or, while an epidemic or raiders are
    known to be about, the deaths they are known to bring
    (`mortality_mult`), whichever is more; plus grain plundered."""
    cfg = config.migration
    fade = 1.0 / cfg.memory_months
    usual = people * cfg.normal_mortality / 12.0
    beyond = np.maximum(deaths - usual - 2.0 * np.sqrt(usual), 0.0)
    danger = np.divide(12.0 * beyond, people, out=np.zeros_like(people), where=people > 0)
    if mortality_mult is not None:
        danger = np.maximum(danger, (mortality_mult - 1.0) * cfg.normal_mortality)
    danger += cfg.plunder_weight * 12.0 * plundered
    memory.hunger += fade * (np.maximum(1.0 - ration, 0.0) - memory.hunger)
    memory.danger += fade * (danger - memory.danger)
    memory.danger_now = danger


def remember_leaving(memory: Memory, left: np.ndarray, people: np.ndarray) -> None:
    """Add a month's departures (a year's memory)."""
    yearly = np.divide(12.0 * left, people + left, out=np.zeros_like(left), where=people + left > 0)
    memory.leaving += (yearly - memory.leaving) / 12.0


def appeal(
    prospects: np.ndarray, memory: Memory, people: np.ndarray, land: np.ndarray, lord_take: np.ndarray, config: Config
) -> np.ndarray:
    """How good a place to live each village is, 1 being an ordinary one."""
    cfg = config.migration
    livelihood = prospects / cfg.usual_prospects
    safety = np.exp(-cfg.hunger_weight * memory.hunger - cfg.danger_weight * memory.danger)
    size = np.clip(people / cfg.small_village, 0.0, 1.0) ** cfg.size_exponent
    # Land to spare draws people and crowding sends them away: what a
    # family's share of the land would yield, against the usual density.
    usual_land = people / config.environment.people_per_plot
    spare = np.divide(land, usual_land, out=np.ones_like(land), where=usual_land > 0)
    room = np.clip(spare ** (1.0 - config.food.labor_share), *cfg.land_range)
    lightness = np.divide(cfg.usual_take, lord_take, out=np.full_like(lord_take, np.inf), where=lord_take > 0)
    burden = np.clip(lightness ** cfg.burden_exponent, *cfg.burden_range)
    return livelihood * safety * size * room * burden


@dataclass
class Moves:
    left: np.ndarray  # people who left, per village
    arrived: np.ndarray  # people who came, from beyond or from another village, per village
    families: int  # whole families that left
    within: int  # people who moved to another village of the run
    coins: float  # coins taken beyond the villages
    grain: float  # rations taken beyond the villages


def _destinations(origin: np.ndarray, place: np.ndarray, people: np.ndarray, config: Config,
                  rng: np.random.Generator) -> np.ndarray:
    """For each leaver from `origin`, another village that is a better place
    (chosen by its people times how much better), or -1 for beyond."""
    dest = np.full(len(origin), -1, dtype=np.int64)
    if len(place) < 2 or not len(origin):
        return dest
    stays = rng.random(len(origin)) < config.migration.stay_in_region
    better = np.maximum(place[None, :] - place[origin][:, None], 0.0) * people[None, :]
    better[np.arange(len(origin)), origin] = 0.0
    total = better.sum(axis=1)
    settle = np.flatnonzero(stays & (total > 0))
    if len(settle):
        cumulative = np.cumsum(better[settle], axis=1) / total[settle, None]
        dest[settle] = np.minimum((rng.random(len(settle))[:, None] >= cumulative).sum(axis=1), len(place) - 1)
    return dest


def move(
    population: Population, households: Households, place: np.ndarray, memory: Memory, family_share: np.ndarray,
    config: Config, rng: np.random.Generator,
) -> Moves:
    """This month's departures: young singles and whole families, by how
    good a place their village is (`place`), and the hungriest families in
    a famine. Those settling in another village move there; the rest leave."""
    cfg = config.migration
    n, n_hh = len(place), len(households)
    none = Moves(left=np.zeros(n), arrived=np.zeros(n), families=0, within=0, coins=0.0, grain=0.0)
    if not cfg.enabled or len(population) == 0:
        return none
    loc = population.location
    alive = population.count > 0  # rows emptied this month are dropped at its end
    age = population.age_years
    worse = np.maximum(1.0 - place, 0.0)
    follow = 1.0 + cfg.chain * memory.leaving

    # Young single people.
    young = alive & (population.count == 1) & ~population.married & (age >= cfg.young_ages[0]) & (age <= cfg.young_ages[1])
    roll = np.ones(len(population))
    roll[alive] = rng.random(int(alive.sum()))
    goes = young & (roll < (rules.monthly_chance(cfg.leave_chance) * (1.0 + cfg.push * worse) * follow)[loc])

    # Whole families, the landless more readily, and the hungriest in a famine.
    size = sizes(population, n_hh)
    living = size > 0
    yearly = cfg.family_leave_chance * np.where(households.land > 0, cfg.rooted, 1.0)
    yearly = yearly * ((1.0 + cfg.push * worse) * follow)[households.location]
    families = living & (rng.random(n_hh) < rules.monthly_chance(np.minimum(yearly, 1.0)))
    families |= living & (family_share < cfg.flee_below) & (rng.random(n_hh) < cfg.flee_chance)
    families |= living & (rng.random(n_hh) < cfg.flee_danger * memory.danger_now[households.location])
    in_family = alive & families[population.household]
    goes &= ~in_family
    if not (goes.any() or families.any()):
        return none

    # Where they go.
    people = rules.by_location(population.count.astype(np.float64), population, n)
    singles, family_ids = np.flatnonzero(goes), np.flatnonzero(families)
    to_single = _destinations(loc[singles], place, people, config, rng)
    to_family = _destinations(households.location[family_ids], place, people, config, rng)
    # A single person settles with a landholding family there, if any.
    hosts = np.flatnonzero((households.land > 0) & living)
    hosts = hosts[np.argsort(households.location[hosts], kind="stable")]
    host_places = households.location[hosts]
    first = np.searchsorted(host_places, np.arange(n))
    last = np.searchsorted(host_places, np.arange(n), side="right")
    settled = to_single >= 0
    settled[settled] = last[to_single[settled]] > first[to_single[settled]]
    to_single[~settled] = -1
    host = np.full(len(singles), -1, dtype=np.int64)
    if settled.any():
        d = to_single[settled]
        host[settled] = hosts[first[d] + np.floor(rng.random(len(d)) * (last - first)[d]).astype(np.int64)]

    # What they take: a single person their share of the family's coins and
    # grain, a family all of it.
    share = np.zeros(n_hh)
    np.add.at(share, population.household[singles], 1.0)
    share = np.divide(share, size, out=np.zeros(n_hh), where=size > 0)
    share[family_ids] = 1.0
    coins, grain = households.money * share, households.grain * share
    households.money -= coins
    households.grain -= grain

    left = rules.by_location(population.count * (goes | in_family), population, n)
    arrived = np.zeros(n)
    beyond_coins = beyond_grain = 0.0

    # Singles: beyond, or to their host's family.
    each_coins = coins[population.household[singles]] / np.maximum(size[population.household[singles]] * share[population.household[singles]], 1.0)
    each_grain = grain[population.household[singles]] / np.maximum(size[population.household[singles]] * share[population.household[singles]], 1.0)
    out = ~settled
    beyond_coins += float(each_coins[out].sum())
    beyond_grain += float(each_grain[out].sum())
    population.count[singles[out]] = 0
    np.add.at(households.money, host[settled], each_coins[settled])
    np.add.at(households.grain, host[settled], each_grain[settled])
    moved_singles = singles[settled]
    population.household[moved_singles] = host[settled]
    population.location[moved_singles] = to_single[settled]
    np.add.at(arrived, to_single[settled], 1.0)

    # Families: beyond, or a new household in the village they chose.
    out = to_family < 0
    beyond_coins += float(coins[family_ids[out]].sum())
    beyond_grain += float(grain[family_ids[out]].sum())
    population.count[np.isin(population.household, family_ids[out]) & alive] = 0
    staying = family_ids[~out]
    if len(staying):
        homes = households.add(to_family[~out])
        households.money[homes] = coins[staying]
        households.grain[homes] = grain[staying]
        new_home = np.full(n_hh, -1, dtype=np.int64)
        new_home[staying] = homes
        rows = np.flatnonzero(alive & (new_home[population.household] >= 0))
        np.add.at(arrived, to_family[~out][np.searchsorted(staying, population.household[rows])], population.count[rows])
        population.household[rows] = new_home[population.household[rows]]
        population.location[rows] = households.location[population.household[rows]]
        moved_singles = np.concatenate([moved_singles, rows])
    # Council officials and healers serve their own village only.
    staff = population.job[moved_singles] >= len(config.businesses)
    population.job[moved_singles[staff]] = NO_JOB

    return Moves(
        left=left, arrived=arrived, families=int(len(family_ids)), within=int(arrived.sum()),
        coins=beyond_coins, grain=beyond_grain,
    )


def arrive(
    population: Population, households: Households, place: np.ndarray, room: np.ndarray, config: Config,
    rng: np.random.Generator,
) -> np.ndarray:
    """Young people come from beyond to villages that are clearly better
    places than ordinary (`place` above `welcome`), and settlers to holdings
    left empty (the `room` on the land beyond the people there) where the
    place is safe enough; they join landholding families. Returns arrivals
    per village."""
    cfg = config.migration
    n = len(place)
    arrived = np.zeros(n)
    if not cfg.enabled:
        return arrived
    people = rules.by_location(population.count.astype(np.float64), population, n)
    expected = cfg.arrive_rate * people / 1000.0 * np.clip(place - cfg.welcome, 0.0, cfg.max_pull)
    safe_enough = np.clip((place - cfg.settle_appeal) / (cfg.welcome - cfg.settle_appeal), 0.0, 1.0)
    expected += cfg.settle_rate / 12.0 * np.maximum(room - people, 0.0) * safe_enough
    count = rng.poisson(expected)
    hosting = np.flatnonzero((households.land > 0) & (sizes(population, len(households)) > 0))
    # Host families grouped by village; a village with none takes nobody.
    hosting = hosting[np.argsort(households.location[hosting], kind="stable")]
    places = households.location[hosting]
    first, last = np.searchsorted(places, np.arange(n)), np.searchsorted(places, np.arange(n), side="right")
    count = np.where(last > first, count, 0)
    k = int(count.sum())
    if k == 0:
        return arrived
    village = np.repeat(np.arange(n), count)
    home = hosting[first[village] + np.floor(rng.random(k) * (last - first)[village]).astype(np.int64)]
    population.append(Population(
        count=np.ones(k, dtype=np.int64),
        age_months=rng.integers(cfg.young_ages[0] * 12, cfg.young_ages[1] * 12, size=k),
        female=rng.random(k) < 0.5,
        health=np.full(k, 90.0),
        skill=rules.draw_skill(np.ones(k, dtype=np.int64), config.skill, rng),
        location=village,
        household=home,
        job=np.full(k, NO_JOB),
    ))
    return count.astype(np.float64)
