//! The chronicle: what happened in a run, ranked by importance, through time.
//!
//! Each month it notes the events that started, famines and shortages that
//! began or ended, deaths and departures far above the usual, grain, tools
//! and salt grown dear, a hungry city, councils formed and villages emptied.
//! An entry names the places it touched, worst first, under it. Entries are
//! of four ranks (`ChronicleConfig`): 1, what touches a large share of the
//! region's people, and every forced event; 2, a smaller but notable share,
//! or a town; 3, a place or a few; 4, the small change of a month. Written
//! out, the chronicle goes year by year (the year in brief first, then its
//! months), each month's entries from the most important down.
//!
//! The usual deaths, departures and prices are running averages of the
//! months without a surge, so a long dearth is still a dearth.

use serde::Serialize;

use crate::config::{ChronicleConfig, Config, EffectValue, EventSpec};
use crate::geography::{thousands, Geography};
use crate::metrics::MonthRecord;

const MONTH_NAME: [&str; 13] = [
    "",
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// What the ranks mean, for the head of a written chronicle.
pub const RANKS: [&str; 4] = [
    "1: across much of the region, or forced on the run",
    "2: a notable share of the region, or a town",
    "3: a place or a few",
    "4: the small change of a month",
];

/// A place under an entry, and what happened there.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Detail {
    pub place: String,
    pub what: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Entry {
    pub month_number: i64, // 1 = the first month of the run
    pub year: i64,
    pub month: i64, // calendar month, 1-12
    pub rank: u8,
    pub kind: &'static str,
    pub what: String,
    pub people: f64,         // people it touched (orders entries of a rank)
    pub places: Vec<Detail>, // worst first, at most `places_listed`
    pub more_places: usize,  // the rest, counted
}

/// The chronicle so far, and what it remembers between months.
#[derive(Clone, Debug, Default)]
pub struct Chronicle {
    pub entries: Vec<Entry>,
    famine: Vec<bool>,
    short: Vec<bool>,
    lived_in: Vec<bool>,
    usual_deaths: Option<f64>,  // a month, per person, in the region
    usual_leaving: Option<f64>, // a month, per person
    grain: Dearness,
    goods: Vec<(String, Dearness)>, // tools and the staples, by name
    city_hungry: bool,
    famine_people: f64, // in famine last month
}

/// A good's usual price (a running average of the months it was not
/// dear), and whether it is dear.
#[derive(Clone, Debug, Default)]
struct Dearness {
    usual: Option<f64>,
    dear: bool,
}

impl Dearness {
    /// This month's `price`: Some(true) if the good has grown dear (`surge`
    /// times its usual price), Some(false) if it is no longer dear (back
    /// below halfway there); with its usual price, then.
    fn month(&mut self, price: f64, surge: f64) -> (Option<bool>, f64) {
        let usual = *self.usual.get_or_insert(price);
        let change = if !self.dear && price >= surge * usual {
            self.dear = true;
            Some(true)
        } else if self.dear && price < 0.5 * (1.0 + surge) * usual {
            self.dear = false;
            Some(false)
        } else {
            None
        };
        if !self.dear {
            self.usual = Some(usual + (price - usual) / 12.0);
        }
        (change, usual)
    }
}

/// What a month left, per place, for the chronicle.
pub struct Month<'a> {
    pub record: &'a MonthRecord,
    pub people: &'a [f64],
    pub deaths: &'a [f64],
    pub ration: &'a [f64],
    pub left: &'a [f64],
    /// Events that started: (the event, its place, forced on the run).
    pub started: &'a [(usize, usize, bool)],
    pub councils: &'a [usize],
}

