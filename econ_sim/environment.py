"""The environment: woods that shrink when overcut, and soil that tires.

A village's woods are a stock of standing wood that regrows logistically:
slowly when nearly bare or nearly untouched, fastest in between. Cutting
and gathering firewood draw it down. A village starts with its woods in
balance with its use; a village that grows beyond what they can bear thins
them, and as they thin, wood takes longer to cut and gather. Fires burn part
of them, a loss that takes years to grow back.

Soil fertility follows how crowded the land is: cropped without enough
fallow it tires, and land left to rest after a famine recovers. It moves
slowly, closing a share of the gap each year.
"""

from __future__ import annotations

import numpy as np

from econ_sim.config import Config


def size_woods(people: np.ndarray, config: Config, richness: np.ndarray | None = None) -> tuple[np.ndarray, np.ndarray]:
    """(capacity, starting stock) of each village's woods, in units of
    firewood, for its starting number of `people`, `richness` times the
    usual (a village in the forest has more): in balance with their yearly
    firewood need (or at half capacity, the most productive, if they need
    more than the woods can bear)."""
    cfg = config.environment
    yearly = people * 12.0 * float(np.mean(config.needs.firewood))
    years = cfg.woods_years * (np.ones_like(people) if richness is None else richness)
    capacity = years * yearly
    # Balance: regrowth x stock x (1 - stock / capacity) = yearly use.
    pressure = 1.0 / (cfg.regrowth * years)
    share = 0.5 * (1.0 + np.sqrt(np.maximum(1.0 - 4.0 * pressure, 0.0)))
    return capacity, share * capacity


def woods_reach(woods: np.ndarray, normal: np.ndarray, config: Config) -> np.ndarray:
    """How far a day's work cutting or gathering wood goes, against the
    start, by how much wood is standing (a little further where the woods
    have grown back thicker)."""
    cfg = config.environment
    if not cfg.enabled:
        return np.ones_like(woods)
    standing = np.divide(woods, normal, out=np.ones_like(woods), where=normal > 0)
    return np.clip(standing, 0.0, 1.5) ** cfg.reach_exponent


def grow_woods(
    woods: np.ndarray, capacity: np.ndarray, cut: np.ndarray, burned: np.ndarray, config: Config
) -> np.ndarray:
    """A month of the woods: fire burns `burned` (a share), they regrow,
    and `cut` (firewood cut and gathered) is taken. Changes `woods`;
    returns the wood lost to fire per village."""
    cfg = config.environment
    if not cfg.enabled:
        return np.zeros_like(woods)
    lost = woods * burned
    woods -= lost
    full = np.divide(woods, capacity, out=np.ones_like(woods), where=capacity > 0)
    woods += cfg.regrowth / 12.0 * woods * (1.0 - full) - cut
    # Some scrub and saplings always remain to grow back from.
    np.maximum(woods, 0.01 * capacity, out=woods)
    return lost


def soil_target(people: np.ndarray, land: np.ndarray, config: Config) -> np.ndarray:
    """The fertility each village's soil heads toward, given how crowded its land is."""
    cfg = config.environment
    crowding = np.divide(people, land * cfg.people_per_plot, out=np.ones_like(people), where=land > 0)
    return np.clip(1.0 - cfg.overcropping * (crowding - 1.0), *cfg.fertility_range)


def tire_soil(soil: np.ndarray, people: np.ndarray, land: np.ndarray, config: Config) -> None:
    """A month's change in soil fertility. Changes `soil`."""
    cfg = config.environment
    if not cfg.enabled:
        return
    monthly = 1.0 - (1.0 - cfg.soil_recovery) ** (1 / 12)
    soil += monthly * (soil_target(people, land, config) - soil)
