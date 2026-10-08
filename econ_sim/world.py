"""The state of the simulated world, and how it is set up at the start."""

from __future__ import annotations

from dataclasses import dataclass, field

import numpy as np

from econ_sim import council, economy, livestock, rules
from econ_sim.council import Councils
from econ_sim.config import Config, EventSpec
from econ_sim.households import Households, assign_land, form_households
from econ_sim.population import Population
from econ_sim.rng import RandomStreams


@dataclass
class ActiveEvent:
    spec: EventSpec
    location: int
    months_left: int  # including the current month
    effects: dict[str, float]  # ranges already drawn


@dataclass
class World:
    month: int  # months since the start; 0 is January of year 1
    population: Population
    land: np.ndarray  # farmland per village, in plots
    households: Households
    stock: np.ndarray  # goods held by the business that made them (villages x products)
    prices: np.ndarray  # coins per unit (villages x products): fair price x mark-up
    markup: np.ndarray  # (villages x products), 1 = selling at the fair price
    wage_level: np.ndarray  # customary monthly wage per village: fair prices are reckoned in it
    cash: np.ndarray  # coins held by each business (villages x businesses)
    tools: np.ndarray  # tools in use by each business (villages x businesses)
    supplies: np.ndarray  # goods a business holds to make its own (villages x businesses x products)
    pay: np.ndarray  # running average monthly pay per worker (villages x businesses)
    orders: np.ndarray  # running average units asked for a month (villages x products)
    food: int  # index of the food product
    council: Councils
    names: tuple[str, ...]  # village names
    active_events: list[ActiveEvent] = field(default_factory=list)
    council_costs: np.ndarray | None = None  # last month's council running costs, per village
    coin_earnings: np.ndarray | None = None  # running average of families' monthly coin income, per village
    seed: np.ndarray | None = None  # grain kept back to sow at the next sowing, per village (set in the first month)
    sown: np.ndarray | None = None  # share of the needed seed sown at the last sowing, per village

    def __post_init__(self) -> None:
        n = len(self.names)
        if self.council_costs is None:
            self.council_costs = np.zeros(n)
        if self.sown is None:
            self.sown = np.ones(n)

    @property
    def farm_grain(self) -> np.ndarray:
        """Grain held by the farms (this month's harvest before it is shared
        out, and what they keep to sell for tools), per village. A view:
        changing it changes the farms' stock."""
        return self.stock[:, self.food]

    @property
    def granary(self) -> np.ndarray:
        """All food in store per village: families' own stores plus the farms'."""
        families = np.bincount(self.households.location, weights=self.households.grain, minlength=self.n_locations)
        return families + self.farm_grain

    @property
    def food_price(self) -> np.ndarray:
        return self.prices[:, self.food]

    @food_price.setter
    def food_price(self, value: np.ndarray) -> None:
        self.prices[:, self.food] = value

    @property
    def year(self) -> int:
        return self.month // 12 + 1

    @property
    def month_of_year(self) -> int:
        """Calendar month, 1-12."""
        return self.month % 12 + 1

    @property
    def n_locations(self) -> int:
        return len(self.names)

    @property
    def money(self) -> float:
        """All coins in the world; never changes."""
        return float(self.households.money.sum() + self.cash.sum() + self.council.treasury.sum())