/// Where a group of places lies in the region: "across the region", "in the
/// north-west", "in the middle" (empty off a map, or for a place or two).
fn whereabouts(places: &[usize], geo: &Geography) -> String {
    let n = geo.x.len();
    if !geo.on_map || places.len() < 3 || n < 10 {
        return String::new();
    }
    if places.len() * 2 >= n {
        return " across the region".into();
    }
    let mean = |v: &[f64], rows: &mut dyn Iterator<Item = usize>| {
        let (s, k) = rows.fold((0.0, 0), |(s, k), r| (s + v[r], k + 1));
        s / k as f64
    };
    let (cx, cy) = (mean(&geo.x, &mut (0..n)), mean(&geo.y, &mut (0..n)));
    let (gx, gy) = (mean(&geo.x, &mut places.iter().copied()), mean(&geo.y, &mut places.iter().copied()));
    let spread = (0..n).map(|r| (geo.x[r] - cx).hypot(geo.y[r] - cy)).fold(0.0, f64::max);
    // (places scattered over the region lie nowhere in particular)
    let scatter = places.iter().map(|&r| (geo.x[r] - gx).hypot(geo.y[r] - gy)).sum::<f64>() / places.len() as f64;
    if scatter > 0.4 * spread {
        return String::new();
    }
    let (dx, dy) = (gx - cx, gy - cy);
    if dx.hypot(dy) < 0.2 * spread {
        return " in the middle of the region".into();
    }
    let compass = ["east", "north-east", "north", "north-west", "west", "south-west", "south", "south-east"];
    let sector = ((dy.atan2(dx) / std::f64::consts::FRAC_PI_4).round() as i64).rem_euclid(8) as usize;
    format!(" in the {}", compass[sector])
}

/// Whether an event only does good (a good harvest, debts cancelled).
fn boon(spec: &EventSpec) -> bool {
    let number = |v: &EffectValue| match v {
        EffectValue::Fixed(x) => *x,
        EffectValue::Range(lo, hi) => 0.5 * (lo + hi),
    };
    spec.effects.iter().all(|(name, value)| match name.as_str() {
        "production_mult" => number(value) > 1.0,
        "debt_cancel" => true,
        _ => false,
    })
}

/// "salt" -> "Salt".
fn capitalised(word: &str) -> String {
    let mut letters = word.chars();
    letters.next().map_or_else(String::new, |first| first.to_uppercase().chain(letters).collect())
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{} {}", thousands(count as i64), if count == 1 { one } else { many })
}

impl Chronicle {
    pub fn new(n: usize) -> Chronicle {
        Chronicle { famine: vec![false; n], short: vec![false; n], lived_in: vec![true; n], ..Default::default() }
    }

