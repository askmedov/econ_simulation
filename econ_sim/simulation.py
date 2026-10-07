"""The monthly loop: the same steps, in the same order, every month."""

from __future__ import annotations

import calendar

from econ_sim import events, metrics, rules
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
        mods = events.modifiers(world)
        lost = world.granary * mods.granary_loss
        world.granary -= lost

        # 2. Production goes into the granary.
        capacity = rules.capacity(pop, world.land, mods.production_mult, config, streams["production"])
        produced = capacity * rules.season_factors(config)[world.month_of_year - 1]
        world.granary += produced
        world.expected_capacity = rules.update_expectation(world.expected_capacity, capacity, config)

        # 3. Everyone eats; if food looks short for the year ahead, all get the same smaller ration.
        need = rules.by_location(rules.food_need(pop, config), pop, n)
        ration = rules.plan_ration(world.granary, need, world.expected_capacity, world.month_of_year, config)
        eaten, share = rules.consume(world.granary, need, ration)
        self._track_shortages(share)

        # 4. Some stored food spoils.
        spoiled = rules.spoil(world.granary, config.food.spoilage)

        # 5. Health responds to food and events.
        rules.update_health(pop, share, mods.health_delta, config)

        # 6. Deaths, births, ageing.
        died = rules.deaths(pop, mods.mortality_mult, config, streams["deaths"], n)
        born = rules.births(pop, mods.fertility_mult, config, streams["births"], n)
        rules.grow_older(pop)

        # 7. Record the month.
        children, workers, elderly = metrics.age_groups(pop, config)
        record = MonthRecord(
            month_number=month_number,
            year=world.year,
            month=world.month_of_year,
            population=pop.size,
            children=children,
            workers=workers,
            elderly=elderly,
            births=int(born.sum()),
            deaths=int(died.sum()),
            food_produced=float(produced.sum()),
            food_needed=float(need.sum()),
            food_eaten=float(eaten.sum()),
            ration=float(eaten.sum() / need.sum()) if need.sum() > 0 else 1.0,
            food_spoiled=float(spoiled.sum()),
            food_lost=float(lost.sum()),
            food_stock=float(world.granary.sum()),
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
            if fed < 1.0 and started is None:
                self._short_since[location] = self.world.month
                self._note(location, f"Rationing began: {fed:.0%} of full rations")
            elif fed >= 1.0 and started is not None:
                months = self.world.month - started
                self._note(location, f"Full rations again after {months} month{'s' * (months != 1)}")
                del self._short_since[location]

    def _active_event_names(self) -> str:
        world = self.world
        names = []
        for event in world.active_events:
            prefix = f"{world.names[event.location]}:" if world.n_locations > 1 else ""
            names.append(prefix + event.spec.name)
        return "+".join(names)
