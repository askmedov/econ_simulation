"""Monthly statistics: totals over the population table, never per-person logs."""

from __future__ import annotations

import csv
from dataclasses import dataclass, fields
from pathlib import Path

import numpy as np

from econ_sim import rules
from econ_sim.config import Config
from econ_sim.population import Population


@dataclass
class MonthRecord:
    month_number: int  # months from the start, 1 = first simulated month
    year: int
    month: int  # calendar month, 1-12
    population: int
    children: int
    workers: int
    elderly: int
    households: int  # with at least one member
    weddings: int  # couples who left home to start a household
    births: int
    deaths: int
    food_produced: float
    food_needed: float
    food_eaten: float  # from families' own stores, bought, relief from the council's reserve, and foraged
    own_food: float  # rations families ate from their own grain stores
    foraged: float  # rations of famine foods (roots, greens, fish) hungry families found
    grain_sold: float  # rations sold on the village market
    ration: float  # share of food need met, 1.0 = everyone fully fed
    food_spoiled: float
    food_lost: float  # destroyed by events
    food_levied: float  # taken by the council into its reserve (levy, and stores of families who died out)
    food_to_seed: float  # picked from the harvest as seed, or given by families at sowing
    meat: float  # rations of meat from animals slaughtered (the winter cull, or in hunger), into stores
    seed_store: float  # grain kept for the next sowing
    sown: float  # share of the needed seed sown at the last sowing (averaged over villages)
    animals: float  # livestock units held by families
    animals_lost: float  # died this month, net of births (negative: the herds grew)
    animals_sold: float  # sold by families who couldn't afford their food
    animal_price: float  # coins per animal at this month's sales
    debt: float  # coins families owe the village's lenders
    debtors: int  # families in debt
    interest: float  # coins of interest added to debts this month
    borrowed: float  # coins lent to families who couldn't afford their food
    repaid: float  # coins paid back
    repaid_in_grain: float  # rations paid back in kind
    debts_cancelled: float  # coins of debt cancelled by decree
    debts_written_off: float  # coins of debt that could never be repaid, lost to the lenders
    land_sold: float  # plots sold by families who couldn't afford their food
    land_foreclosed: float  # plots taken by lenders for debts
    animals_foreclosed: float  # animals taken by lenders for debts
    land_price: float  # coins a plot is worth (years of its rent)
    food_rent: float  # rations of the harvest that went to the lord (his demesne and labour days)
    lord_carted: float  # rations carted from the lord's barn to his hall
    lord_sold: float  # rations the lord's steward sold in the village
    lord_relief: float  # rations a charitable lord gave the hungry
    lord_barn: float  # rations in the lord's barn
    lord_purse: float  # coins the lord has taken out of the village
    state_tax: float  # coins the state collected this month
    tax_grain: float  # rations seized from families who couldn't pay the state's tax
    state_purse: float  # coins the state has taken out of the village
    food_requisitioned: float  # rations soldiers or raiders took from families and farms
    town_price: float  # coins per ration in the town's grain market
    grain_exported: float  # rations merchants carried to the town
    grain_imported: float  # rations merchants brought from the town and sold
    town_purse: float  # coins the villages have paid the town, net (negative: received)
    coins_lost: float  # coins lost, worn away or buried since the start
    emigrants: int  # people who left for the town
    immigrants: int  # people who came from the region
    food_emigrated: float  # rations leavers took with them
    food_stock: float  # at the end of the month
    food_margin: float  # normal harvest over need; below ~1.05 the land is crowded
    food_cover: float  # stores plus expected harvests, as a share of the coming year's need
    wage_cover: float  # food a month's pay buys, over food need per worker
    food_price: float  # coins per ration (averaged over villages)
    wage: float  # average monthly wage of a worker, in coins
    savings: float  # coins held by families
    money_months: float  # families' coins over a month of their coin earnings
    business_cash: float  # coins held by businesses
    treasury: float  # coins held by village councils
    councils: int  # villages with a council
    officials: int  # council officials
    taxes: float  # coins collected in tax this month
    food_reserve: float  # rations held by councils for famine relief
    relief: float  # rations the council gave free to families who couldn't buy enough
    cash_relief: float  # coins the council gave families who couldn't afford food and firewood
    healers: int  # healers employed by the council
    treated: int  # people healers saw this month
    shared: float  # coins given by better-off families to families short of food money
    underfed: int  # people whose family got less than 90% of its food need
    landless: int  # people in families holding no land
    landless_ration: float  # share of food need met for people in landless families
    poorest_fifth_ration: float  # share of food need met for the poorest fifth of people
    warmth: float  # share of the firewood families needed that they got
    clothing: float  # garments worn per person this month: bought, and homespun
    homespun: float  # garments women spun and wove at home
    harvest_help: float  # workers' worth of help in the fields from outside farming
    kin_help: float  # rations kin gave families who couldn't afford their food
    job_changes: int  # workers who moved to a better-paid trade
    prices: dict[str, float]  # coins per unit of each product
    jobs: dict[str, int]  # workers in each business
    avg_health: float
    poor_health: int  # people below the danger threshold
    accidents: int
    events: str  # active village events

    def value(self, name: str) -> float:
        """A measure by name, including flattened ones like "price_firewood" or "jobs_farming"."""
        return flat(self)[name]


def flat(record: MonthRecord) -> dict:
    """The record as one flat dict: prices and jobs become price_<product> and jobs_<business>."""
    row = {}
    for f in fields(record):
        value = getattr(record, f.name)
        if f.name == "prices":
            row.update({f"price_{k}": v for k, v in value.items()})
        elif f.name == "jobs":
            row.update({f"jobs_{k}": v for k, v in value.items()})
        else:
            row[f.name] = value
    return row


def age_groups(population: Population, config: Config) -> tuple[int, int, int]:
    """(children, working age, elderly) headcounts."""
    age = population.age_years
    children = int(population.count[age < config.demography.adult_age].sum())
    workers = int(population.count[rules.is_working_age(population, config)].sum())
    return children, workers, population.size - children - workers


def average_health(population: Population) -> float:
    if population.size == 0:
        return 0.0
    return float(np.average(population.health, weights=population.count))


def poor_health(population: Population, config: Config) -> int:
    return int(population.count[population.health < config.health.danger_threshold].sum())


def poorest_fifth_ration(money: np.ndarray, size: np.ndarray, bought: np.ndarray, need: np.ndarray) -> float:
    """Share of food need met for the fifth of people in the poorest families
    (by `money` per person: savings, or savings plus grain in store)."""
    lived = size > 0
    if not lived.any():
        return 1.0
    money, size, bought, need = money[lived], size[lived], bought[lived], need[lived]
    order = np.argsort(money / size, kind="stable")
    people_before = np.cumsum(size[order]) - size[order]
    poorest = order[people_before < 0.2 * size.sum()]
    wanted = need[poorest].sum()
    return float(bought[poorest].sum() / wanted) if wanted > 0 else 1.0


def write_csv(records: list[MonthRecord], path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", newline="") as f:
        rows = [flat(record) for record in records]
        writer = csv.DictWriter(f, fieldnames=list(rows[0]) if rows else [f.name for f in fields(MonthRecord)])
        writer.writeheader()
        for row in rows:
            writer.writerow({k: round(v, 3) if isinstance(v, float) else v for k, v in row.items()})
