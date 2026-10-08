import numpy as np

from econ_sim import credit
from econ_sim.config import Config, CouncilConfig, ScheduledEvent
from econ_sim.households import Households
from econ_sim.simulation import Simulation

CONFIG = Config()
CFG = CONFIG.credit


def balanced(hh):
    return np.isclose(hh.debt.sum(), hh.lent.sum())


def test_families_short_of_food_money_borrow_from_those_with_coins_to_spare():
    hh = Households(np.zeros(3), money=np.array([0.0, 0.0, 100.0]))
    worth = np.array([40.0, 0.0, 0.0])  # the first holds land and animals
    lent = credit.borrow(hh, np.array([30.0, 30.0, 0.0]), np.array([0.0, 0.0, 100.0]), worth, np.full(3, 5.0), CONFIG)
    # Up to half of its land's worth plus a little on its word; only on its word.
    assert np.isclose(hh.debt[0], 25.0) and np.isclose(hh.debt[1], 5.0)
    assert np.isclose(lent[0], 30.0) and np.isclose(hh.money.sum(), 100.0) and balanced(hh)


def test_lenders_lend_only_part_of_their_spare_coins():
    hh = Households(np.zeros(2), money=np.array([0.0, 20.0]))
    credit.borrow(hh, np.array([100.0, 0.0]), np.array([0.0, 20.0]), np.array([1000.0, 0.0]), np.zeros(2), CONFIG)
    assert np.isclose(hh.debt[0], CFG.lend_share * 20.0)


def test_interest_grows_debts_and_claims_alike():
    hh = Households(np.zeros(2), debt=np.array([100.0, 0.0]), lent=np.array([0.0, 100.0]))
    for _ in range(12):
        credit.accrue_interest(hh, CONFIG)
    assert np.isclose(hh.debt[0], 100 * (1 + CFG.interest)) and balanced(hh)


def test_repayments_go_to_the_lenders():
    hh = Households(np.zeros(3), money=np.array([50.0, 0.0, 0.0]), debt=np.array([20.0, 0.0, 0.0]),
                    lent=np.array([0.0, 15.0, 5.0]))
    repaid = credit.repay(hh, np.array([10.0, 0.0, 0.0]), CONFIG)
    paid = CFG.repay_share * 10.0
    assert np.isclose(repaid[0], paid) and np.isclose(hh.debt[0], 20.0 - paid)
    assert np.isclose(hh.money[1], paid * 0.75) and np.isclose(hh.money[2], paid * 0.25) and balanced(hh)


def test_debts_beyond_what_a_family_owns_cost_it_its_animals_then_land():
    hh = Households(np.zeros(3), land=np.array([10.0, 0.0, 0.0]), animals=np.array([2.0, 0.0, 0.0]),
                    debt=np.array([120.0, 0.0, 0.0]), lent=np.array([0.0, 110.0, 10.0]))
    plot, animal = np.array([10.0]), np.array([20.0])
    done = credit.foreclose(hh, plot, animal, CONFIG)
    assert hh.animals[0] == 0.0 and 0 < hh.land[0] < 10.0
    worth_left = hh.land[0] * 10.0
    assert np.isclose(hh.debt[0], CFG.loan_to_value * worth_left)
    assert hh.land[1] > 0 and hh.land[2] == 0  # the biggest creditor takes it
    assert np.isclose(hh.land.sum(), 10.0) and np.isclose(hh.animals.sum(), 2.0) and balanced(hh)
    assert np.isclose(done.value[0], 120.0 - hh.debt[0])


def test_unpayable_debt_is_written_off():
    hh = Households(np.zeros(2), debt=np.array([100.0, 0.0]), lent=np.array([0.0, 100.0]))
    lost = credit.default(hh, np.array([10.0, 0.0]), CONFIG)
    assert np.isclose(hh.debt[0], CFG.default_at * 10.0) and np.isclose(lost[0], 100.0 - CFG.default_at * 10.0)
    assert balanced(hh)


def test_hungry_families_sell_land_cheaply_when_many_sell_at_once():
    def sell(sellers):
        hh = Households(np.zeros(sellers + 2), money=np.array([0.0] * sellers + [60.0, 40.0]),
                        land=np.array([5.0] * sellers + [0.0, 0.0]))
        short = np.array([20.0] * sellers + [0.0, 0.0])
        spare = np.array([0.0] * sellers + [60.0, 40.0])
        sales = credit.sell_land(hh, short, spare, np.array([10.0]), CONFIG)
        assert np.isclose(hh.money.sum(), 100.0) and np.isclose(hh.land.sum(), 5.0 * sellers)
        return sales, hh

    one, hh = sell(1)
    assert np.isclose(one.price[0], 10.0) and np.isclose(hh.land[-2], 2.0) and hh.land[-1] == 0  # richest buys
    many, _ = sell(20)
    assert CFG.lowest_price * 10 - 1e-9 <= many.price[0] < 10.0


def test_a_jubilee_cancels_debts():
    hh = Households(np.zeros(2), debt=np.array([30.0, 0.0]), lent=np.array([0.0, 30.0]))
    assert np.isclose(credit.cancel_debts(hh, np.array([1.0]))[0], 30.0)
    assert hh.debt.sum() == 0 and hh.lent.sum() == 0


def test_bad_years_bring_debt_and_land_changes_hands():
    records = Simulation(Config(seed=1, months=120, council=CouncilConfig(enabled=False),
                                scheduled_events=(ScheduledEvent("drought", 4), ScheduledEvent("drought", 28)))).run()
    assert max(r.debt for r in records) > 0 and sum(r.borrowed for r in records) > 0
    assert sum(r.land_sold + r.land_foreclosed for r in records) > 0


def test_a_scheduled_jubilee_wipes_the_slate():
    config = Config(seed=1, months=60, council=CouncilConfig(enabled=False),
                    scheduled_events=(ScheduledEvent("drought", 4), ScheduledEvent("debt_jubilee", 48)))
    records = Simulation(config).run()
    assert records[47].debt == 0.0 or records[47].debts_cancelled > 0
