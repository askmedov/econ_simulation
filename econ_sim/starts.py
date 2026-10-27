"""Starting points: the state a region starts from, as changes to its settings.

Every run starts from the settings in `Config`. A starting point changes
some of them: how many people the land holds, what is in the barns, how
much coin there is, who holds the land and what they owe, how worn the
land is, what the lords and the state take, how open the region is. Each
is a list of changes: `set` a parameter to a value, or `scale` it by a
factor. A parameter is named by its path in the config ("lord.demesne");
a path through a list changes every item in it ("villages.population":
every place), or the one with that name ("events.plague.chance"). Scaling
keeps whole numbers whole, and a town's zero store a zero.

The Rust engine reads them from rust/configs/starts.json: `econ-sim --start
NAME` runs from one (with `--set` and `--scale` for changes of your own),
and `econ-sim starts` runs several from the same seeds and compares them.
Python's model does not read them yet.
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class Start:
    name: str
    about: str
    set: tuple[tuple[str, object], ...] = ()
    scale: tuple[tuple[str, float], ...] = ()


STARTS: tuple[Start, ...] = (
    Start("standard", "The standard settings, as every run starts"),
    Start(
        "after-plague",
        "A plague has just carried off a third of the people: land to spare, labour scarce",
        scale=(("villages.population", 2 / 3),),
    ),
    Start(
        "crowded",
        "Half as many people again as the land feeds well; the soil tired, the woods thinned",
        set=(("environment.initial_soil", 0.85), ("environment.initial_woods", 0.7)),
        scale=(("villages.population", 1.5),),
    ),
    Start(
        "lean-barns",
        "After a bad harvest: a month of food in store, and half the usual savings",
        scale=(("villages.initial_food_months", 0.2), ("money.initial_savings_months", 0.5)),
    ),
    Start(
        "full-barns",
        "After a run of good harvests: a year of food in store",
        scale=(("villages.initial_food_months", 2.4),),
    ),
    Start(
        "short-of-coin",
        "Coin is scarce: a third of the usual savings and business cash",
        scale=(("money.initial_savings_months", 1 / 3), ("money.initial_business_cash_per_worker", 1 / 3)),
    ),
    Start(
        "heavy-lords",
        "The lords and the state take twice as much: twice the demesne and labour days, twice the tax",
        scale=(
            ("lord.demesne", 2.0),
            ("lord.labour_service", 2.0),
            ("state.hearth_tax_months", 2.0),
            ("state.land_tax", 2.0),
        ),
    ),
    Start(
        "free-peasants",
        "No lords, half the tax, and more families holding land",
        set=(("lord.enabled", False), ("land.landless_share", 0.15)),
        scale=(("state.hearth_tax_months", 0.5), ("state.land_tax", 0.5)),
    ),
    Start(
        "unequal",
        "Land in few hands: half the families landless, the rest's holdings more uneven",
        set=(("land.landless_share", 0.5), ("land.holding_spread", 1.2)),
    ),
    Start(
        "in-debt",
        "Landholding families owe a year and a half's wage to the biggest landholders, and have half the usual savings",
        set=(("credit.initial_debt_months", 18.0),),
        scale=(("money.initial_savings_months", 0.5),),
    ),
    Start(
        "war-torn",
        "After a war: a seventh of the people gone, the rest weakened, barns and herds plundered",
        set=(("health.initial", (55.0, 90.0)), ("livestock.initial_herds", 0.5)),
        scale=(("villages.population", 0.85), ("villages.initial_food_months", 0.4)),
    ),
    Start(
        "worn-land",
        "Soil worn out and woods cut back by generations of crowding",
        set=(("environment.initial_soil", 0.75), ("environment.initial_woods", 0.5)),
    ),
    Start(
        "open-trade",
        "Good roads and rivers: carting costs halved, twice the grain trade beyond the region",
        scale=(
            ("map.handling", 0.5),
            ("map.road_per_km", 0.5),
            ("map.river_per_km", 0.5),
            ("town.outside_capacity", 2.0),
        ),
    ),
    Start(
        "closed",
        "No grain trade with the world beyond the region",
        set=(("town.outside_capacity", 0.0),),
    ),
    Start(
        "no-garrison",
        "The state spends none of its tax in the city",
        set=(("state.garrison", 0.0),),
    ),
)
