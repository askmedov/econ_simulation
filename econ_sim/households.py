"""Families: who lives, earns and eats together.

Each person row has a `household` index into the Households table. Families
are formed at the start; newborns join their mother's family, and young
people marry: the first of a family to marry stays and brings their spouse
home (the heir), later ones set up a household of their own.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np

from econ_sim import rules
from econ_sim.config import Config
from econ_sim.population import Population

ADULT_AGE = 18
HEAD_AGES = (18, 64)  # married women of these ages head a household at the start
PARTNER_MAX_AGE = 64
MOTHER_AGE_GAP = (18, 45)  # a child joins a woman this much older


@dataclass
class Households:
    location: np.ndarray  # village of each household
    money: np.ndarray | None = None  # savings, in coins
    land: np.ndarray | None = None  # farmland held, in plots
    grain: np.ndarray | None = None  # food in the family's own store, in rations
    animals: np.ndarray | None = None  # livestock units
    debt: np.ndarray | None = None  # coins owed to the village's lenders
    lent: np.ndarray | None = None  # coins the family is owed (its claim on the village's borrowers)
    kin: np.ndarray | None = None  # the family this one came from (-1: none known)

    def __post_init__(self) -> None:
        self.location = np.asarray(self.location, dtype=np.int32)
        for name in ("money", "land", "grain", "animals", "debt", "lent"):
            value = getattr(self, name)
            setattr(self, name, np.zeros(len(self.location)) if value is None else np.asarray(value, dtype=np.float64))
        n = len(self.location)
        self.kin = np.full(n, -1, dtype=np.int64) if self.kin is None else np.asarray(self.kin, dtype=np.int64)

    def __len__(self) -> int:
        return len(self.location)

    def add(self, location: np.ndarray) -> np.ndarray:
        """Append empty households in these villages; returns their ids."""
        first = len(self)
        self.location = np.concatenate([self.location, np.asarray(location, dtype=np.int32)])
        for name in ("money", "land", "grain", "animals", "debt", "lent"):
            setattr(self, name, np.concatenate([getattr(self, name), np.zeros(len(location))]))
        self.kin = np.concatenate([self.kin, np.full(len(location), -1, dtype=np.int64)])
        return first + np.arange(len(location))


def form_households(
    population: Population, n_locations: int, rng: np.random.Generator, yearly_marriage: float = 1.0
) -> Households:
    """Group the starting villagers into families and set `population.household`
    and `population.married`.

    Women of family age head a household, with a husband of similar age if
    there is one. A young woman has married with `yearly_marriage` for each
    year since turning 18; the others, like children and young men without a
    wife, live with a woman old enough to be their mother.
    """
    household = np.full(len(population), -1, dtype=np.int64)
    population.married = population.age_years >= ADULT_AGE
    population.couple = np.full(len(population), -1, dtype=np.int64)
    locations: list[int] = []
    for location in range(n_locations):
        rows = np.flatnonzero(population.location == location)
        if len(rows) == 0:
            continue
        first = len(locations)
        count = _form_village(population, rows, first, household, yearly_marriage, rng)
        locations.extend([location] * count)
    population.household = household
    return Households(location=np.array(locations))


def _form_village(
    population: Population,
    rows: np.ndarray,
    first: int,
    household: np.ndarray,
    yearly_marriage: float,
    rng: np.random.Generator,
) -> int:
    """Form one village's households, numbered from `first`; returns how many."""
    age = population.age_years[rows]
    female = population.female[rows]

    women = female & (age >= HEAD_AGES[0]) & (age <= HEAD_AGES[1])
    still_single = rng.random(len(rows)) < (1.0 - yearly_marriage) ** (age - ADULT_AGE + 0.5)
    heads = rows[women & ~still_single]
    if len(heads) == 0:
        heads = rows[women]
    if len(heads) == 0:  # no women of family age: everyone lives together
        household[rows] = first
        return 1
    heads = heads[np.argsort(population.age_years[heads], kind="stable")]
    ids = first + np.arange(len(heads))
    household[heads] = ids

    # Men pair with women in age order, so partners are of similar age. If
    # there are more men than wives, the youngest stay single.
    men = rows[~female & (age >= ADULT_AGE) & (age <= PARTNER_MAX_AGE)]
    men = men[np.argsort(population.age_years[men], kind="stable")]
    pairs = min(len(men), len(heads))
    household[men[len(men) - pairs :]] = ids[:pairs]
    population.couple[heads[:pairs]] = ids[:pairs]
    population.couple[men[len(men) - pairs :]] = ids[:pairs]
    single = np.concatenate([rows[women & (household[rows] < 0)], men[: len(men) - pairs]])
    population.married[single] = False

    # Children and the single young join a woman old enough to be their mother.
    ages = population.age_years
    head_ages = ages[heads]
    young = rows[(age < ADULT_AGE)]
    for child in np.concatenate([young, single]):
        gap = head_ages - ages[child]
        mothers = ids[(gap >= MOTHER_AGE_GAP[0]) & (gap <= MOTHER_AGE_GAP[1])]
        household[child] = rng.choice(mothers if len(mothers) else ids)

    # Everyone else (older people) lives with a family.
    rest = rows[household[rows] < 0]
    household[rest] = rng.choice(ids, size=len(rest))
    return len(heads)