    /// The rank of something touching `people` of the region's `total`.
    fn rank(cfg: &ChronicleConfig, people: f64, total: f64, town: bool) -> u8 {
        let share = people / total.max(1.0);
        if share >= cfg.major_share {
            1
        } else if share >= cfg.notable_share || town {
            2
        } else {
            3
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn note(
        &mut self,
        record: &MonthRecord,
        rank: u8,
        kind: &'static str,
        what: String,
        people: f64,
        mut places: Vec<(f64, Detail)>,
        listed: usize,
    ) {
        places.sort_by(|a, b| b.0.total_cmp(&a.0));
        let more_places = places.len().saturating_sub(listed);
        self.entries.push(Entry {
            month_number: record.month_number,
            year: record.year,
            month: record.month,
            rank,
            kind,
            what,
            people,
            places: places.into_iter().take(listed).map(|(_, d)| d).collect(),
            more_places,
        });
    }

    /// Note what this month brought.
    pub fn month(&mut self, m: &Month, config: &Config, geo: &Geography) {
        let cfg = &config.chronicle;
        let n = m.people.len();
        let names: Vec<&str> = config.villages.iter().map(|v| v.name.as_str()).collect();
        let total: f64 = m.people.iter().sum();
        let r = m.record;
        let listed = cfg.places_listed;
        let is_town = |v: usize| geo.town.get(v).copied().unwrap_or(false);
        let city = config.villages.iter().position(|v| v.stronghold);
        let about = |v: usize| format!("{} people", thousands(m.people[v].round() as i64));

        // Events that started, one entry for each kind (forced ones apart).
        let mut kinds: Vec<(usize, bool)> = Vec::new();
        for &(spec, _, forced) in m.started {
            if !kinds.contains(&(spec, forced)) {
                kinds.push((spec, forced));
            }
        }
        for (spec, forced) in kinds {
            let places: Vec<usize> = m.started.iter().filter(|s| s.0 == spec && s.2 == forced).map(|s| s.1).collect();
            let people: f64 = places.iter().map(|&v| m.people[v]).sum();
            let event = &config.events[spec];
            // (in a single place everything touches everyone: there, an event is of the second rank)
            let mut rank =
                if n == 1 { 2 } else { Self::rank(cfg, people, total, places.iter().any(|&v| Some(v) == city)) };
            if boon(event) {
                rank = (rank + 1).min(4);
            }
            if forced {
                rank = 1;
            }
            // (an event's message speaks of one village: for many, its name part)
            let short = event.message.split(':').next().unwrap_or(&event.message);
            let what = if n == 1 {
                event.message.clone()
            } else if places.len() == 1 {
                format!("{}: {}", names[places[0]], event.message)
            } else {
                format!(
                    "{short} in {} ({} people){}",
                    plural(places.len(), "place", "places"),
                    thousands(people.round() as i64),
                    whereabouts(&places, geo)
                )
            };
            let what = if forced { format!("{what} (forced)") } else { what };
            let details =
                places.iter().map(|&v| (m.people[v], Detail { place: names[v].into(), what: about(v) })).collect();
            self.note(r, rank, "event", what, people, if places.len() > 1 { details } else { Vec::new() }, listed);
        }

        // Famine, as it spreads, eases and ends; places newly short of food.
        let in_famine: Vec<usize> = (0..n).filter(|&v| m.people[v] > 0.0 && m.ration[v] < cfg.famine_ration).collect();
        let newly: Vec<usize> = in_famine.iter().copied().filter(|&v| !self.famine[v]).collect();
        let now: Vec<bool> = (0..n).map(|v| m.people[v] > 0.0 && m.ration[v] < cfg.famine_ration).collect();
        let recovered = (0..n).filter(|&v| self.famine[v] && !now[v]).count();
        let famine_people: f64 = in_famine.iter().map(|&v| m.people[v]).sum();
        let fed = |v: usize| format!("ate {:.0}% of its need", 100.0 * m.ration[v]);
        if !newly.is_empty() {
            let new_people: f64 = newly.iter().map(|&v| m.people[v]).sum();
            let rank = Self::rank(cfg, famine_people, total, newly.iter().any(|&v| is_town(v)));
            let what = if n == 1 {
                format!("Famine: the village ate {:.0}% of its need", 100.0 * m.ration[0])
            } else {
                format!(
                    "Famine in {} ({} people): {} more ({} people){}",
                    plural(in_famine.len(), "place", "places"),
                    thousands(famine_people.round() as i64),
                    thousands(newly.len() as i64),
                    thousands(new_people.round() as i64),
                    whereabouts(&newly, geo)
                )
            };
            let details =
                newly.iter().map(|&v| (1.0 - m.ration[v], Detail { place: names[v].into(), what: fed(v) })).collect();
            self.note(r, rank, "famine", what, famine_people, if n > 1 { details } else { Vec::new() }, listed);
        } else if recovered > 0 && in_famine.is_empty() {
            let rank = if Self::rank(cfg, self.famine_people, total, false) <= 2 { 2 } else { 3 };
            self.note(r, rank, "famine over", "The famine is over".into(), self.famine_people, Vec::new(), listed);
        } else if recovered > 0 {
            let what = format!(
                "Famine eases: {} fed again, {} ({} people) still in famine",
                plural(recovered, "place", "places"),
                thousands(in_famine.len() as i64),
                thousands(famine_people.round() as i64)
            );
            self.note(r, 4, "famine eases", what, famine_people, Vec::new(), listed);
        }
        self.famine_people = famine_people;
        let short: Vec<usize> = (0..n)
            .filter(|&v| m.people[v] > 0.0 && m.ration[v] < cfg.short_ration && m.ration[v] >= cfg.famine_ration)
            .filter(|&v| !self.short[v])
            .collect();
        if !short.is_empty() {
            let people: f64 = short.iter().map(|&v| m.people[v]).sum();
            let what = if n == 1 {
                format!("Food short: the village ate {:.0}% of its need", 100.0 * m.ration[0])
            } else {
                format!(
                    "Food short in {} ({} people)",
                    plural(short.len(), "place", "places"),
                    thousands(people.round() as i64)
                )
            };
            let details =
                short.iter().map(|&v| (1.0 - m.ration[v], Detail { place: names[v].into(), what: fed(v) })).collect();
            self.note(r, 4, "short", what, people, if n > 1 { details } else { Vec::new() }, listed);
        }
        for v in 0..n {
            self.famine[v] = m.people[v] > 0.0 && m.ration[v] < cfg.famine_ration;
            self.short[v] = m.people[v] > 0.0 && m.ration[v] < cfg.short_ration;
        }

        // Deaths far above the usual: in the region, or in places.
        let deaths: f64 = m.deaths.iter().sum();
        let rate = deaths / total.max(1.0);
        let usual = *self.usual_deaths.get_or_insert(rate);
        // (more than chance would bring, too: three standard deviations of a count)
        let surge = |died: f64, people: f64| {
            let expected = usual * people;
            died >= cfg.deaths_surge * expected
                && died - expected >= cfg.fewest_deaths
                && died - expected >= 3.0 * expected.sqrt()
        };
        let surged: Vec<usize> = (0..n).filter(|&v| surge(m.deaths[v], m.people[v])).collect();
        let excess = |v: usize| m.deaths[v] - usual * m.people[v];
        let death_details = |places: &[usize]| -> Vec<(f64, Detail)> {
            places
                .iter()
                .map(|&v| {
                    let what = format!(
                        "{} died ({} usual)",
                        thousands(m.deaths[v].round() as i64),
                        thousands((usual * m.people[v]).round() as i64)
                    );
                    (excess(v), Detail { place: names[v].into(), what })
                })
                .collect()
        };
        let region_surge = surge(deaths, total);
        // (a share of the region's people dying beyond the usual in a month)
        let deaths_rank = |extra: f64| match extra / total.max(1.0) {
            x if x >= 0.002 => 1,
            x if x >= 0.0005 => 2,
            _ => 3,
        };
        if region_surge {
            let extra = deaths - usual * total;
            let rank = deaths_rank(extra).min(2);
            let what = format!(
                "{} deaths, {} more than usual",
                thousands(deaths.round() as i64),
                thousands(extra.round() as i64)
            );
            self.note(r, rank, "deaths", what, extra, if n > 1 { death_details(&surged) } else { Vec::new() }, listed);
        } else if !surged.is_empty() && n > 1 {
            let extra: f64 = surged.iter().map(|&v| excess(v)).sum();
            let rank = deaths_rank(extra);
            let what = format!(
                "Deaths surged in {}: {} more than usual{}",
                plural(surged.len(), "place", "places"),
                thousands(extra.round() as i64),
                whereabouts(&surged, geo)
            );
            self.note(r, rank, "deaths", what, extra, death_details(&surged), listed);
        }
        if !region_surge {
            self.usual_deaths = Some(usual + (rate - usual) / 12.0);
        }

        // People leaving far more than usual.
        let left: f64 = m.left.iter().sum();
        let leaving = left / total.max(1.0);
        let usual_leaving = *self.usual_leaving.get_or_insert(leaving);
        if leaving >= cfg.leaving_surge * usual_leaving && left >= 10.0 * cfg.fewest_deaths {
            let rank = if leaving >= 0.01 { 1 } else { 2 };
            let what = format!(
                "{} people left their homes, {} times the usual",
                thousands(left.round() as i64),
                (leaving / usual_leaving.max(1e-12) * 10.0).round() / 10.0
            );
            let details = (0..n)
                .filter(|&v| m.left[v] >= 1.0)
                .map(|v| {
                    (
                        m.left[v],
                        Detail {
                            place: names[v].into(),
                            what: format!("{} left", thousands(m.left[v].round() as i64)),
                        },
                    )
                })
                .collect();
            self.note(r, rank, "flight", what, left, if n > 1 { details } else { Vec::new() }, listed);
        } else {
            self.usual_leaving = Some(usual_leaving + (leaving - usual_leaving) / 12.0);
        }

        // Grain grown dear, and back to its usual price.
        match self.grain.month(r.food_price, cfg.price_surge) {
            (Some(true), usual) => {
                let what = format!(
                    "Grain is dear: {:.2} a ration, {:.1} times its usual price",
                    r.food_price,
                    r.food_price / usual
                );
                self.note(r, 1, "dearth", what, total, Vec::new(), listed);
            }
            (Some(false), _) => {
                let what = format!("Grain is no longer dear: {:.2} a ration", r.food_price);
                self.note(r, 3, "dearth over", what, total, Vec::new(), listed);
            }
            (None, _) => {}
        }
        // Tools too (the smiths short of iron, or of hands): every farm feels
        // it; and the staples every family buys (salt).
        let tool = config.product_for("tool");
        let staples = (0..config.products.len()).filter(|&p| config.products[p].use_ == "staple");
        for p in std::iter::once(tool).chain(staples) {
            let name = &config.products[p].name;
            let Some(&(_, price)) = r.prices.iter().find(|(n, _)| n == name) else { continue };
            let k = match self.goods.iter().position(|(n, _)| n == name) {
                Some(k) => k,
                None => {
                    self.goods.push((name.clone(), Dearness::default()));
                    self.goods.len() - 1
                }
            };
            let (good, unit, its, kinds) = if p == tool {
                ("Tools are".to_string(), "a tool", "their", ("tools dear", "tools dear over"))
            } else {
                (format!("{} is", capitalised(name)), "a unit", "its", ("staple dear", "staple dear over"))
            };
            match self.goods[k].1.month(price, cfg.price_surge) {
                (Some(true), usual) => {
                    let what = format!("{good} dear: {price:.2} {unit}, {:.1} times {its} usual price", price / usual);
                    self.note(r, 2, kinds.0, what, total, Vec::new(), listed);
                }
                (Some(false), _) => {
                    let what = format!("{good} no longer dear: {price:.2} {unit}");
                    self.note(r, 3, kinds.1, what, total, Vec::new(), listed);
                }
                (None, _) => {}
            }
        }

        // The city hungry (its people die and leave first in a dearth).
        if let Some(city) = city {
            let hungry = m.ration[city] < 0.5 * (cfg.famine_ration + cfg.short_ration) && m.people[city] > 0.0;
            if hungry && !self.city_hungry {
                let what = format!("{} goes hungry: it ate {:.0}% of its need", names[city], 100.0 * m.ration[city]);
                self.note(r, 2, "city", what, m.people[city], Vec::new(), listed);
            }
            self.city_hungry = hungry;
        }

        // Councils formed, and places left empty.
        if !m.councils.is_empty() {
            let people: f64 = m.councils.iter().map(|&v| m.people[v]).sum();
            let what = if n == 1 {
                "The village has formed a council".to_string()
            } else {
                format!("Councils formed in {}", plural(m.councils.len(), "place", "places"))
            };
            let details =
                m.councils.iter().map(|&v| (m.people[v], Detail { place: names[v].into(), what: about(v) })).collect();
            // (in a region, one council among many; in a village, its government)
            let rank = if n == 1 { 2 } else { 4 };
            self.note(r, rank, "council", what, people, if n > 1 { details } else { Vec::new() }, listed);
        }
        let emptied: Vec<usize> = (0..n).filter(|&v| self.lived_in[v] && m.people[v] <= 0.0).collect();
        if !emptied.is_empty() {
            let what = if n == 1 || emptied.len() == n {
                "The last of the people have died or left".to_string()
            } else {
                format!("{} left empty", plural(emptied.len(), "place", "places"))
            };
            let rank = if emptied.len() == n { 1 } else { 3 };
            let details =
                emptied.iter().map(|&v| (1.0, Detail { place: names[v].into(), what: "empty".into() })).collect();
            self.note(
                r,
                rank,
                "emptied",
                what,
                0.0,
                if n > 1 && emptied.len() < n { details } else { Vec::new() },
                listed,
            );
        }
        for v in 0..n {
            self.lived_in[v] = m.people[v] > 0.0;
        }
    }

    /// The chronicle written out: year by year, the year in brief and then
    /// its months, each month's entries from rank 1 down to `most_rank`.
    pub fn text(&self, records: &[MonthRecord], most_rank: u8) -> String {
        let mut out = String::new();
        let mut years: Vec<i64> = records.iter().map(|r| r.year).collect();
        years.dedup();
        for year in years {
            let months: Vec<&MonthRecord> = records.iter().filter(|r| r.year == year).collect();
            let (first, last) = (months[0], months[months.len() - 1]);
            let start_people = first.population + first.deaths - first.births;
            let sum = |f: &dyn Fn(&MonthRecord) -> i64| months.iter().map(|r| f(r)).sum::<i64>();
            let lowest = months.iter().map(|r| r.food_price).fold(f64::INFINITY, f64::min);
            let highest = months.iter().map(|r| r.food_price).fold(f64::NEG_INFINITY, f64::max);
            let worst = months.iter().map(|r| r.ration).fold(f64::INFINITY, f64::min);
            out += &format!(
                "Year {year} ({} to {}): {} people to {} ({:+.1}%); {} born, {} died, {} left; grain {:.2} to {:.2}; worst month {:.0}% fed\n",
                &MONTH_NAME[first.month as usize][..3],
                &MONTH_NAME[last.month as usize][..3],
                thousands(start_people),
                thousands(last.population),
                100.0 * (last.population as f64 / start_people.max(1) as f64 - 1.0),
                thousands(sum(&|r| r.births)),
                thousands(sum(&|r| r.deaths)),
                thousands(sum(&|r| r.emigrants)),
                lowest,
                highest,
                100.0 * worst,
            );
            let entries: Vec<&Entry> = self.entries.iter().filter(|e| e.year == year).collect();
            let mut brief: Vec<&Entry> = entries.iter().copied().filter(|e| e.rank == 1).collect();
            if brief.is_empty() {
                brief = entries.iter().copied().filter(|e| e.rank == 2).collect();
                brief.sort_by(|a, b| b.people.total_cmp(&a.people));
                brief.truncate(3);
                brief.sort_by_key(|e| e.month_number);
            }
            if !brief.is_empty() {
                out += "  In brief:\n";
                for line in brief_lines(&brief) {
                    out += &format!("    {line}\n");
                }
            }
            for month in &months {
                let mut here: Vec<&Entry> = entries
                    .iter()
                    .copied()
                    .filter(|e| e.month_number == month.month_number && e.rank <= most_rank)
                    .collect();
                if here.is_empty() {
                    continue;
                }
                here.sort_by(|a, b| a.rank.cmp(&b.rank).then(b.people.total_cmp(&a.people)));
                out += &format!("  {}\n", MONTH_NAME[month.month as usize]);
                for e in here {
                    out += &format!("    [{}] {}\n", e.rank, e.what);
                    if !e.places.is_empty() {
                        let mut line: Vec<String> =
                            e.places.iter().map(|d| format!("{}: {}", d.place, d.what)).collect();
                        if e.more_places > 0 {
                            line.push(format!("and {} more", thousands(e.more_places as i64)));
                        }
                        out += &format!("          {}\n", line.join("; "));
                    }
                }
            }
        }
        out
    }
}

/// The year in brief, one line an entry, but a run of months of deaths,
/// famine or flight in one line.
fn brief_lines(brief: &[&Entry]) -> Vec<String> {
    let lasting = |kind: &str| matches!(kind, "deaths" | "famine" | "flight");
    let mut runs: Vec<Vec<&Entry>> = Vec::new();
    for &e in brief {
        match runs.iter_mut().rev().find(|run| run[0].kind == e.kind) {
            Some(run) if lasting(e.kind) && run.last().unwrap().month_number + 1 == e.month_number => run.push(e),
            _ => runs.push(vec![e]),
        }
    }
    runs.sort_by_key(|run| run[0].month_number);
    runs.iter()
        .map(|run| {
            let (first, last) = (run[0], run[run.len() - 1]);
            let when = &MONTH_NAME[first.month as usize][..3];
            if run.len() == 1 {
                return format!("[{}] {when}: {}", first.rank, first.what);
            }
            let until = format!("{when} to {}", &MONTH_NAME[last.month as usize][..3]);
            let what = match first.kind {
                "deaths" => format!(
                    "deaths ran above the usual for {} months, {} more in all",
                    run.len(),
                    thousands(run.iter().map(|e| e.people).sum::<f64>().round() as i64)
                ),
                "flight" => format!(
                    "people fled for {} months, {} in all",
                    run.len(),
                    thousands(run.iter().map(|e| e.people).sum::<f64>().round() as i64)
                ),
                _ => format!(
                    "famine for {} months, at worst {} people",
                    run.len(),
                    thousands(run.iter().map(|e| e.people).fold(0.0, f64::max).round() as i64)
                ),
            };
            format!("[{}] {until}: {what}", first.rank)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_good_grows_dear_at_the_surge_and_is_dear_until_halfway_back_to_its_usual_price() {
        let mut good = Dearness::default();
        let changes: Vec<Option<bool>> =
            [2.0, 2.0, 2.7, 2.9, 2.5, 2.3, 2.3].iter().map(|&p| good.month(p, 1.4).0).collect();
        assert_eq!(changes, [None, None, None, Some(true), None, Some(false), None]);
        // (the usual price stood still while the good was dear)
        let (_, usual) = good.month(2.0, 1.4);
        assert!(usual > 2.0 && usual < 2.1, "{usual}");
        assert_eq!(capitalised("salt"), "Salt");
    }
}
