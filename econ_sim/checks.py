"""Invariants of the world's state: things that must hold every month, or the
model has a bug. Used by the tests and the sanity check."""

from __future__ import annotations

import numpy as np

from econ_sim.population import NO_JOB
from econ_sim.world import World


def _close(a: np.ndarray, b: np.ndarray, scale: np.ndarray, floor: float = 1.0) -> bool:
    """Equal up to rounding: a millionth of `scale`, or of `floor` when the
    values are small (rounding left from when they were large)."""
    return bool(np.all(np.abs(a - b) <= 1e-6 * np.maximum(scale, floor)))


def broken_invariants(world: World) -> list[str]:
    """Names of the invariants the world breaks right now (empty if none)."""
    broken = []
    hh, pop, n = world.households, world.population, world.n_locations
    n_hh = len(hh)

    def per_village(values: np.ndarray) -> np.ndarray:
        return np.bincount(hh.location, weights=values, minlength=n)

    # Land: families' holdings plus the lord's demesne are the village's land.
    if not _close(per_village(hh.land) + world.lord.land, world.land, world.land):
        broken.append("land not conserved")
    # Debts and claims on them balance in every village.
    debt, lent = per_village(hh.debt), per_village(hh.lent)
    if not _close(debt, lent, np.maximum(debt, lent), floor=1000.0):  # a thousandth of a coin
        broken.append("debts and claims don't balance")
    # Nothing held is negative.
    held = {
        "grain": hh.grain, "money": hh.money, "land": hh.land, "animals": hh.animals, "debt": hh.debt, "lent": hh.lent,
        "business cash": world.cash, "tools": world.tools, "stock": world.stock, "supplies": world.supplies,
        "lord's barn": world.lord.barn, "council reserve": world.council.reserve, "treasury": world.council.treasury,
        "woods": world.woods,
    }
    for name, values in held.items():
        if values.size and (not np.all(np.isfinite(values)) or values.min() < -1e-9):
            broken.append(f"negative or not-a-number {name}")
    if not np.all((world.soil > 0) & np.isfinite(world.soil)) or not np.all(world.sown >= 0):
        broken.append("soil or sowing out of range")
    # People: whole, of a real age, in a household of their own village.
    if len(pop):
        if pop.count.min() < 1 or pop.age_months.min() < 0:
            broken.append("empty rows or negative ages")
        if pop.household.min() < 0 or pop.household.max() >= n_hh:
            broken.append("people in no household")
        elif not np.array_equal(hh.location[pop.household], pop.location):
            broken.append("people living outside their household's village")
        if not np.all((pop.job == NO_JOB) | (pop.job >= 0)):
            broken.append("unknown job")
        # Couples: at most two people, a man and a woman, both married.
        coupled = pop.couple >= 0
        if (~pop.married[coupled]).any():
            broken.append("unmarried people in a couple")
        ids, members = np.unique(pop.couple[coupled], return_counts=True)
        women = np.bincount(np.searchsorted(ids, pop.couple[coupled & pop.female]), minlength=len(ids))
        if len(ids) and (members.max() > 2 or women.max() > 1 or (members - women).max() > 1):
            broken.append("couples of more than a man and a woman")
    # Kin links point at real families of the same village.
    linked = hh.kin >= 0
    if (hh.kin >= n_hh).any() or not np.array_equal(hh.location[hh.kin[linked]], hh.location[linked]):
        broken.append("kin links to no family or another village")
    return broken
