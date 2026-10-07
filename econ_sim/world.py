"""The state of the simulated world, and how it is set up at the start."""

from __future__ import annotations

from dataclasses import dataclass, field

import numpy as np

from econ_sim import economy, rules
from econ_sim.config import Config, EventSpec
from econ_sim.households import Households, form_households
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
    names: tuple[str, ...]  # village names
    active_events: list[ActiveEvent] = field(default_factory=list)

    @property
    def granary(self) -> np.ndarray:
        """Food in store, per village (held by the farms)."""
        return self.stock[:, self.food]

    @granary.setter
    def granary(self, value: np.ndarray) -> None:
        self.stock[:, self.food] = value

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
        return float(self.households.money.sum() + self.cash.sum())


def create_world(config: Config, streams: RandomStreams) -> World:
    rng = streams["setup"]
    population = Population.empty()
    for location, village in enumerate(config.villages):
        population.append(_initial_people(village.population, location, config, rng))

    n, n_business, n_products = len(config.villages), len(config.businesses), len(config.products)
    households = form_households(population, n, rng)
    economy.assign_starting_jobs(population, config, rng)
    land = np.array([v.land for v in config.villages], dtype=np.float64)
    food = config.product_for("food")

    # Businesses start with a full set of tools, some cash, and some goods.
    workers = economy.headcount(population, n, config)
    boost = np.array([b.tool_boost for b in config.businesses])
    tools = np.where(boost > 0, workers, 0.0)

    # Prices start fair, with the wage set so food costs `food_price`.
    per_worker = economy.productivity(population, tools, workers, land, config)
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

    made = economy.capacity(economy.labor(population, n, config), tools, workers, land, np.ones((n, n_business)), config)
    # Food in store as configured; other goods, the stock their trade aims to keep.
    food_need = rules.by_location(need_rows, population, n)
    orders = np.zeros((n, n_products))
    orders[:, economy.product_of(config)] = made
    orders[:, food] = food_need
    stock = config.trade.stock_target_months * orders
    stock[:, food] = food_need * np.array([v.initial_food_months for v in config.villages])
    supplies = economy.input_needs(config)[None, :, :] * made[:, :, None]  # a month's worth
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
