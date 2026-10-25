//! Families with minds of their own (on by default).
//!
//! The model as written (and the Python model) has every family follow the
//! same rules: keep this many months of grain beyond the plan, that many
//! months of coins, lend half of what is spare. Here each family has traits
//! drawn at the start, a memory of what happened to it, and expectations of
//! the grain price; how much grain it holds back, how many coins it keeps,
//! how much it lends and whether it leaves follow from them. (Settings:
//! `MindsConfig`, exported from Python, which doesn't use them.)
//!
//! Minds are made to be steady. The typical family decides as the rules
//! do: each trait's factor averages 1 over families, so what minds add is
//! variety, not a lean. Hunger pushes a family to leave only beyond its
//! neighbours' (the place's own hunger already counts, in how good a place
//! it is). Expected prices are anchored to the price a family thinks
//! normal and kept within bounds. How much grain a family holds back and
//! how much it lends change by habit, a share of the way each month.
//!
//! Both kinds decide through one interface, `Deciding`: `Minds`, and the
//! rules (every family alike, the config's numbers; minds switched off).
//! With no spread of traits and no responses, minds decide exactly as the
//! rules do (a test checks it, run for run).

use crate::config::MindsConfig;
use crate::geography::normal_share;
use crate::rng::Rng;

/// How a family decides, given what the rules would have it do.
pub trait Deciding {
    /// Months of its need it holds back beyond what it plans to eat
    /// (rules: `land.keep_months`).
    fn grain_margin(&self, family: usize, usual: f64) -> f64;
    /// Months of food and firewood costs it keeps in coins (rules:
    /// `needs.savings_months`).
    fn savings_months(&self, family: usize, usual: f64) -> f64;
    /// Share of its spare coins it lends (rules: `credit.lend_share`).
    fn lend_share(&self, family: usize, usual: f64) -> f64;
    /// Its yearly chance of leaving (rules: by its place and its land).
    fn leave_chance(&self, family: usize, usual: f64) -> f64;
    /// Its monthly chance of fleeing a famine, once it eats less than
    /// `migration.flee_below` of its need (rules: `migration.flee_chance`).
    fn flee_chance(&self, family: usize, usual: f64) -> f64;
}

/// The ported rules: every family does what the config says.
pub struct Rules;

impl Deciding for Rules {
    fn grain_margin(&self, _: usize, usual: f64) -> f64 {
        usual
    }
    fn savings_months(&self, _: usize, usual: f64) -> f64 {
        usual
    }
    fn lend_share(&self, _: usize, usual: f64) -> f64 {
        usual
    }
    fn leave_chance(&self, _: usize, usual: f64) -> f64 {
        usual
    }
    fn flee_chance(&self, _: usize, usual: f64) -> f64 {
        usual
    }
}

/// Each family's traits, memory, expectations and habits (one entry per
/// household).
#[derive(Clone, Debug, PartialEq)]
pub struct Minds {
    settings: MindsConfig,
    /// Scales a trait's factor so that its average over families is 1.
    typical: f64,
    // Traits, in standard deviations from the typical family (0).
    pub caution: Vec<f64>,    // grain held back; divides what it lends
    pub thrift: Vec<f64>,     // coins kept
    pub nerve: Vec<f64>,      // hunger it stands before leaving or fleeing
    pub learning: Vec<f64>,   // how fast its expectations follow what it sees
    pub attachment: Vec<f64>, // to its place: divides its chance of leaving
    pub patience: Vec<f64>,   // how far ahead it looks
    // Memory.
    pub hunger: Vec<f64>,       // the share of its need it went without, remembered
    pub hungrier: Vec<f64>,     // its remembered hunger less its neighbours'
    pub losses: Vec<f64>,       // the share of its claims written off, remembered
    pub last_price: Vec<f64>,   // the grain price it paid last month (0: none yet)
    pub normal_price: Vec<f64>, // the price it thinks normal (0: none yet)
    // Expectations.
    pub trend: Vec<f64>, // the monthly change in the grain price it expects
    // Habits: its decisions against the rules' (1: as the rules), which move
    // toward what it wants a share of the way each month.
    pub hold: Vec<f64>,    // grain held back beyond its plan
    pub lending: Vec<f64>, // share of spare coins lent
    /// Its chance of leaving against its place's families' on average:
    /// hunger beyond its neighbours' and a weak attachment make it likelier
    /// to be one of those who go, not more to go.
    pub leaving: Vec<f64>,
    /// Its traits' factors on the decisions they raise or lower (see
    /// `Factor`), worked out when its traits are set rather than each month.
    factors: Vec<[f64; FACTORS]>,
}