def link_kin(households: Households, population: Population, rng: np.random.Generator) -> None:
    """At the start, link each family to a family of its village a
    generation older (its oldest member 18 to 45 years older), if there is
    one: the family it came from."""
    n = len(households)
    oldest = np.full(n, -1, dtype=np.int64)
    np.maximum.at(oldest, population.household, population.age_years.astype(np.int64))
    # Families in order of village, then of their oldest member's age: each
    # family's possible kin are one run of that order.
    key = households.location.astype(np.int64) * 1000 + oldest
    order = np.argsort(key, kind="stable")
    ordered = key[order]
    low = np.searchsorted(ordered, key + 18, side="left")
    high = np.searchsorted(ordered, key + 45, side="right")
    found = (oldest >= 0) & (high > low)
    pick = low + np.floor(rng.random(n) * (high - low)).astype(np.int64)
    households.kin = np.where(found, order[np.minimum(pick, n - 1)], -1)


def assign_land(
    households: Households, population: Population, village_land: np.ndarray, config: Config, rng: np.random.Generator
) -> None:
    """Share each village's farmland among its households at the start.

    `landless_share` of households hold none (they live by wages and
    crafts); the others' holdings are spread log-normally (a few big
    farms, many small ones), larger for families with more workers.
    """
    cfg = config.land
    n = len(households)
    size = sizes(population, n)
    adults = np.bincount(population.household, weights=population.count * (population.age_years >= 15), minlength=n)
    weight = np.maximum(adults, 1.0) * rng.lognormal(0.0, cfg.holding_spread, size=n)
    weight[rng.random(n) < cfg.landless_share] = 0.0
    weight[size == 0] = 0.0
    total = np.bincount(households.location, weights=weight, minlength=len(village_land))
    # Every village has someone holding its land: the family with most adults.
    for village in np.flatnonzero(total <= 0):
        here = np.flatnonzero((households.location == village) & (size > 0))
        if len(here):
            weight[here[np.argmax(adults[here])]] = 1.0
    total = np.bincount(households.location, weights=weight, minlength=len(village_land))
    households.land = np.divide(
        weight * village_land[households.location], total[households.location],
        out=np.zeros(n), where=total[households.location] > 0,
    )


