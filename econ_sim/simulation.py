"""The monthly loop: the same steps, in the same order, every month."""

from __future__ import annotations

import calendar

import numpy as np

from econ_sim import (
    council, credit, economy, environment, events, farms, healthcare, households, livestock, lords, market, metrics,
    migration,
    rules, town, work,
)
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
        if any(v.population < 1 for v in config.villages):
            raise ValueError("every village needs at least one person at the start")
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
        # Debts grow by a month's interest; a ruler may cancel them.
        interest = credit.accrue_interest(hh, config)
        cancelled = credit.cancel_debts(hh, mods.debt_cancel)
        # Soldiers or raiders take grain and animals.
        requisitioned, taken_from_lord = lords.requisition(hh, world.lord, world.farm_grain, mods.requisition)
        # The region's grain price swings; a ruler may debase the coinage;
        # some coins are lost or buried.
        town.drift(world.town, config, streams["town"])
        if mods.debase.max() > 0:
            world.town.level /= 1.0 - mods.debase.max()
        coins_lost = town.lose_coins(hh.money, world.town, config)
        # Fire or pests strike stored grain: families' stores and the farms'.
        lost_by_family = hh.grain * mods.granary_loss[hh.location]
        hh.grain -= lost_by_family
        lost = market.by_village(lost_by_family, hh.location, n) + world.farm_grain * mods.granary_loss
        world.farm_grain[:] *= 1.0 - mods.granary_loss
        # Herds grow, or die off in bad weather; each November they are
        # thinned to what can be fed through winter, the meat into stores.
        animals_lost = livestock.grow_and_die(hh, mods.production_mult[:, farm], mods.heating_mult, config)
        culled, meat = np.zeros(n), np.zeros(n)
        if world.month_of_year == 11:
            culled, meat = livestock.winter_cull(hh, world.land, config)
        # Families with no food and no coins slaughter their animals.
        hungry_need = market.by_household(rules.food_need(pop, config), pop, len(hh))
        killed, eaten_animals = livestock.slaughter_in_hunger(hh, hungry_need, world.food_price, config)
        culled, meat = culled + killed, meat + eaten_animals

        # Wood takes longer to cut and gather where the woods have thinned.
        wood_trade = economy.seller_of(config)[config.product_for("heating")]
        mods.production_mult[:, wood_trade] *= environment.woods_reach(world.woods, world.woods_normal, config)

        # 2. Businesses make goods, using up supplies and wearing out tools.
        # Other than farms, they work less when unsold goods pile up. Farms
        # grow more with plough animals, and only as much as was sown.
        product_of = economy.product_of(config)
        workers = economy.headcount(pop, n, config)
        effort = economy.labor(pop, n, config, rng=streams["production"])
        # At harvest everyone helps in the fields: the trades lose part of
        # their days, and children and the old glean, bind and carry.
        helping = work.harvest_help(pop, n, len(hh), world.month_of_year, config)
        effort = effort * helping.kept
        effort[:, farm] += helping.farm
        herd = market.by_village(hh.animals, hh.location, n)
        animal_factor = livestock.farm_factor(herd, world.land, config)
        farm_mult = animal_factor * world.soil  # plough animals, and how fertile the soil is
        farm_output = config.businesses[farm].output * economy.tool_factor(world.tools, workers, config)[:, farm]
        farmed = economy.land_in_use(world.land, workers[:, farm], config, 12.0 * farm_output * farm_mult)
        possible = economy.capacity(
            effort, world.tools, workers, world.land, mods.production_mult, config, farm_mult, world.sown
        )
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

        # Prices: what each product fairly costs to make in a normal year
        # (a poor sowing or harvest shows in the grain mark-up), times its mark-up.
        per_worker = economy.productivity(pop, world.tools, workers, world.land, config, farm_mult)
        world.prices = economy.fair_prices(world.wage_level, per_worker, world.prices, config) * world.markup

        # 3. How well the village's stores and coming harvests cover the year
        # (this sets the grain price and says whether it is a famine year).
        need_rows = rules.food_need(pop, config)
        need = rules.by_location(need_rows, pop, n)
        full_strength = economy.labor(pop, n, config, at_full_health=True)
        no_events = np.ones_like(mods.production_mult)
        normal_gross = economy.capacity(full_strength, world.tools, workers, world.land, no_events, config, farm_mult)[:, farm]
        months_ahead = max(config.food.planning_months - 1, 0)
        ahead = events.production_outlook(world, months_ahead, names[farm])
        ahead = ahead * farms.sown_outlook(world.sown, world.month_of_year, ahead.shape[1], config)
        if config.work.enabled:  # harvests gathered with everyone's help
            at_harvest = work.harvest_help(pop, n, len(hh), config.work.harvest_months[0], config).farm
            boost = work.harvest_boost(full_strength[:, farm], at_harvest, config)
            ahead = ahead * work.outlook_boost(boost, world.month_of_year, ahead.shape[1], config)
        # Food to come: the harvests expected, less the seed picked from them,
        # plus what merchants bring from the town (or less what they carry off).
        harvests = rules.harvest_outlook(normal_gross, ahead, world.month_of_year, config)
        seed_ahead = farms.seed_outlook(farms.seed_needed(farmed, config), world.month_of_year, ahead.shape[1], config)
        outlook = harvests - np.minimum(seed_ahead, config.food.max_seed_share * harvests)
        town_price = town.price(world.town, world.month_of_year, mods.production_mult[:, farm], config)
        carts = town.merchants(world.food_price, town_price, need, config)
        outlook = np.maximum(outlook + (carts.imports - carts.exports)[:, None], 0.0)
        # This month's seed is about to be picked from the harvest: not food.
        seed_needed = farms.seed_needed(farmed, config)
        if world.seed is None:  # the first month: seed already picked this season
            world.seed = seed_needed * farms.seed_gathered(world.month_of_year, config)
        seed_now = farms.seed_due(world.farm_grain, world.seed, seed_needed, produced, world.month_of_year, config)
        supply_cover = rules.plan_ration_realistically(world.granary - seed_now, need, outlook, config, most=2.0)
        famine = supply_cover < 1.0

        # The harvest is shared out in kind. The council's levy comes off the
        # top, then (at harvest time) next year's seed; the farms keep enough
        # to sell for new tools; the rest goes to the owners of the plough
        # animals, the families who worked the fields and the families who
        # hold them.
        n_hh = len(hh)
        levied = council.levy_grain(world.council, produced, world.farm_grain, need, famine, config)
        seed_kept = farms.keep_seed(world.farm_grain, world.seed, seed_needed, produced, world.month_of_year, config)
        tool = config.product_for("tool")
        tools_value = config.trade.tool_wear * workers[:, farm] * world.prices[:, tool]
        keep_for_tools = np.divide(tools_value, world.food_price, out=np.zeros(n), where=world.food_price > 0)
        shares, to_lord = farms.income_shares(
            pop, hh, world.land, config, livestock.owners_part(animal_factor), world.lord.land, helping.households
        )
        harvest_share, rent = farms.share_harvest(hh, world.farm_grain, keep_for_tools, shares, to_lord)
        share_out = market.by_village(harvest_share, hh.location, n) + rent
        service = np.where(world.lord.land > 0, config.lord.labour_service, 0.0)
        labour_grain = config.food.labor_share * (1 - livestock.owners_part(animal_factor)) * (1 - service) * share_out
        # The lord's share goes to his barn; part of it is carted away to his hall.
        lords.into_barn(world.lord, rent)
        carted_away = lords.cart_away(world.lord, config)

        # 4. What families need this month. They eat from their own store as
        # far as it lasts until their next harvest, offer what they can
        # spare, and need coins for the rest of their food and for firewood.
        members = market.by_household(pop.count.astype(np.float64), pop, n_hh)
        family_food = market.by_household(need_rows, pop, n_hh)
        levy_rate = np.where(world.council.formed, config.council.grain_levy, 0.0)
        plan = farms.plan_family_food(hh, family_food, outlook, shares, 1.0 - levy_rate, config, world.markup[:, food])
        # Kin with grain to spare help families who can't afford their food.
        affordable = np.divide(hh.money, world.food_price[hh.location], out=np.zeros(n_hh), where=world.food_price[hh.location] > 0)
        from_kin = work.kin_help(hh, members > 0, plan.want - affordable, plan.spare, config)
        # It goes to this month's food; the givers have that much less to sell.
        plan.own += np.maximum(from_kin, 0.0)
        plan.want = np.maximum(plan.want - np.maximum(from_kin, 0.0), 0.0)
        plan.spare = np.maximum(plan.spare + np.minimum(from_kin, 0.0), 0.0)
        firewood_need = heating[world.month_of_year - 1]
        family_fuel = members * firewood_need * mods.heating_mult[hh.location]
        essentials = plan.want * world.prices[hh.location, food] + family_fuel * world.prices[hh.location, fuel]
        # Families keep coins for a few months of firewood and of whatever food
        # their own store won't cover, before they spend on comforts, lend, or
        # buy animals and land.
        months = config.needs.savings_months
        uncovered = np.maximum(months * family_food - hh.grain, 0.0)
        coin_target = uncovered * world.prices[hh.location, food] + months * family_fuel * world.prices[hh.location, fuel]
        offer = world.stock.copy()
        lord_offer = lords.offer(world.lord, config)
        offer[:, food] = market.by_village(plan.spare, hh.location, n) + world.farm_grain + lord_offer + carts.imports
        stock_before = world.stock.copy()
        shared = market.share_with_neighbours(hh.money, essentials, hh.location, n, config)
        # The council helps families who still can't afford them.
        keep = 2.0 * world.council_costs
        cash_relief = council.cash_relief(
            world.council, hh.money, np.maximum(essentials - hh.money, 0.0), hh.location, keep, config
        )
        # Families still short sell animals to families with coins to spare;
        # when many must sell at once, the price collapses.
        worth = config.livestock.value_months * world.wage_level
        animal_sales = livestock.distress_sales(
            hh, np.maximum(essentials - hh.money, 0.0), hh.money - coin_target, worth, config
        )
        # Then they borrow against their land and animals (and a little on
        # their word); failing that, they sell land. Land is worth some years
        # of its rent at the usual grain price.
        normal_food_price = world.food_price / np.maximum(world.markup[:, food], 1e-9)
        yearly_net = np.maximum(12.0 * normal_gross - seed_needed, 0.0)
        land_part = (1.0 - config.food.labor_share) * (1.0 - livestock.owners_part(animal_factor))
        rent_per_plot = np.divide(land_part * yearly_net, world.land, out=np.zeros(n), where=world.land > 0) * normal_food_price
        plot_price = credit.land_price(rent_per_plot, config)
        # Only food is worth debt or land: firewood can be gathered.
        food_cost = plan.want * world.food_price[hh.location]
        borrowed = np.zeros(n)
        if config.credit.enabled:
            borrowed = credit.borrow(
                hh, np.maximum(food_cost - hh.money, 0.0), hh.money - coin_target,
                credit.collateral(hh, plot_price, worth), config.credit.personal_months * world.wage_level[hh.location],
                config,
            )
        land_sales = credit.sell_land(hh, np.maximum(food_cost - hh.money, 0.0), hh.money - coin_target, plot_price, config)

        # 5. Markets, most needed first. Businesses buy supplies and tools
        # with part of their cash, before paying wages.
        seller = economy.seller_of(config)
        demand = np.zeros_like(world.stock)
        sold = np.zeros_like(world.stock)
        bought = {}

        def trade(product: int, family_want: np.ndarray | None, family_money: np.ndarray | None,
                  business_want: np.ndarray | None, lord_spend: np.ndarray | None = None) -> None:
            money_parts, want_parts, where = [], [], []
            if family_want is not None:
                money_parts.append(family_money)
                want_parts.append(family_want)
                where.append(hh.location)
            if lord_spend is not None:
                price = world.prices[:, product]
                money_parts.append(lord_spend.copy())
                want_parts.append(np.divide(lord_spend, price, out=np.zeros(n), where=price > 0))
                where.append(np.arange(n))
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
            if lord_spend is not None:
                first = n_hh if family_want is not None else 0
                world.lord.purse -= sale.spent[first:first + n]
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

        # Grain: families short of it buy from the farms' store, merchants'
        # grain from the town, and families and the lord with grain to spare,
        # all at the village price. Then merchants buy what is left, if the
        # village is cheap enough to carry grain to the town.
        sale = market.buy(hh.money, plan.want, world.food_price, offer[:, food], hh.location)
        demand[:, food] = sale.demand
        bought[food] = sale.bought
        # (merchants never bring grain and carry it off in the same month)
        exported = np.minimum(carts.exports, np.maximum(offer[:, food] - sale.sold, 0.0))
        sellers = farms.sellers_share(
            sale.sold + exported, plan.spare, world.farm_grain, hh.location, lord_offer, carts.imports
        )
        sold[:, food] = sale.sold + exported
        sold_by_family, sold_by_lord = sellers.families, sellers.lord
        lords.sold(world.lord, sold_by_lord, world.food_price)
        grain_receipts = sold_by_family * world.food_price[hh.location]
        world.cash[:, farm] += sellers.farms * world.food_price
        world.farm_grain[:] -= sellers.farms
        hh.grain -= plan.own + sold_by_family
        imported = sellers.imports
        world.town.purse += float(((imported - exported) * world.food_price).sum())

        homespun = work.home_cloth(pop, n_hh, world.month_of_year, config)
        for product, spec in enumerate(config.products):
            if spec.use == "food":
                continue
            elif spec.use == "heating":
                trade(product, family_fuel, hh.money, business_want(product))
            elif spec.use == "comfort":
                spare = np.maximum(0.0, hh.money - coin_target)
                price = world.prices[hh.location, product]
                # Families wear their homespun first and buy less cloth.
                spend = np.maximum(config.needs.spare_spending * spare - homespun * price, 0.0)
                want = np.divide(spend, price, out=np.zeros_like(spend), where=(spend > 0) & (price > 0))
                # The lord's household buys the village's cloth too.
                trade(product, want, spend, business_want(product), lords.local_spending(world.lord, config))
            else:
                trade(product, None, None, business_want(product))

        # The council gives food from its reserve to families who couldn't eat enough.
        relief = council.give_relief(world.council, plan.own + bought[food], family_food, hh.location, config)
        # Families still hungry find famine foods: roots, greens, nuts, fish
        # (more in summer and autumn; fewer in a drought).
        summer = 4 / 3 if 5 <= world.month_of_year <= 10 else 2 / 3
        can_find = config.food.foraging * summer * np.minimum(mods.production_mult[:, farm], 1.0)
        lord_relief = lords.charity(
            world.lord, np.maximum(family_food - plan.own - bought[food] - relief, 0.0), hh.location, famine, config
        )
        council_relief = relief
        relief = relief + lord_relief
        fed = plan.own + bought[food] + relief
        foraged = np.clip(family_food - fed, 0.0, family_food * can_find[hh.location])
        family_eaten = fed + foraged
        family_share = np.divide(family_eaten, family_food, out=np.ones_like(family_food), where=family_food > 0)
        # Families who couldn't buy all their firewood gather some from the
        # commons: dung, straw and furze, and wood unless the woods have
        # burned or thinned.
        other = config.needs.other_fuels
        can_gather = config.needs.gathering * family_fuel * (other + (1.0 - other) * mods.production_mult[hh.location, wood])
        gathered = np.clip(family_fuel - bought[fuel], 0.0, can_gather)
        # The woods regrow, less what was cut and gathered, and what burned;
        # the soil tires on crowded land and recovers on land left to rest.
        cut = made[:, wood] + market.by_village(gathered * (1.0 - other), hh.location, n)
        woods_burned = environment.grow_woods(world.woods, world.woods_capacity, cut, mods.woods_burned, config)
        environment.tire_soil(world.soil, people_here, world.land, config)
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
        # Borrowers pay part of their income toward their debts; debts that
        # outgrow a family's animals and land cost them those.
        repaid = credit.repay(hh, income + staff_income + healer_income, config, spare=hh.money - coin_target)
        repaid_in_grain = credit.repay_in_grain(hh, family_food, world.food_price, config)
        foreclosed = credit.foreclose(hh, plot_price, worth, config)
        # Once a year, after harvest, the state collects its tax in coin
        # (or seizes grain from families without the coins).
        state_tax = lords.TaxTake(coins=np.zeros(n), grain=np.zeros(n))
        if world.month_of_year == config.state.collection_month:
            state_tax = lords.collect_state_tax(
                hh, world.lord, world.wage_level, rent_per_plot, world.food_price, famine, config, members > 0,
                keep=coin_target,
            )
        limit = config.credit.loan_to_value * credit.collateral(hh, plot_price, worth)
        written_off = credit.default(hh, limit + config.credit.personal_months * world.wage_level[hh.location], config)
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
        rules.spoil(world.lord.barn, config.food.spoilage)
        # Sowing, after the month's harvest is in.
        seed_from_families = np.zeros(n)
        if world.month_of_year == farms.sowing_month(config):
            world.sown, seed_from_families = farms.sow(hh, world.seed, seed_needed, family_food)

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
        # How well a family can live here: by its wage, or by what the land
        # yields per person after seed and the lord's share (a village
        # thinned by famine has land to spare, and marries and draws people).
        lord_part = np.divide(world.lord.land, world.land, out=np.zeros(n), where=world.land > 0)
        land_cover = np.divide(
            (normal_gross - seed_needed / 12.0) * (1.0 - (1.0 - config.food.labor_share) * lord_part), need,
            out=np.zeros(n), where=need > 0,
        )
        prospects = np.maximum(cover, land_cover)
        willing = rules.marriage_factor(prospects, config)
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
        # Young people leave for the town (and starving families flee); when
        # hands are short, young people come from the region.
        moves = migration.leave(pop, hh, cover, family_share, config, streams["migration"])
        world.town.purse += moves.coins
        moves.arrived = migration.arrive(pop, hh, prospects, config, streams["migration"])
        weddings = households.marry(pop, hh, config, streams["marriage"], willing)
        size = households.sizes(pop, len(hh))
        credit.write_off(hh, size == 0)
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
            foraged=float(foraged.sum()),
            grain_sold=float(sale.sold.sum()),
            ration=float(eaten.sum() / need.sum()) if need.sum() > 0 else 1.0,
            food_spoiled=float(spoiled.sum()),
            food_lost=float(lost.sum()),
            food_levied=float(levied.sum()),
            food_to_seed=float((seed_kept + seed_from_families).sum()),
            meat=float(meat.sum()),
            seed_store=float(world.seed.sum()),
            sown=float(world.sown.mean()),
            animals=float(hh.animals.sum()),
            animals_lost=float(animals_lost.sum() + culled.sum()),
            animals_sold=float(animal_sales.sold.sum()),
            animal_price=float(animal_sales.price.mean()),
            debt=float(hh.debt.sum()),
            debtors=int(((hh.debt > 0.01) & (size > 0)).sum()),
            interest=float(interest.sum()),
            borrowed=float(borrowed.sum()),
            repaid=float(repaid.sum()),
            repaid_in_grain=float(repaid_in_grain.sum()),
            debts_cancelled=float(cancelled.sum()),
            debts_written_off=float(written_off.sum()),
            land_sold=float(land_sales.plots.sum()),
            land_foreclosed=float(foreclosed.land.sum()),
            animals_foreclosed=float(foreclosed.animals.sum()),
            land_price=float(plot_price.mean()),
            food_rent=float(rent.sum()),
            lord_carted=float(carted_away.sum()),
            lord_sold=float(sold_by_lord.sum()),
            lord_relief=float(lord_relief.sum()),
            lord_barn=float(world.lord.barn.sum()),
            lord_purse=float(world.lord.purse.sum()),
            state_tax=float(state_tax.coins.sum()),
            tax_grain=float(state_tax.grain.sum()),
            state_purse=float(world.lord.state_purse.sum()),
            food_requisitioned=float(requisitioned.sum()),
            town_price=float(town_price.mean()),
            grain_exported=float(exported.sum()),
            grain_imported=float(imported.sum()),
            town_purse=float(world.town.purse),
            coins_lost=float(world.town.coins_lost),
            emigrants=int(moves.left.sum()),
            immigrants=int(moves.arrived.sum()),
            food_emigrated=float(moves.grain),
            food_stock=float(world.granary.sum()),
            food_margin=float((normal_gross - seed_needed / 12.0).sum() / need.sum()) if need.sum() > 0 else 0.0,
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
            relief=float(council_relief.sum()),
            healers=int(round(healers.sum())),
            treated=int(round(treated.sum())),
            cash_relief=float(cash_relief.sum()),
            shared=float(shared.sum()),
            underfed=underfed,
            landless=int(size[(hh.land <= 0) & (size > 0)].sum()),
            landless_ration=landless_ration,
            poorest_fifth_ration=poorest,
            warmth=float((bought[fuel] + gathered).sum() / family_fuel.sum()) if family_fuel.sum() > 0 else 1.0,
            clothing=float((bought[config.product_for("comfort")] + homespun).sum() / max(pop.size, 1)),
            homespun=float(homespun.sum()),
            harvest_help=float(helping.farm.sum()),
            kin_help=float(np.maximum(from_kin, 0.0).sum()),
            woods=float(world.woods.sum() / max(world.woods_normal.sum(), 1e-9)),
            woods_burned=float(woods_burned.sum()),
            soil=float((world.soil * world.land).sum() / max(world.land.sum(), 1e-9)),
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
