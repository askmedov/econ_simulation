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
        """Add `other`'s rows at the end. Columns keep spare room at their
        end, so a month's newcomers don't copy the whole table each time."""
        n, k = len(self), len(other)
        if k == 0:
            return
        for name, col in self._columns().items():
            buffer = self._room(name, col, n + k)
            buffer[n:n + k] = getattr(other, name)
            setattr(self, name, buffer[:n + k])

    def _room(self, name: str, col: np.ndarray, size: int) -> np.ndarray:
        """The buffer behind column `name`, with room for `size` rows (a new,
        larger one holding the column if it has none or it is too small)."""
        buffers = self.__dict__.setdefault("_buffers", {})
        buffer = buffers.get(name)
        starts_it = buffer is not None and col.base is buffer and col.ctypes.data == buffer.ctypes.data
        if not starts_it or len(buffer) < size:
            buffer = np.empty(size + max(size // 4, 1024), dtype=col.dtype)
            buffer[:len(col)] = col
            buffers[name] = buffer
        return buffer

    @classmethod
    def concatenate(cls, parts: list[Population]) -> Population:
        """One table of all the parts' rows, in order (copying each once)."""
        if not parts:
            return cls.empty()
        return cls(**{f.name: np.concatenate([getattr(p, f.name) for p in parts]) for f in fields(cls)})

    def remove_empty(self) -> None:
        """Drop rows with nobody left, keeping the others in order (in place,
        in the columns' own buffers)."""
        kept = self.count > 0
        if kept.all():
            return
        m = int(kept.sum())
        for name, col in self._columns().items():
            buffer = self._room(name, col, len(col))
            buffer[:m] = col[kept]
            setattr(self, name, buffer[:m])

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