def pass_on_land_and_grain(
    households: Households, size: np.ndarray, reserve: np.ndarray, council: np.ndarray
) -> np.ndarray:
    """Land and grain of families with nobody left.

    A vacant holding, with its animals, goes whole to a landless family of
    the village (the newest, usually a young couple), or if there is none
    is shared among the other families by size. Grain goes to the council's
    reserve where there is a council, otherwise to the other families by
    size. Returns grain added to the reserve, per village.
    """
    gone = size == 0
    n = len(reserve)
    vacant = np.flatnonzero(gone & ((households.land > 0) | (households.animals > 0)))
    for household in vacant:
        village = households.location[household]
        here = (households.location == village) & ~gone
        landless = np.flatnonzero(here & (households.land <= 0))
        for held in (households.land, households.animals):
            if len(landless):
                held[landless[-1]] += held[household]
            elif here.any():
                held[here] += held[household] * size[here] / size[here].sum()
            else:
                continue  # nobody left in the village: it waits
            held[household] = 0.0

    left = gone & (households.grain > 0)
    if not left.any():
        return np.zeros(n)
    to_council = left & council[households.location]
    to_reserve = np.bincount(households.location[to_council], weights=households.grain[to_council], minlength=n)
    reserve += to_reserve
    households.grain[to_council] = 0.0
    left &= ~to_council
    grain = np.bincount(households.location[left], weights=households.grain[left], minlength=n)
    people = np.bincount(households.location, weights=np.where(gone, 0, size), minlength=n)
    share = np.divide(grain, people, out=np.zeros(n), where=people > 0)
    households.grain = np.where(left & (people[households.location] > 0), 0.0, households.grain)
    households.grain += np.where(gone, 0.0, size * share[households.location])
    return to_reserve


def sizes(population: Population, n_households: int) -> np.ndarray:
    """People in each household."""
    return np.bincount(population.household, weights=population.count, minlength=n_households).astype(np.int64)


def pass_on_savings(
    households: Households, size: np.ndarray, treasury: np.ndarray | None = None, council: np.ndarray | None = None
) -> None:
    """Savings of families with nobody left go to the village council if
    there is one (`council` per village, paid into `treasury`), otherwise to
    the other families of their village."""
    gone = (size == 0) & (households.money > 0)
    if not gone.any():
        return
    n = int(households.location.max()) + 1
    if council is not None and council.any():
        to_council = gone & council[households.location]
        treasury += np.bincount(households.location[to_council], weights=households.money[to_council], minlength=n)
        households.money[to_council] = 0.0
        gone &= ~to_council
        if not gone.any():
            return
    left = np.bincount(households.location[gone], weights=households.money[gone], minlength=n)
    alive = size > 0
    heirs = np.bincount(households.location[alive], minlength=n)
    share = np.divide(left, heirs, out=np.zeros_like(left), where=heirs > 0)
    inherited = np.where(alive, share[households.location], 0.0)
    keep = gone & (heirs[households.location] == 0)  # nobody left in the village at all
    households.money = np.where(gone & ~keep, 0.0, households.money) + inherited