/// The factors a family's traits put on its decisions.
#[derive(Clone, Copy)]
enum Factor {
    Cautious,   // grain held back
    Uncautious, // share lent
    Thrifty,    // coins kept
    Nervy,      // divides the push of hunger
    Unnervy,    // chance of fleeing
    Learning,   // how fast expectations follow prices
    Unattached, // chance of leaving
    Patient,    // horizon
}

const FACTORS: usize = 8;

const TRAITS: usize = 6;

impl Minds {
    /// Traits drawn for each of `n` families; nothing remembered yet.
    pub fn new(n: usize, settings: &MindsConfig, rng: &mut Rng) -> Minds {
        let mut minds = Minds {
            settings: settings.clone(),
            typical: typical(settings.trait_spread, settings.trait_limit),
            caution: Vec::with_capacity(n),
            thrift: Vec::with_capacity(n),
            nerve: Vec::with_capacity(n),
            learning: Vec::with_capacity(n),
            attachment: Vec::with_capacity(n),
            patience: Vec::with_capacity(n),
            hunger: Vec::with_capacity(n),
            hungrier: Vec::with_capacity(n),
            losses: Vec::with_capacity(n),
            last_price: Vec::with_capacity(n),
            normal_price: Vec::with_capacity(n),
            trend: Vec::with_capacity(n),
            hold: Vec::with_capacity(n),
            lending: Vec::with_capacity(n),
            leaving: Vec::with_capacity(n),
            factors: Vec::with_capacity(n),
        };
        let spread = settings.trait_spread;
        let limit = settings.trait_limit;
        for _ in 0..n {
            minds.push_blank();
            let f = minds.len() - 1;
            if spread > 0.0 {
                for one in minds.traits_mut() {
                    one[f] = rng.normal(0.0, 1.0).clamp(-limit, limit);
                }
                minds.set_factors(f);
            }
        }
        minds
    }

    pub fn len(&self) -> usize {
        self.caution.len()
    }

    pub fn is_empty(&self) -> bool {
        self.caution.is_empty()
    }

    fn traits_mut(&mut self) -> [&mut Vec<f64>; TRAITS] {
        [
            &mut self.caution,
            &mut self.thrift,
            &mut self.nerve,
            &mut self.learning,
            &mut self.attachment,
            &mut self.patience,
        ]
    }

    /// Every column: the traits, the memory and expectations, then the habits.
    fn all_mut(&mut self) -> [&mut Vec<f64>; 15] {
        [
            &mut self.caution,
            &mut self.thrift,
            &mut self.nerve,
            &mut self.learning,
            &mut self.attachment,
            &mut self.patience,
            &mut self.hunger,
            &mut self.hungrier,
            &mut self.losses,
            &mut self.last_price,
            &mut self.normal_price,
            &mut self.trend,
            &mut self.hold,
            &mut self.lending,
            &mut self.leaving,
        ]
    }

    /// A new family: typical traits, nothing remembered, deciding as the
    /// rules do (see `copy` and `inherit`).
    pub fn push_blank(&mut self) {
        for (k, column) in self.all_mut().into_iter().enumerate() {
            column.push(if k >= 12 { 1.0 } else { 0.0 }); // (the habits and leaving: as the rules)
        }
        self.factors.push([0.0; FACTORS]);
        self.set_factors(self.len() - 1);
    }

    fn set_factors(&mut self, f: usize) {
        use Factor::*;
        let mut factors = [0.0; FACTORS];
        factors[Cautious as usize] = self.up(self.caution[f]);
        factors[Uncautious as usize] = self.down(self.caution[f]);
        factors[Thrifty as usize] = self.up(self.thrift[f]);
        factors[Nervy as usize] = self.up(self.nerve[f]);
        factors[Unnervy as usize] = self.down(self.nerve[f]);
        factors[Learning as usize] = self.up(self.learning[f]);
        factors[Unattached as usize] = self.down(self.attachment[f]);
        factors[Patient as usize] = self.up(self.patience[f]);
        self.factors[f] = factors;
    }

