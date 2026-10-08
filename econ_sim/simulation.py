"""The monthly loop: the same steps, in the same order, every month."""

from __future__ import annotations

import calendar

import numpy as np

from econ_sim import council, economy, events, farms, healthcare, households, market, metrics, rules
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
        farm, food = config.farming, world.food
        hh = world.households
        # Fire or pests strike stored grain: families' stores and the farms'.
        lost_by_family = hh.grain * mods.granary_loss[hh.location]
        hh.grain -= lost_by_family
        lost = market.by_village(lost_by_family, hh.location, n) + world.farm_grain * mods.granary_loss
        world.farm_grain[:] *= 1.0 - mods.granary_loss

        # 2. Businesses make goods, using up supplies and wearing out tools.
        # Other than farms, they work less when unsold goods pile up.
        product_of = economy.product_of(config)
        workers = economy.headcount(pop, n, config)
        effort = economy.labor(pop, n, config, rng=streams["production"])
        possible = economy.capacity(effort, world.tools, workers, world.land, mods.production_mult, config)
        possible[:, farm] *= rules.season_factors(config)[world.month_of_year - 1]
        target = config.trade.stock_target_months * world.orders[:, product_of]
        # Woodcutters also stock up ahead of winter.
        fuel, wood = config.product_for("heating"), economy.seller_of(config)[config.product_for("heating")]
        heating = np.asarray(config.needs.firewood)
        people_here = rules.by_location(pop.count.astype(np.float64), pop, n)
        usual_heating = people_here * heating.mean() * mods.heating_mult
        buffer = market.seasonal_buffer(config.needs.firewood)
        target[:, wood] += buffer[world.month_of_year - 1] * usual_heating
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

        # 3. How well the village's stores and coming harvests cover the year
        # (this sets the grain price and says whether it is a famine year).
        need_rows = rules.food_need(pop, config)
        need = rules.by_location(need_rows, pop, n)
        full_strength = economy.labor(pop, n, config, at_full_health=True)
        no_events = np.ones_like(mods.production_mult)
        normal = economy.capacity(full_strength, world.tools, workers, world.land, no_events, config)[:, farm]
        ahead = events.production_outlook(world, max(config.food.planning_months - 1, 0), names[farm])
        outlook = rules.harvest_outlook(normal, ahead, world.month_of_year, config)
        supply_cover = rules.plan_ration_realistically(world.granary, need, outlook, config, most=2.0)
        famine = supply_cover < 1.0

        # The harvest is shared out in kind. The council's levy comes off the
        # top; the farms keep enough to sell for new tools; the rest goes to
        # the families who worked the fields and the families who hold them.
        n_hh = len(hh)
        levied = council.levy_grain(world.council, produced, world.farm_grain, need, famine, config)
        tool = config.product_for("tool")
        tools_value = config.trade.tool_wear * workers[:, farm] * world.prices[:, tool]
        keep_for_tools = np.divide(tools_value, world.food_price, out=np.zeros(n), where=world.food_price > 0)
        shares = farms.income_shares(pop, hh, world.land, config)
        harvest_share = farms.share_harvest(hh, world.farm_grain, keep_for_tools, shares)
        labour_grain = config.food.labor_share * market.by_village(harvest_share, hh.location, n)

        # 4. What families need this month. They eat from their own store as
        # far as it lasts until their next harvest, offer what they can
        # spare, and need coins for the rest of their food and for firewood.
        members = market.by_household(pop.count.astype(np.float64), pop, n_hh)
        family_food = market.by_household(need_rows, pop, n_hh)
        levy_rate = np.where(world.council.formed, config.council.grain_levy, 0.0)
        plan = farms.plan_family_food(hh, family_food, outlook, shares, 1.0 - levy_rate, config, world.markup[:, food])
        firewood_need = heating[world.month_of_year - 1]
        family_fuel = members * firewood_need * mods.heating_mult[hh.location]
        essentials = plan.want * world.prices[hh.location, food] + family_fuel * world.prices[hh.location, fuel]
        # Families keep coins for a few months of all their food and firewood
        # before spending on comforts, even if they live off their own grain now.
        usual_costs = family_food * world.prices[hh.location, food] + family_fuel * world.prices[hh.location, fuel]
        offer = world.stock.copy()
        offer[:, food] = market.by_village(plan.spare, hh.location, n) + world.farm_grain
        stock_before = world.stock.copy()
        shared = market.share_with_neighbours(hh.money, essentials, hh.location, n, config)
        # The council helps families who still can't afford them.
        keep = 2.0 * world.council_costs
        cash_relief = council.cash_relief(
            world.council, hh.money, np.maximum(essentials - hh.money, 0.0), hh.location, keep, config
        )

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
                # Replace worn tools, and close any other gap over a few months.
                boost = np.array([b.tool_boost for b in config.businesses])
                gap = np.maximum(workers - world.tools, 0.0)
                wanted = np.minimum(gap, config.trade.tool_wear * workers + gap / config.trade.stock_target_months)
                return np.where(boost > 0, wanted, 0.0)
            if per_unit.any():
                return np.maximum(possible * per_unit - world.supplies[:, :, product], 0.0)
            return None

        # Grain: families short of it buy from families with grain to spare
        # and from the farms' store, all at the village price.
        sale = market.buy(hh.money, plan.want, world.food_price, offer[:, food], hh.location)
        demand[:, food], sold[:, food] = sale.demand, sale.sold
        bought[food] = sale.bought
        sold_by_family, sold_by_farms = farms.sellers_share(sale.sold, plan.spare, world.farm_grain, hh.location)
        grain_receipts = sold_by_family * world.food_price[hh.location]
        world.cash[:, farm] += sold_by_farms * world.food_price
        world.farm_grain[:] -= sold_by_farms
        hh.grain -= plan.own + sold_by_family

        for product, spec in enumerate(config.products):
            if spec.use == "food":
                continue
            elif spec.use == "heating":
                trade(product, family_fuel, hh.money, business_want(product))
            elif spec.use == "comfort":
                spare = np.maximum(0.0, hh.money - config.needs.savings_months * usual_costs)
                spend = config.needs.spare_spending * spare
                price = world.prices[hh.location, product]
                want = np.divide(spend, price, out=np.zeros_like(spend), where=(spend > 0) & (price > 0))
                trade(product, want, spend, business_want(product))
            else:
                trade(product, None, None, business_want(product))

        # The council gives food from its reserve to families who couldn't eat enough.
        relief = council.give_relief(world.council, plan.own + bought[food], family_food, hh.location, config)
        family_eaten = plan.own + bought[food] + relief
        family_share = np.divide(family_eaten, family_food, out=np.ones_like(family_food), where=family_food > 0)
        # Families who couldn't buy all their firewood gather some from the
        # commons, unless the woods have burned.
        can_gather = config.needs.gathering * family_fuel * mods.production_mult[hh.location, wood]
        gathered = np.clip(family_fuel - bought[fuel], 0.0, can_gather)
        warmth = np.divide(bought[fuel] + gathered, family_fuel, out=np.ones_like(family_fuel), where=family_fuel > 0)
        eaten = market.by_village(family_eaten, hh.location, n)
        share = np.divide(eaten, need, out=np.ones_like(need), where=need > 0)
        self._track_shortages(share)

        # 6. Businesses pay out their takings as wages. Keep a running average
        # of what each trade pays (or could pay, if nobody works in it).
        supplies_cost = (economy.input_needs(config)[None, :, :] * world.prices[:, None, :]).sum(axis=2) * possible
        tool = config.product_for("tool")
        boost = np.array([b.tool_boost for b in config.businesses])
        tools_cost = np.where(boost > 0, config.trade.tool_wear * workers * world.prices[:, tool, None], 0.0)
        income, payout = market.pay_wages(world.cash, pop, n_hh, config, keep=supplies_cost + tools_cost)
        # Farm workers are paid mostly in grain: count its value too.
        earned = payout.copy()
        earned[:, farm] += labour_grain * world.food_price
        paid = np.divide(earned, workers, out=np.zeros_like(earned), where=workers > 0)
        empty = workers < 1
        hoped = economy.potential_pay(world.prices, per_worker, config)
        weight = config.trade.pay_memory
        world.pay = np.where(empty, hoped, (1 - weight) * world.pay + weight * paid)
        all_workers = workers.sum(axis=1)
        average_pay = np.divide((world.pay * workers).sum(axis=1), all_workers, out=np.zeros(n), where=all_workers > 0)

        # The council taxes wages and grain sales (while its treasury is below
        # target), and pays its officials and healers.
        cc = config.council
        official, healer = council.official_job(config), healthcare.healer_job(config)
        officials = rules.by_location(np.where(pop.job == official, pop.count, 0).astype(np.float64), pop, n)
        healers = rules.by_location(np.where(pop.job == healer, pop.count, 0).astype(np.float64), pop, n)
        official_pay = cc.official_pay * average_pay
        healer_pay = config.healthcare.healer_pay * average_pay
        running_costs = officials * official_pay + healers * healer_pay
        income += grain_receipts
        taxes = council.collect_taxes(income, hh.location, world.council, cc.treasury_months * running_costs, famine, config)
        hh.money += income
        staff_income, _ = council.pay_staff(pop, world.council, official, official_pay, n_hh)
        hh.money += staff_income
        healer_income, _ = council.pay_staff(pop, world.council, healer, healer_pay, n_hh)
        hh.money += healer_income
        world.council_costs = running_costs
        # The customary wage that fair prices are reckoned in rises slowly
        # while families hold more money than `money_months` of their coin
        # earnings (averaged over a year), and falls while they hold less:
        # prices follow the money there is.
        this_month = market.by_village(income + staff_income + healer_income, hh.location, n)
        if world.coin_earnings is None:
            world.coin_earnings = this_month
        world.coin_earnings = (1 - 1 / 12) * world.coin_earnings + this_month / 12
        earnings = world.coin_earnings
        held = market.by_village(hh.money, hh.location, n)
        months_held = np.divide(held, config.trade.money_months * earnings, out=np.ones(n), where=earnings > 0)
        world.wage_level *= 1.0 + config.trade.wage_adjustment * np.clip(months_held - 1.0, -1.0, 1.0)

        # 7. Some stored food spoils: in families' stores, the farms' and the council's reserve.
        spoiled = market.by_village(rules.spoil(hh.grain, config.food.spoilage), hh.location, n)
        spoiled += rules.spoil(world.farm_grain, config.food.spoilage)
        rules.spoil(world.council.reserve, config.food.spoilage)

        # 8. Health follows each family's food and warmth, plus events.
        coldness = firewood_need / max(config.needs.firewood)
        cold = config.needs.cold_damage * coldness * (1.0 - warmth)
        rules.update_health(pop, family_share[pop.household], mods.health_delta[pop.location] - cold[pop.household], config)

        # Healers see the people most likely to die first.
        risk = rules.death_chance(pop, mods.mortality_mult, config)
        care, treated = healthcare.treat(pop, np.where(world.council.formed, healers, 0.0), risk, config)

        # 9. Deaths, births, ageing. Businesses with more orders than workers
        # take people on; those with too many let some go to trades that are
        # short. Farms aim to grow a little more than the village eats.
        died = rules.deaths(pop, mods.mortality_mult, config, streams["deaths"], n, care)
        cover = rules.wage_cover(average_pay, world.food_price, need, all_workers)
        willing = rules.marriage_factor(cover, config)
        born = rules.births(pop, mods.fertility_mult, config, streams["births"], n)
        rules.grow_older(pop)
        people = rules.by_location(pop.count.astype(np.float64), pop, n)
        for location in np.flatnonzero(council.check_formation(world.council, people, config)):
            self._note(location, f"The village has formed a council: {cc.tax_rate:.0%} tax, officials, a food reserve")
        council.staff(pop, world.council, official, cc.officials_per_1000, config, streams["council"])
        per_1000 = config.healthcare.healers_per_1000 if config.healthcare.enabled else 0.0
        council.staff(pop, world.council, healer, per_1000, config, streams["council"])
        # Firewood orders count families' need at its yearly average, not
        # this month's, so woodcutters work steadily all year.
        weight = config.trade.orders_memory
        families_heating = market.by_village(family_fuel, hh.location, n)
        steady = demand.copy()
        if firewood_need > 0:
            steady[:, fuel] += families_heating * (heating.mean() / firewood_need - 1.0)
        world.orders = (1 - weight) * world.orders + weight * steady
        orders = world.orders[:, product_of]
        orders[:, farm] = need * (1.0 + config.food.reserve_margin)
        pull = world.markup[:, product_of] ** config.trade.hiring_price_response
        work_needed = orders / np.maximum(per_worker, 1e-9) * pull
        wanted = economy.wanted_workers(work_needed, economy.headcount(pop, n, config), config)
        economy.assign_new_workers(pop, wanted, config)
        switched = economy.move_workers(pop, wanted, config, streams["jobs"])
        # Measure families before weddings move young people into new homes.
        underfed = int(pop.count[family_share[pop.household] < 0.9].sum())
        no_land = hh.land <= 0
        landless_need = family_food[no_land].sum()
        landless_ration = float(family_eaten[no_land].sum() / landless_need) if landless_need > 0 else 1.0
        wealth = hh.money + hh.grain * world.food_price[hh.location]
        poorest = metrics.poorest_fifth_ration(wealth, households.sizes(pop, n_hh), family_eaten, family_food)
        weddings = households.marry(pop, hh, config, streams["marriage"], willing)
        size = households.sizes(pop, len(hh))
        households.pass_on_savings(hh, size, world.council.treasury, world.council.formed)
        levied += households.pass_on_land_and_grain(hh, size, world.council.reserve, world.council.formed)

        # 10. Mark-ups move with the gap between demand and supply: for most
        # goods, what was asked for against what was made this month plus any
        # stock piled up beyond the target.
        months = config.trade.stock_target_months
        target_by_product = months * world.orders
        excess = np.maximum(stock_before - target_by_product, 0.0) / months
        made_by_product = np.zeros_like(world.stock)
        made_by_product[:, product_of] = made
        supply = made_by_product + excess
        # Firewood: steady orders against what was cut plus the stock beyond
        # the usual stock and next season's buffer (or short of them).
        ahead = target_by_product[:, fuel] + buffer[world.month_of_year % 12] * usual_heating
        supply[:, fuel] = np.maximum(made_by_product[:, fuel] + (world.stock[:, fuel] - ahead) / 12, 0.0)
        food_markup = world.markup[:, food].copy()
        world.markup = market.adjust_markup(world.markup, steady, supply, config)
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
            weddings=weddings,
            births=int(born.sum()),
            deaths=int(died.sum()),
            food_produced=float(produced.sum()),
            food_needed=float(need.sum()),
            food_eaten=float(eaten.sum()),
            own_food=float(plan.own.sum()),
            grain_sold=float(sale.sold.sum()),
            ration=float(eaten.sum() / need.sum()) if need.sum() > 0 else 1.0,
            food_spoiled=float(spoiled.sum()),
            food_lost=float(lost.sum()),
            food_levied=float(levied.sum()),
            food_stock=float(world.granary.sum()),
            food_margin=float(normal.sum() / need.sum()) if need.sum() > 0 else 0.0,
            food_cover=float(np.average(supply_cover, weights=np.maximum(need, 1e-9))),
            wage_cover=float(np.average(cover, weights=np.maximum(need, 1e-9))),
            food_price=float(np.average(world.food_price, weights=np.maximum(need, 1e-9))),
            wage=float(earned.sum() / workers.sum()) if workers.sum() > 0 else 0.0,
            savings=float(hh.money.sum()),
            money_months=float(held.sum() / earnings.sum()) if earnings.sum() > 0 else 0.0,
            business_cash=float(world.cash.sum()),
            treasury=float(world.council.treasury.sum()),
            councils=int(world.council.formed.sum()),
            officials=int(round(officials.sum())),
            taxes=float(taxes.sum()),
            food_reserve=float(world.council.reserve.sum()),
            relief=float(relief.sum()),
            healers=int(round(healers.sum())),
            treated=int(round(treated.sum())),
            cash_relief=float(cash_relief.sum()),
            shared=float(shared.sum()),
            underfed=underfed,
            landless=int(size[(hh.land <= 0) & (size > 0)].sum()),
            landless_ration=landless_ration,
            poorest_fifth_ration=poorest,
            warmth=float((bought[fuel] + gathered).sum() / family_fuel.sum()) if family_fuel.sum() > 0 else 1.0,
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