def marry(
    population: Population,
    households: Households,
    config: Config,
    rng: np.random.Generator,
    willingness: np.ndarray | None = None,
) -> int:
    """Young people marry, and widows and widowers remarry; returns the
    number of weddings.

    Each month a single woman of `bride_ages` (or a widow up to
    `widow_ages[0]`) marries with `marriage_chance` (times `willingness` in
    her village: people wait when a wage can barely feed a family), to a
    single man of `groom_ages` (or a widower up to `widow_ages[1]`) from
    another family of her village while any are left. A widow or widower
    stays in their household and the new spouse moves in. Otherwise the
    couple live with the groom's family if it has no heir yet, else with
    the bride's if that has none, else in a new household. Whoever moves takes their share of their family's savings
    and grain (savings / family size), and of its land and animals if land
    is split among children (`LandConfig.partible`); otherwise the heir
    keeps them all. A new household's kin is the groom's family.
    """
    demo = config.demography
    n = len(households)
    age = population.age_years
    alone = population.count == 1
    single = alone & ~population.married
    widowed = alone & population.married & ~rules.spouse_alive(population)
    chance = np.full(len(population), demo.marriage_chance)
    if willingness is not None:
        chance *= willingness[population.location]
    female = population.female
    may_wed_woman = (single & (age <= demo.bride_ages[1])) | (widowed & (age <= demo.widow_ages[0]))
    may_wed_man = (single & (age <= demo.groom_ages[1])) | (widowed & (age <= demo.widow_ages[1]))
    brides = np.flatnonzero(female & may_wed_woman & (age >= demo.bride_ages[0]))
    brides = brides[rng.random(len(brides)) < chance[brides]]
    grooms = np.flatnonzero(~female & may_wed_man & (age >= demo.groom_ages[0]))
    if len(brides) == 0 or len(grooms) == 0:
        return 0
    couples = []
    for location in np.unique(population.location[brides]):
        here_b = rng.permutation(brides[population.location[brides] == location])
        free = list(rng.permutation(grooms[population.location[grooms] == location]))
        for bride in here_b:
            home = population.household[bride]
            match = next((i for i, groom in enumerate(free) if population.household[groom] != home), None)
            if match is not None:
                couples.append((bride, free.pop(match), location))
    if not couples:
        return 0
    bride, groom, where = (np.array(x) for x in zip(*couples))
    family_b, family_g = population.household[bride], population.household[groom]

    # A family takes in a couple (at most one a month) if it holds no more
    # than one couple and the newlywed is a generation (15 years) younger
    # than its married members; so heirs stay, their brothers and sisters leave.
    married = np.bincount(population.household, weights=population.count * population.married, minlength=n)
    elder = np.full(n, -1, dtype=np.int64)
    np.maximum.at(elder, population.household[population.married], age[population.married].astype(np.int64))
    room = married <= 2  # at most one couple (or a widowed parent)

    def can_stay(person: np.ndarray, family: np.ndarray) -> np.ndarray:
        return room[family] & ((elder[family] < 0) | (age[person] <= elder[family] - 15))

    # Widows and widowers stay where they are, with their children.
    at_bride = widowed[bride].copy()
    at_groom = ~at_bride & widowed[groom]
    rest = ~at_bride & ~at_groom
    heir_g = rest & can_stay(groom, family_g) & _first(np.where(rest, family_g, -1))
    room[family_g[heir_g]] = False
    rest &= ~heir_g
    heir_b = rest & can_stay(bride, family_b) & _first(np.where(rest, family_b, -1))
    at_groom |= heir_g
    at_bride |= heir_b
    new = ~at_groom & ~at_bride
    home = np.where(at_groom, family_g, family_b)
    home[new] = n + np.arange(new.sum())

    # Whoever leaves takes their share of their family's savings and grain
    # (and land, where it is split among children).
    size = sizes(population, n).astype(np.float64)
    leaving = np.concatenate([bride[~at_bride], groom[~at_groom]])
    left = population.household[leaving]
    founded = households.add(where[new])
    households.kin[founded] = family_g[new]
    going_to = np.concatenate([home[~at_bride], home[~at_groom]])
    for name in ("money", "grain", "land", "animals") if config.land.partible else ("money", "grain"):
        held = getattr(households, name)
        portion = held[left] / size[left]
        np.subtract.at(held, left, portion)
        np.add.at(held, going_to, portion)

    population.household[bride] = home
    population.household[groom] = home
    population.married[bride] = True
    population.married[groom] = True
    couple = population.couple.max() + 1 + np.arange(len(bride))
    population.couple[bride] = couple
    population.couple[groom] = couple
    return len(couples)


def _first(ids: np.ndarray) -> np.ndarray:
    """True at the first place each id (>= 0) appears."""
    first = np.zeros(len(ids), dtype=bool)
    _, at = np.unique(ids, return_index=True)
    first[at] = True
    return first & (ids >= 0)