    #[inline]
    fn factor(&self, f: usize, which: Factor) -> f64 {
        self.factors[f][which as usize]
    }

    /// A family moving to a new home takes its mind with it.
    pub fn copy(&mut self, from: usize, to: usize) {
        for column in self.all_mut() {
            column[to] = column[from];
        }
        self.factors[to] = self.factors[from];
    }

    /// A family founded by a member of `parent` takes its traits, each a
    /// little changed, and what it remembers, expects and is used to.
    pub fn inherit(&mut self, child: usize, parent: usize, rng: &mut Rng) {
        self.copy(parent, child);
        self.vary(child, rng);
    }

    /// Change each of a family's traits a little (a new family's, from
    /// those it took from its parents).
    pub fn vary(&mut self, f: usize, rng: &mut Rng) {
        let s = &self.settings;
        if s.inherited_spread <= 0.0 || s.trait_spread <= 0.0 {
            return;
        }
        let (step, limit) = (s.inherited_spread / s.trait_spread, s.trait_limit);
        for one in self.traits_mut() {
            one[f] = (one[f] + rng.normal(0.0, step)).clamp(-limit, limit);
        }
        self.set_factors(f);
    }

    /// A trait's factor on a decision it raises (`up`) or lowers (`down`);
    /// each averages 1 over families.
    fn up(&self, z: f64) -> f64 {
        (self.settings.trait_spread * z).exp() * self.typical
    }

    fn down(&self, z: f64) -> f64 {
        (-self.settings.trait_spread * z).exp() * self.typical
    }

    /// The month's lessons. `ate` is the share of its need each family ate,
    /// `price` the grain price it paid, `location` and `size` where it lives
    /// and how many it is (for its neighbours' hunger).
    pub fn remember_month(&mut self, ate: &[f64], price: &[f64], location: &[u32], size: &[f64]) {
        let s = self.settings.clone();
        let fade = 1.0 / s.memory_months.max(1.0);
        let settle = 1.0 / s.normal_months.max(1.0);
        for f in 0..self.len() {
            let went_without = (1.0 - ate[f]).clamp(0.0, 1.0);
            self.hunger[f] += fade * (went_without - self.hunger[f]);
            self.losses[f] *= 1.0 - fade;
            if self.last_price[f] > 0.0 && price[f] > 0.0 {
                let change = price[f] / self.last_price[f] - 1.0;
                let pace = (s.learning * self.factor(f, Factor::Learning)).clamp(0.0, 1.0);
                self.trend[f] += pace * (change - self.trend[f]);
            }
            if price[f] > 0.0 {
                self.last_price[f] = price[f];
                self.normal_price[f] = if self.normal_price[f] > 0.0 {
                    self.normal_price[f] + settle * (price[f] - self.normal_price[f])
                } else {
                    price[f]
                };
            }
        }
        // Neighbours: the place's living families.
        let places = location.iter().map(|&l| l as usize + 1).max().unwrap_or(0);
        let living = |f: usize| if size[f] > 0.0 { 1.0 } else { 0.0 };
        let (mut hungry, mut families) = (vec![0.0; places], vec![0.0; places]);
        for f in 0..self.len() {
            hungry[location[f] as usize] += living(f) * self.hunger[f];
            families[location[f] as usize] += living(f);
        }
        for f in 0..self.len() {
            let v = location[f] as usize;
            let usual = if families[v] > 0.0 { hungry[v] / families[v] } else { 0.0 };
            self.hungrier[f] = self.hunger[f] - usual;
        }
        // Who among them is likelier to leave, against the place's average.
        let push: Vec<f64> = (0..self.len())
            .map(|f| {
                let hungrier = (s.hunger_leaving * self.hungrier[f] / self.factor(f, Factor::Nervy)).clamp(-2.0, 2.0);
                self.factor(f, Factor::Unattached) * hungrier.exp()
            })
            .collect();
        let mut pushed = vec![0.0; places];
        for f in 0..self.len() {
            pushed[location[f] as usize] += living(f) * push[f];
        }
        for f in 0..self.len() {
            let v = location[f] as usize;
            self.leaving[f] = if pushed[v] > 0.0 { push[f] * families[v] / pushed[v] } else { 1.0 };
        }
        // Habits move toward what each family now wants.
        for f in 0..self.len() {
            let hold = self.factor(f, Factor::Cautious) * (s.speculation * self.expected_rise(f)).exp();
            self.hold[f] += s.inertia * (hold - self.hold[f]);
            let fear = (1.0 - s.loss_fear * self.losses[f]).max(s.least_lending);
            let lending = self.factor(f, Factor::Uncautious) * fear;
            self.lending[f] += s.inertia * (lending - self.lending[f]);
        }
    }

