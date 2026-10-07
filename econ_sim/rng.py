"""Named random streams.

Each rule draws from its own stream, derived from the main seed and the
stream's name. Adding a new rule or event therefore doesn't shift the random
draws of existing ones, so results stay comparable as the model grows.
"""

from __future__ import annotations

import zlib

import numpy as np


class RandomStreams:
    def __init__(self, seed: int) -> None:
        self.seed = seed
        self._streams: dict[str, np.random.Generator] = {}

    def __getitem__(self, name: str) -> np.random.Generator:
        if name not in self._streams:
            self._streams[name] = np.random.default_rng([self.seed, zlib.crc32(name.encode())])
        return self._streams[name]