def create_world(config: Config, streams: RandomStreams) -> World:
    rng = streams["setup"]
    population = Population.empty()
    for location, village in enumerate(config.villages):
        population.append(_initial_people(village.population, location, config, rng))

    n, n_business, n_products = len(config.villages), len(config.businesses), len(config.products)
    yearly_marriage = 1.0 - (1.0 - config.demography.marriage_chance) ** 12
    households = form_households(population, n, rng, yearly_marriage)
    economy.assign_starting_jobs(population, config, rng)
    land = np.array([v.land for v in config.villages], dtype=np.float64)
    food = config.product_for("food")

    # Businesses start with a full set of tools, some cash, and some goods.
    workers = economy.headcount(population, n, config)
    boost = np.array([b.tool_boost for b in config.businesses])
    tools = np.where(boost > 0, workers, 0.0)

    # Families hold the land, and landholders the plough animals.
    assign_land(households, population, land, config, streams["land"])
    livestock.place_herds(households, land, config, streams["land"])
    animals = livestock.farm_factor(np.bincount(households.location, weights=households.animals, minlength=n), land, config)

    # Prices start fair, with the wage set so food costs `food_price`.
    per_worker = economy.productivity(population, tools, workers, land, config, animals)
    wage = config.money.food_price * per_worker[:, config.farming]
    prices = np.zeros((n, n_products))
    for _ in range(len(config.products)):  # supplies are valued at the prices being worked out
        prices = economy.fair_prices(wage, per_worker, prices, config)

    # Every family starts with a few months of food and firewood money saved.
    need_rows = rules.food_need(population, config)
    family_need = np.bincount(population.household, weights=need_rows, minlength=len(households))
    members = np.bincount(population.household, weights=population.count, minlength=len(households))
    fuel = config.product_for("heating")
    monthly = (
        family_need * prices[households.location, food]
        + members * np.mean(config.needs.firewood) * prices[households.location, fuel]
    )
    households.money = config.money.initial_savings_months * monthly

    made = economy.capacity(economy.labor(population, n, config), tools, workers, land, np.ones((n, n_business)), config, animals)
    # Food in store as configured; other goods, the stock their trade aims to keep.
    food_need = rules.by_location(need_rows, population, n)
    orders = np.zeros((n, n_products))
    orders[:, economy.product_of(config)] = made
    orders[:, food] = food_need
    stock = config.trade.stock_target_months * orders
    stock[:, food] = 0.0

    # Families hold the food in store: half of each village's stores spread
    # by families' needs, half by the land they hold.
    in_store = food_need * np.array([v.initial_food_months for v in config.villages])
    loc = households.location
    held = np.bincount(loc, weights=households.land, minlength=n)
    by_need = np.divide(family_need, food_need[loc], out=np.zeros(len(households)), where=food_need[loc] > 0)
    by_land = np.divide(households.land, held[loc], out=by_need.copy(), where=held[loc] > 0)
    households.grain = in_store[loc] * (0.5 * by_need + 0.5 * by_land)
    supplies = economy.input_needs(config)[None, :, :] * made[:, :, None]  # a month's worth

    # Big villages start with an established council.
    councils = Councils.none(n)
    cc = config.council
    people = rules.by_location(population.count.astype(np.float64), population, n)
    if cc.enabled and cc.established_at_start:
        councils.formed = people >= cc.forms_at_population
        council.staff(population, councils, council.official_job(config), cc.officials_per_1000, config, rng)
        if config.healthcare.enabled:
            council.staff(population, councils, council.official_job(config) + 1, config.healthcare.healers_per_1000, config, rng)
        officials = rules.by_location(
            np.where(population.job == council.official_job(config), population.count, 0).astype(np.float64), population, n
        )
        councils.treasury = np.where(councils.formed, cc.treasury_months * officials * cc.official_pay * wage, 0.0)
        councils.reserve = np.where(councils.formed, cc.reserve_months * food_need, 0.0)
        councils.months_ready = np.where(councils.formed, cc.forms_after_months, 0)
    return World(
        month=config.start_month - 1,
        population=population,
        land=land,
        households=households,
        stock=stock,
        prices=prices,
        markup=np.ones((n, n_products)),
        wage_level=wage,
        cash=config.money.initial_business_cash_per_worker * workers,
        tools=tools,
        supplies=supplies,
        pay=np.tile(wage[:, None], (1, n_business)),
        orders=orders,
        food=food,
        council=councils,
        names=tuple(v.name for v in config.villages),
    )


def _initial_people(n: int, location: int, config: Config, rng: np.random.Generator) -> Population:
    demo = config.demography
    ages = np.arange(demo.max_initial_age + 1)
    weights = np.exp(-demo.initial_age_decay * ages)
    years = rng.choice(ages, size=n, p=weights / weights.sum())
    # Alternate sexes down the age order, so every age group is balanced.
    by_age = np.argsort(years + rng.random(n))
    female = np.zeros(n, dtype=bool)
    female[by_age[rng.integers(2) :: 2]] = True
    count = np.ones(n, dtype=np.int64)
    return Population(
        count=count,
        age_months=years * 12 + rng.integers(0, 12, size=n),
        female=female,
        health=rng.uniform(*config.health.initial, size=n),
        skill=rules.draw_skill(count, config.skill, rng),
        location=np.full(n, location),
    )