    /// Lenders remember the share of their claims written off: `before` and
    /// `after` are what each family was owed.
    pub fn remember_losses(&mut self, before: &[f64], after: &[f64]) {
        for f in 0..self.len().min(before.len()) {
            if before[f] > 0.0 && after[f] < before[f] {
                self.losses[f] = (self.losses[f] + (before[f] - after[f]) / before[f]).min(1.0);
            }
        }
    }

    /// How much dearer a family expects grain at the end of its horizon, as
    /// a log ratio (0: no change): the trend it has seen carried forward,
    /// pulled back toward the price it thinks normal, within bounds.
    pub fn expected_rise(&self, f: usize) -> f64 {
        let s = &self.settings;
        if self.last_price[f] <= 0.0 || self.normal_price[f] <= 0.0 {
            return 0.0;
        }
        let months = s.horizon_months * self.factor(f, Factor::Patient);
        let carried = months * (1.0 + self.trend[f]).max(1e-9).ln();
        let pulled = s.reversion * (self.last_price[f] / self.normal_price[f]).ln();
        let limit = s.expectation_limit.clamp(0.0, 0.99);
        (carried - pulled).clamp((1.0 - limit).ln(), (1.0 + limit).ln())
    }
}

/// 1 over the average of exp(spread x z) for z standard normal within
/// +-limit: what makes a trait's factor average 1 over families.
fn typical(spread: f64, limit: f64) -> f64 {
    if spread <= 0.0 {
        return 1.0;
    }
    let inside = normal_share(limit) - normal_share(-limit);
    let raised = (spread * spread / 2.0).exp() * (normal_share(limit - spread) - normal_share(-limit - spread));
    inside / raised
}

impl Deciding for Minds {
    fn grain_margin(&self, f: usize, usual: f64) -> f64 {
        usual * self.hold[f]
    }

    fn savings_months(&self, f: usize, usual: f64) -> f64 {
        usual * self.factor(f, Factor::Thrifty) * (1.0 + self.settings.hunger_saving * self.hunger[f])
    }

    fn lend_share(&self, f: usize, usual: f64) -> f64 {
        (usual * self.lending[f]).min(1.0)
    }

    fn leave_chance(&self, f: usize, usual: f64) -> f64 {
        (usual * self.leaving[f]).min(1.0)
    }

