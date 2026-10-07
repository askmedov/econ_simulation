"""The monthly loop: the same steps, in the same order, every month."""

from __future__ import annotations

import calendar

import numpy as np

from econ_sim import economy, events, households, market, metrics, rules
from econ_sim.config import Config
from econ_sim.metrics import MonthRecord
from econ_sim.rng import RandomStreams
from econ_sim.world import World, create_world


class Simulation:
    def __init__(self, config: Config) -> None:
        events.validate(config.events)
        events.validate_schedule(config.scheduled_events, config.events, len(config.villages))
        if not 1 <= config.start_month <= 12:
            raise ValueError("start_month must be between 1 and 12")
        self.config = config
        self.streams = RandomStreams(config.seed)
        self.world: World = create_world(config, self.streams)
        self.records: list[MonthRecord] = []
        self.log: list[str] = []  # notable happenings, in order
        self._short_since: dict[int, int] = {}  # village -> month rationing began

    @property
    def extinct(self) -> bool:
        return self.world.population.size == 0

    def run(self, months: int | None = None) -> list[MonthRecord]:
        for _ in range(self.config.months if months is None else months):
            self.step()
        return self.records

    def step(self) -> MonthRecord:
        world, config, streams = self.world, self.config, self.streams
        pop, n = world.population, world.n_locations

        alive_at_start = not self.extinct

        # 1. Events: end old ones, start scheduled and random ones, combine their effects.
        events.advance(world)
        month_number = len(self.records) + 1
        scheduled = events.start_scheduled_events(
            world, config.scheduled_events, config.events, month_number, streams["scheduled_events"]
        )
        for event in scheduled:
            self._note(event.location, f"{event.spec.message} (scheduled)")
        hits: dict[str, int] = {}
        if config.random_events:
            for event in events.start_location_events(world, config.events, streams["events"]):
                self._note(event.location, event.spec.message)
            hits = events.apply_person_events(world, config.events, config.health, streams["person_events"])
        names = tuple(b.name for b in config.businesses)
        mods = events.modifiers(world, names)
        lost = world.granary * mods.granary_loss
        world.granary -= lost

        # 2. Businesses make goods, using up supplies and wearing out tools.
        # Other than farms, they work less when unsold goods pile up.
        farm, food = config.farming, world.food
        hh = world.households
        product_of = economy.product_of(config)
        workers = economy.headcount(pop, n, config)
        effort = economy.labor(pop, n, config, rng=streams["production"])
        possible = economy.capacity(effort, world.tools, workers, world.land, mods.production_mult, config)
        possible[:, farm] *= rules.season_factors(config)[world.month_of_year - 1]
        target = config.trade.stock_target_months * world.orders[:, product_of]
        piled_up = np.divide(world.stock[:, product_of], target, out=np.zeros_like(target), where=target > 0)
        pace = np.clip(2.0 - piled_up, 0.0, 1.0)
        pace[:, farm] = 1.0
        made = economy.limit_by_supplies(possible * pace, world.supplies, config)
        world.stock[:, product_of] += made
        world.tools *= 1.0 - config.trade.tool_wear
        produced = made[:, farm]

        # Prices: what each product fairly costs to make, times its mark-up.
        per_worker = economy.productivity(pop, world.tools, workers, world.land, config)
        world.prices = economy.fair_prices(world.wage_level, per_worker, world.prices, config) * world.markup

        # 3. What's on offer: farms hold food back to last until the next
        # harvest; other businesses sell from their stock.
        need_rows = rules.food_need(pop, config)
        need = rules.by_location(need_rows, pop, n)
        full_strength = economy.labor(pop, n, config, at_full_health=True)
        no_events = np.ones_like(mods.production_mult)
        normal = economy.capacity(full_strength, world.tools, workers, world.land, no_events, config)[:, farm]
        ahead = events.production_outlook(world, max(config.food.planning_months - 1, 0), names[farm])
        outlook = rules.harvest_outlook(normal, ahead, world.month_of_year, config)
        offer = world.stock.copy()
        supply_cover = rules.plan_ration_realistically(world.granary, need, outlook, config, most=2.0)
        offer[:, food] = np.minimum(need * np.minimum(supply_cover, 1.0), world.granary)
        stock_before = world.stock.copy()

        # 4. What families need this month, and help between neighbours.
        n_hh = len(hh)
        members = market.by_household(pop.count.astype(np.float64), pop, n_hh)
        family_food = market.by_household(need_rows, pop, n_hh)
        fuel = config.product_for("heating")
        firewood_need = config.needs.firewood[world.month_of_year - 1]
        family_fuel = members * firewood_need * mods.heating_mult[hh.location]
        essentials = family_food * world.prices[hh.location, food] + family_fuel * world.prices[hh.location, fuel]
        shared = market.share_with_neighbours(hh.money, essentials, hh.location, n, config)

        # 5. Markets, most needed first. Businesses buy supplies and tools
        # with part of their cash, before paying wages.
        seller = economy.seller_of(config)
        demand = np.zeros_like(world.stock)
        sold = np.zeros_like(world.stock)
        bought = {}

        def trade(product: int, family_want: np.ndarray | None, family_money: np.ndarray | None,
                  business_want: np.ndarray | None) -> None:
            money_parts, want_parts, where = [], [], []
            if family_want is not None:
                money_parts.append(family_money)
                want_parts.append(family_want)
                where.append(hh.location)
            budget = config.trade.buying_budget * world.cash
            if business_want is not None:
                money_parts.append(budget.reshape(-1).copy())
                want_parts.append(business_want.reshape(-1))
                where.append(np.repeat(np.arange(n), len(names)))
            money = np.concatenate(money_parts)
            sale = market.buy(money, np.concatenate(want_parts), world.prices[:, product], offer[:, product], np.concatenate(where))
            world.stock[:, product] = np.maximum(world.stock[:, product] - sale.sold, 0.0)
            world.cash[:, seller[product]] += market.by_village(sale.spent, np.concatenate(where), n)
            demand[:, product] = sale.demand
            sold[:, product] = sale.sold
            if family_want is not None:
                if family_money is not hh.money:
                    hh.money -= sale.spent[:n_hh]
                else:
                    hh.money[:] = money[:n_hh]
                bought[product] = sale.bought[:n_hh]
            if business_want is not None:
                spent = sale.spent[-budget.size:].reshape(budget.shape)
                world.cash -= spent
                got = sale.bought[-budget.size:].reshape(budget.shape)
                if config.products[product].use == "tool":
                    world.tools += got
                else:
                    world.supplies[:, :, product] += got

        def business_want(product: int) -> np.ndarray | None:
            per_unit = economy.input_needs(config)[:, product]
            if config.products[product].use == "tool":
                boost = np.array([b.tool_boost for b in config.businesses])
                return np.where(boost > 0, np.maximum(workers - world.tools, 0.0), 0.0)
            if per_unit.any():
                return np.maximum(possible * per_unit - world.supplies[:, :, product], 0.0)
            return None

        for product, spec in enumerate(config.products):
            if spec.use == "food":
                trade(product, family_food, hh.money, None)
            elif spec.use == "heating":
                trade(product, family_fuel, hh.money, business_want(product))
            elif spec.use == "comfort":
                spare = np.maximum(0.0, hh.money - config.needs.savings_months * essentials)
                spend = config.needs.spare_spending * spare
                price = world.prices[hh.location, product]
                want = np.divide(spend, price, out=np.zeros_like(spend), where=(spend > 0) & (price > 0))
                trade(product, want, spend, business_want(product))
            else:
                trade(product, None, None, business_want(product))

        family_share = np.divide(bought[food], family_food, out=np.ones_like(family_food), where=family_food > 0)
        warmth = np.divide(bought[fuel], family_fuel, out=np.ones_like(family_fuel), where=family_fuel > 0)
        eaten = market.by_village(bought[food], hh.location, n)
        share = np.divide(eaten, need, out=np.ones_like(need), where=need > 0)
        self._track_shortages(share)

        # 6. Businesses pay out their takings as wages. Keep a running average
        # of what each trade pays (or could pay, if nobody works in it).
        supplies_cost = (economy.input_needs(config)[None, :, :] * world.prices[:, None, :]).sum(axis=2) * possible
        tool = config.product_for("tool")
        boost = np.array([b.tool_boost for b in config.businesses])
        tools_cost = np.where(boost > 0, config.trade.tool_wear * workers * world.prices[:, tool, None], 0.0)
        income, payout = market.pay_wages(world.cash, pop, n_hh, config, keep=supplies_cost + tools_cost)
        hh.money += income
        paid = np.divide(payout, workers, out=np.zeros_like(payout), where=workers > 0)
        empty = workers < 1
        hoped = economy.potential_pay(world.prices, per_worker, config)
        weight = config.trade.pay_memory
        world.pay = np.where(empty, hoped, (1 - weight) * world.pay + weight * paid)

        # 7. Some stored food spoils.
        spoiled = rules.spoil(world.granary, config.food.spoilage)

        # 8. Health follows each family's food and warmth, plus events.
        coldness = firewood_need / max(config.needs.firewood)
        cold = config.needs.cold_damage * coldness * (1.0 - warmth)
        rules.update_health(pop, family_share[pop.household], mods.health_delta[pop.location] - cold[pop.household], config)

        # 9. Deaths, births, ageing. Businesses with more orders than workers
        # take people on; those with too many let some go to trades that are
        # short. Farms aim to grow a little more than the village eats.
        died = rules.deaths(pop, mods.mortality_mult, config, streams["deaths"], n)
        all_workers = workers.sum(axis=1)
        average_pay = np.divide((world.pay * workers).sum(axis=1), all_workers, out=np.zeros(n), where=all_workers > 0)
        cover = rules.wage_cover(average_pay, world.food_price, need, all_workers)
        crowding = rules.birth_factor(cover, config)
        born = rules.births(pop, mods.fertility_mult * crowding, config, streams["births"], n)
        rules.grow_older(pop)
        weight = config.trade.orders_memory
        world.orders = (1 - weight) * world.orders + weight * demand
        orders = world.orders[:, product_of]
        orders[:, farm] = need * (1.0 + config.food.reserve_margin)
        pull = world.markup[:, product_of] ** config.trade.hiring_price_response
        work_needed = orders / np.maximum(per_worker, 1e-9) * pull
        wanted = economy.wanted_workers(work_needed, economy.headcount(pop, n, config), config)
        economy.assign_new_workers(pop, wanted, config)
        switched = economy.move_workers(pop, wanted, config, streams["jobs"])
        size = households.sizes(pop, n_hh)
        households.pass_on_savings(hh, size)

        # 10. Mark-ups move with the gap between demand and supply: for most
        # goods, what was asked for against what was made this month plus any
        # stock piled up beyond the target.
        target_by_product = config.trade.stock_target_months * world.orders
        excess = np.maximum(stock_before - target_by_product, 0.0) / config.trade.stock_target_months
        made_by_product = np.zeros_like(world.stock)
        made_by_product[:, product_of] = made
        supply = made_by_product + excess
        food_markup = world.markup[:, food].copy()
        world.markup = market.adjust_markup(world.markup, demand, supply, config)
        # Grain follows how short the year's supply looks (King-Davenant).
        world.markup[:, food] = market.grain_markup(food_markup, supply_cover, config)

        # 11. Record the month.
        children, working, elderly = metrics.age_groups(pop, config)
        jobs = economy.headcount(pop, n, config).sum(axis=0)
        record = MonthRecord(
            month_number=month_number,
            year=world.year,
            month=world.month_of_year,
            population=pop.size,
            children=children,
            workers=working,
            elderly=elderly,
            households=int((size > 0).sum()),
            births=int(born.sum()),
            deaths=int(died.sum()),
            food_produced=float(produced.sum()),
            food_needed=float(need.sum()),
            food_eaten=float(eaten.sum()),
            ration=float(eaten.sum() / need.sum()) if need.sum() > 0 else 1.0,
            food_spoiled=float(spoiled.sum()),
            food_lost=float(lost.sum()),
            food_stock=float(world.granary.sum()),
            food_margin=float(normal.sum() / need.sum()) if need.sum() > 0 else 0.0,
            food_cover=float(np.average(supply_cover, weights=np.maximum(need, 1e-9))),
            wage_cover=float(np.average(cover, weights=np.maximum(need, 1e-9))),
            food_price=float(np.average(world.food_price, weights=np.maximum(need, 1e-9))),
            wage=float(payout.sum() / workers.sum()) if workers.sum() > 0 else 0.0,
            savings=float(hh.money.sum()),
            business_cash=float(world.cash.sum()),
            shared=float(shared.sum()),
            underfed=int(pop.count[family_share[pop.household] < 0.9].sum()),
            poorest_fifth_ration=metrics.poorest_fifth_ration(hh.money, size, bought[food], family_food),
            warmth=float(bought[fuel].sum() / family_fuel.sum()) if family_fuel.sum() > 0 else 1.0,
            clothing=float(bought[config.product_for("comfort")].sum() / max(pop.size, 1)),
            job_changes=switched,
            prices={p.name: float(world.prices[:, i].mean()) for i, p in enumerate(config.products)},
            jobs={b.name: int(round(jobs[i])) for i, b in enumerate(config.businesses)},
            avg_health=metrics.average_health(pop),
            poor_health=metrics.poor_health(pop, config),
            accidents=sum(hits.values()),
            events=self._active_event_names(),
        )
        self.records.append(record)
        if alive_at_start and self.extinct:
            self._note(None, "The last villager has died")
        world.month += 1
        return record

    def _note(self, location: int | None, message: str) -> None:
        world = self.world
        where = f"{world.names[location]}: " if location is not None and world.n_locations > 1 else ""
        self.log.append(f"Year {world.year}, {calendar.month_abbr[world.month_of_year]}: {where}{message}")

    def _track_shortages(self, share) -> None:
        for location, fed in enumerate(share):
            started = self._short_since.get(location)
            if fed < 0.97 and started is None:
                self._short_since[location] = self.world.month
                self._note(location, f"Food short: the village ate {fed:.0%} of what it needs")
            elif fed >= 0.99 and started is not None:
                months = self.world.month - started
                self._note(location, f"Enough food again after {months} month{'s' * (months != 1)}")
                del self._short_since[location]

    def _active_event_names(self) -> str:
        world = self.world
        names = []
        for event in world.active_events:
            prefix = f"{world.names[event.location]}:" if world.n_locations > 1 else ""
            names.append(prefix + event.spec.name)
        return "+".join(names)
