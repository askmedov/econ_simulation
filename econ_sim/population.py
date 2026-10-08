"""The population table.

People are stored as columns (one array per attribute), not one object per
person. Each row stands for `count` identical people. Today every row has
count 1, but every rule is written against counts, so the same code can later
run a country where one row is thousands of similar people.
"""

from __future__ import annotations

from dataclasses import dataclass, fields

import numpy as np

NO_JOB = -1

DTYPES = {
    "count": np.int64,
    "age_months": np.int32,
    "female": np.bool_,
    "health": np.float64,
    "skill": np.float64,
    "location": np.int32,
    "household": np.int64,
    "job": np.int16,
    "married": np.bool_,
    "couple": np.int64,
}
# Columns that may be left out when building a table, and their fill value.
DEFAULTS = {"household": 0, "job": NO_JOB, "married": False, "couple": -1}


@dataclass
class Population:
    count: np.ndarray  # people this row stands for
    age_months: np.ndarray
    female: np.ndarray
    health: np.ndarray  # 0-100, group average
    skill: np.ndarray  # work productivity multiplier, group average
    location: np.ndarray  # village index
    household: np.ndarray | None = None  # family the row's people belong to
    job: np.ndarray | None = None  # business they work in, or NO_JOB
    married: np.ndarray | None = None  # married (or widowed): no longer looking for a spouse
    couple: np.ndarray | None = None  # id shared with their spouse; -1 if never had one here

    def __post_init__(self) -> None:
        for name, fill in DEFAULTS.items():
            if getattr(self, name) is None:
                setattr(self, name, np.full(len(self.count), fill))
        lengths = set()
        for name, dtype in DTYPES.items():
            column = np.asarray(getattr(self, name), dtype=dtype)
            setattr(self, name, column)
            lengths.add(len(column))
        if len(lengths) > 1:
            raise ValueError(f"columns have different lengths: {sorted(lengths)}")

    @classmethod
    def empty(cls) -> Population:
        return cls(**{name: [] for name in DTYPES})

    def __len__(self) -> int:
        return len(self.count)

    @property
    def size(self) -> int:
        """Number of people (not rows)."""
        return int(self.count.sum())

    @property
    def age_years(self) -> np.ndarray:
        return self.age_months // 12

    def _columns(self) -> dict[str, np.ndarray]:
        return {f.name: getattr(self, f.name) for f in fields(self)}

    def select(self, rows: np.ndarray) -> Population:
        """New table with the given rows (index array or boolean mask)."""
        return Population(**{name: col[rows] for name, col in self._columns().items()})

    def append(self, other: Population) -> None:
        for name, col in self._columns().items():
            setattr(self, name, np.concatenate([col, getattr(other, name)]))

    def remove_empty(self) -> None:
        if (self.count == 0).any():
            kept = self.select(self.count > 0)
            for name, col in kept._columns().items():
                setattr(self, name, col)

    def split(self, take: np.ndarray) -> np.ndarray:
        """Separate `take[i]` people out of each row i, e.g. those hit by an event.

        Returns the indices of rows that now hold exactly the taken people, so
        the caller can change their attributes. A row taken in full is
        returned as is; otherwise its taken people move to a new copy of it.
        """
        take = np.asarray(take, dtype=np.int64)
        whole = np.flatnonzero((take > 0) & (take == self.count))
        partial = np.flatnonzero((take > 0) & (take < self.count))
        if len(partial):
            moved = self.select(partial)
            moved.count = take[partial]
            self.count[partial] -= take[partial]
            first_new = len(self)
            self.append(moved)
            new_rows = np.arange(first_new, len(self))
        else:
            new_rows = np.array([], dtype=np.int64)
        return np.concatenate([whole, new_rows])