    fn flee_chance(&self, f: usize, usual: f64) -> f64 {
        (usual * self.factor(f, Factor::Unnervy)).min(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::default_config;

    fn settings() -> MindsConfig {
        default_config().minds
    }

    fn one_place(n: usize) -> (Vec<u32>, Vec<f64>) {
        (vec![0; n], vec![4.0; n])
    }

    #[test]
    fn the_typical_family_decides_as_the_rules_do_on_average() {
        let minds = Minds::new(20000, &settings(), &mut Rng::named(1, "minds"));
        let mean = |f: &dyn Fn(usize) -> f64| (0..20000).map(f).sum::<f64>() / 20000.0;
        assert!((mean(&|f| minds.up(minds.caution[f])) - 1.0).abs() < 0.01);
        assert!((mean(&|f| minds.down(minds.attachment[f])) - 1.0).abs() < 0.01);
        assert!((mean(&|f| minds.savings_months(f, 1.0)) - 1.0).abs() < 0.01);
        assert!(minds.caution.iter().all(|z| z.abs() <= settings().trait_limit));
    }

    #[test]
    fn a_family_that_has_seen_grain_get_dearer_holds_more_back_but_not_without_bound() {
        // (without the pull back toward the normal price, which a steady
        // rise soon meets: see the next test)
        let neutral = MindsConfig { trait_spread: 0.0, reversion: 0.0, ..settings() };
        let mut minds = Minds::new(2, &neutral, &mut Rng::named(1, "minds"));
        let (location, size) = one_place(2);
        for _ in 0..24 {
            minds.remember_month(&[1.0, 1.0], &[1.0, 1.0], &location, &size);
        }
        // Early in a rise: the trend is up, the price not yet far above normal.
        for month in 1..=4 {
            let rising = 1.0 + 0.05 * month as f64;
            minds.remember_month(&[1.0, 1.0], &[rising, 1.0], &location, &size);
        }
        assert!(minds.trend[0] > 0.02 && minds.trend[1] == 0.0);
        assert!(minds.grain_margin(0, 1.0) > 1.01);
        assert_eq!(minds.grain_margin(1, 1.0), 1.0);
        // However steep the rise, never beyond the bound.
        for month in 1..=24 {
            minds.remember_month(&[1.0, 1.0], &[1.5f64.powi(month), 1.0], &location, &size);
        }
        assert!(minds.grain_margin(0, 1.0) <= 1.0 + neutral.expectation_limit + 1e-9);
    }

    #[test]
    fn a_family_expects_a_price_far_above_normal_to_fall_back() {
        let neutral = MindsConfig { trait_spread: 0.0, ..settings() };
        let mut minds = Minds::new(1, &neutral, &mut Rng::named(1, "minds"));
        let (location, size) = one_place(1);
        for _ in 0..24 {
            minds.remember_month(&[1.0], &[1.0], &location, &size);
        }
        // A sudden doubling, then the price holds for a year: once the jump
        // has faded from its trend, it expects the price to ease.
        minds.remember_month(&[1.0], &[2.0], &location, &size);
        for _ in 0..12 {
            minds.remember_month(&[1.0], &[2.0], &location, &size);
        }
        assert!(minds.expected_rise(0) < 0.0);
    }

    #[test]
    fn habits_change_a_share_of_the_way_each_month() {
        let neutral = MindsConfig { trait_spread: 0.0, ..settings() };
        let mut minds = Minds::new(1, &neutral, &mut Rng::named(1, "minds"));
        let (location, size) = one_place(1);
        minds.remember_month(&[1.0], &[1.0], &location, &size);
        minds.remember_losses(&[100.0], &[0.0]);
        minds.remember_month(&[1.0], &[1.0], &location, &size);
        let want = (1.0 - neutral.loss_fear * minds.losses[0]).max(neutral.least_lending);
        assert!((minds.lending[0] - (1.0 + neutral.inertia * (want - 1.0))).abs() < 1e-12);
        for _ in 0..3 {
            minds.remember_losses(&[100.0], &[0.0]);
            minds.remember_month(&[1.0], &[1.0], &location, &size);
        }
        assert!(minds.lend_share(0, 0.5) >= 0.5 * neutral.least_lending - 1e-12);
    }

    #[test]
    fn hunger_pushes_a_family_away_only_beyond_its_neighbours() {
        let neutral = MindsConfig { trait_spread: 0.0, ..settings() };
        let mut minds = Minds::new(3, &neutral, &mut Rng::named(1, "minds"));
        let location = vec![0, 0, 1];
        let size = vec![4.0; 3];
        for _ in 0..6 {
            minds.remember_month(&[0.6, 1.0, 0.8], &[1.0; 3], &location, &size);
        }
        assert!(minds.leave_chance(0, 0.01) > 0.01); // hungrier than its neighbour
        assert!(minds.leave_chance(1, 0.01) < 0.01); // better off
        assert!((minds.leave_chance(2, 0.01) - 0.01).abs() < 1e-12); // as hungry as its place
                                                                     // As many leave from the place on average as by the rules.
        assert!((minds.leave_chance(0, 0.01) + minds.leave_chance(1, 0.01) - 0.02).abs() < 1e-12);
    }

    #[test]
    fn a_founded_family_takes_its_parents_mind_a_little_changed() {
        let mut minds = Minds::new(1, &settings(), &mut Rng::named(1, "minds"));
        minds.hunger[0] = 0.3;
        minds.push_blank();
        minds.inherit(1, 0, &mut Rng::named(2, "minds"));
        assert_eq!(minds.hunger[1], 0.3);
        let moved = (minds.caution[1] - minds.caution[0]).abs();
        assert!(moved > 0.0 && moved < 2.0);
    }
}
