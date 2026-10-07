import numpy as np
import pytest

from econ_sim.population import Population
from econ_sim.rng import RandomStreams


def make(counts, health=None):
    n = len(counts)
    return Population(
        count=counts,
        age_months=np.arange(n) * 12,
        female=np.zeros(n, dtype=bool),
        health=health if health is not None else np.full(n, 100.0),
        skill=np.ones(n),
        location=np.zeros(n),
    )


def test_columns_are_cast_to_their_types():
    pop = make([1, 2])
    assert pop.count.dtype == np.int64
    assert pop.female.dtype == np.bool_
    assert pop.location.dtype == np.int32


def test_mismatched_columns_are_rejected():
    with pytest.raises(ValueError):
        Population(count=[1, 1], age_months=[0], female=[True], health=[1.0], skill=[1.0], location=[0])


def test_size_counts_people_not_rows():
    pop = make([1, 10, 100])
    assert len(pop) == 3
    assert pop.size == 111


def test_split_moves_people_into_new_rows():
    pop = make([10, 5, 3])
    rows = pop.split(np.array([4, 0, 3]))
    # Row 2 is taken whole; row 0 loses 4 people to a new row.
    assert pop.count.tolist() == [6, 5, 3, 4]
    assert sorted(rows.tolist()) == [2, 3]
    assert pop.age_months[3] == pop.age_months[0]
    assert pop.size == 18


def test_split_rows_can_be_changed_independently():
    pop = make([10])
    rows = pop.split(np.array([4]))
    pop.health[rows] -= 30
    assert pop.health.tolist() == [100.0, 70.0]


def test_remove_empty_drops_rows_with_no_people():
    pop = make([0, 2, 0, 1])
    pop.remove_empty()
    assert pop.count.tolist() == [2, 1]
    assert pop.age_months.tolist() == [12, 36]


def test_append_and_empty():
    pop = Population.empty()
    pop.append(make([3]))
    pop.append(make([2]))
    assert pop.count.tolist() == [3, 2]


def test_streams_are_reproducible():
    a, b = RandomStreams(7), RandomStreams(7)
    assert a["births"].random(5).tolist() == b["births"].random(5).tolist()


def test_streams_do_not_affect_each_other():
    a, b = RandomStreams(7), RandomStreams(7)
    b["some_new_rule"].random(100)  # a new rule drawing numbers...
    # ...doesn't change what existing rules draw.
    assert a["births"].random(5).tolist() == b["births"].random(5).tolist()
    assert a["births"].random(5).tolist() != a["deaths"].random(5).tolist()
