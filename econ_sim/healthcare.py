"""Basic healthcare: healers paid by the village council see the people most
likely to die first, as far as their time allows."""

from __future__ import annotations

import numpy as np

from econ_sim.config import Config
from econ_sim.population import Population


def healer_job(config: Config) -> int:
    return len(config.businesses) + 1  # just past the council's officials


def treat(
    population: Population, healers: np.ndarray, risk: np.ndarray, config: Config
) -> tuple[np.ndarray, np.ndarray]:
    """Treat patients up to each village's capacity (`healers` per village).

    Healers triage: they see the people most likely to die this month first
    (`risk` per row), down to `min_risk`: infants, the old, the starving and
    anyone hit by an outbreak. Treated people gain health now. Returns (care
    multiplier on each row's risk of dying, people treated per village).
    Splits groups when only some of a row can be seen.
    """
    cfg = config.healthcare
    n = len(healers)
    treated = np.zeros(n)
    if not cfg.enabled or not (healers > 0).any():
        return np.ones(len(population)), treated
    capacity = healers * cfg.patients_per_healer
    take = np.zeros(len(population), dtype=np.int64)
    # Patients by village, the most at risk first; each village's healers see
    # them in that order until their time runs out.
    patients = np.flatnonzero((capacity[population.location] > 0) & (risk >= cfg.min_risk))
    patients = patients[np.lexsort((-risk[patients], population.location[patients]))]
    counts = population.count[patients]
    where = population.location[patients]
    seen = np.cumsum(counts)
    first = np.r_[True, where[1:] != where[:-1]]
    village_start = np.maximum.accumulate(np.where(first, seen - counts, 0))
    before = seen - counts - village_start
    take[patients] = np.clip(capacity[where] - before, 0, counts).astype(np.int64)
    rows = population.split(take)
    care = np.ones(len(population))
    if len(rows):
        care[rows] = cfg.treatment_mortality
        population.health[rows] = np.minimum(population.health[rows] + cfg.treatment_recovery, config.health.maximum)
        treated = np.bincount(population.location[rows], weights=population.count[rows], minlength=n)
    return care, treated
