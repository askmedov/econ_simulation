//! Families with minds of their own.
//!
//! The model as written has every family follow the same rules: keep this
//! many months of grain beyond the plan, that many months of coins, lend
//! half of what is spare. Here each family can instead have traits drawn
//! at the start, a memory of what happened to it, and expectations of the
//! grain price; how much grain it holds back, how many coins it keeps, how
//! much it lends and whether it leaves follow from them. (Settings:
//! `MindsConfig`, exported from Python, which doesn't use them.)
//!
//! Both kinds decide through one interface, `Deciding`. The rules are one
//! kind of mind (every family alike: the config's numbers) and the default;
//! `Minds` is the other. A setting switches with `minds.enabled`, and paired
//! runs compare the two. With no spread of traits and no responses, minds
//! decide exactly as the rules do (a test checks it, run for run).

use crate::config::MindsConfig;
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
    /// The share of its need eaten below which it flees a famine (rules:
    /// `migration.flee_below`).
    fn flees_below(&self, family: usize, usual: f64) -> f64;
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
    fn flees_below(&self, _: usize, usual: f64) -> f64 {
        usual
    }
}

/// Each family's traits, memory and expectations (one entry per household).
#[derive(Clone, Debug, PartialEq)]
pub struct Minds {
    settings: MindsConfig,
    // Traits: each multiplies a decision of the rules (1: as the rules).
    pub caution: Vec<f64>,    // grain held back; divides what it lends
    pub thrift: Vec<f64>,     // coins kept
    pub nerve: Vec<f64>,      // hunger it stands before leaving or fleeing
    pub learning: Vec<f64>,   // how fast its expectations follow what it sees
    pub attachment: Vec<f64>, // to its place: divides its chance of leaving
    pub patience: Vec<f64>,   // how far ahead it looks
    // Memory.
    pub hunger: Vec<f64>,     // the share of its need it went without, remembered
    pub losses: Vec<f64>,     // the share of its claims written off, remembered
    pub last_price: Vec<f64>, // the grain price it saw last month (0: none yet)
    // Expectations.
    pub trend: Vec<f64>, // the monthly change in the grain price it expects
}

impl Minds {
    /// Traits drawn for each of `n` families; nothing remembered yet.
    pub fn new(n: usize, settings: &MindsConfig, rng: &mut Rng) -> Minds {
        let mut minds = Minds {
            settings: settings.clone(),
            caution: Vec::with_capacity(n),
            thrift: Vec::with_capacity(n),
            nerve: Vec::with_capacity(n),
            learning: Vec::with_capacity(n),
            attachment: Vec::with_capacity(n),
            patience: Vec::with_capacity(n),
            hunger: Vec::with_capacity(n),
            losses: Vec::with_capacity(n),
            last_price: Vec::with_capacity(n),
            trend: Vec::with_capacity(n),
        };
        let spread = settings.trait_spread;
        for _ in 0..n {
            minds.push_blank();
            let f = minds.len() - 1;
            for one in minds.traits_mut() {
                one[f] = draw(rng, spread);
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

    fn traits_mut(&mut self) -> [&mut Vec<f64>; 6] {
        [
            &mut self.caution,
            &mut self.thrift,
            &mut self.nerve,
            &mut self.learning,
            &mut self.attachment,
            &mut self.patience,
        ]
    }

    fn all_mut(&mut self) -> [&mut Vec<f64>; 10] {
        [
            &mut self.caution,
            &mut self.thrift,
            &mut self.nerve,
            &mut self.learning,
            &mut self.attachment,
            &mut self.patience,
            &mut self.hunger,
            &mut self.losses,
            &mut self.last_price,
            &mut self.trend,
        ]
    }

    /// A new family, as the rules would have it decide, with nothing
    /// remembered (see `copy` and `inherit`).
    pub fn push_blank(&mut self) {
        for (k, column) in self.all_mut().into_iter().enumerate() {
            column.push(if k < 6 { 1.0 } else { 0.0 });
        }
    }

    /// A family moving to a new home takes its mind with it.
    pub fn copy(&mut self, from: usize, to: usize) {
        for column in self.all_mut() {
            column[to] = column[from];
        }
    }

    /// A family founded by a member of `parent` takes its traits, each a
    /// little changed, and what it remembers and expects.
    pub fn inherit(&mut self, child: usize, parent: usize, rng: &mut Rng) {
        self.copy(parent, child);
        self.vary(child, rng);
    }

    /// Change each of a family's traits a little (a new family's, from
    /// those it took from its parents).
    pub fn vary(&mut self, f: usize, rng: &mut Rng) {
        let spread = self.settings.inherited_spread;
        for one in self.traits_mut() {
            one[f] *= draw(rng, spread);
        }
    }

    /// The month's lessons: how much of its need each family ate (`ate`, a
    /// share) and the grain price it paid (`price`, per family).
    pub fn remember_month(&mut self, ate: &[f64], price: &[f64]) {
        let fade = 1.0 / self.settings.memory_months.max(1.0);
        let learning = self.settings.learning;
        for f in 0..self.len() {
            let went_without = (1.0 - ate[f]).clamp(0.0, 1.0);
            self.hunger[f] += fade * (went_without - self.hunger[f]);
            self.losses[f] *= 1.0 - fade;
            if self.last_price[f] > 0.0 && price[f] > 0.0 {
                let change = price[f] / self.last_price[f] - 1.0;
                let pace = (learning * self.learning[f]).clamp(0.0, 1.0);
                self.trend[f] += pace * (change - self.trend[f]);
            }
            if price[f] > 0.0 {
                self.last_price[f] = price[f];
            }
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

    /// How much dearer a family expects grain at the end of its horizon (a
    /// factor: 1 for no change).
    pub fn expected_rise(&self, f: usize) -> f64 {
        let months = self.settings.horizon_months * self.patience[f];
        (1.0 + self.trend[f]).max(1e-9).powf(months)
    }
}

/// A trait's draw: exp(normal(0, spread)), or 1 with no spread (and then no
/// draw, so a setting without spread leaves the stream untouched).
fn draw(rng: &mut Rng, spread: f64) -> f64 {
    if spread > 0.0 {
        rng.lognormal(0.0, spread)
    } else {
        1.0
    }
}

impl Deciding for Minds {
    fn grain_margin(&self, f: usize, usual: f64) -> f64 {
        let s = &self.settings;
        let limit = s.hoard_limit.max(1.0);
        let hoarding = if s.speculation == 0.0 {
            1.0
        } else {
            self.expected_rise(f).powf(s.speculation).clamp(1.0 / limit, limit)
        };
        usual * self.caution[f] * hoarding
    }

    fn savings_months(&self, f: usize, usual: f64) -> f64 {
        usual * self.thrift[f] * (1.0 + self.settings.hunger_saving * self.hunger[f])
    }

    fn lend_share(&self, f: usize, usual: f64) -> f64 {
        let fear = (1.0 - self.settings.loss_fear * self.losses[f]).max(0.0);
        (usual / self.caution[f] * fear).min(1.0)
    }

    fn leave_chance(&self, f: usize, usual: f64) -> f64 {
        let pushed = 1.0 + self.settings.hunger_leaving * self.hunger[f] / self.nerve[f];
        (usual / self.attachment[f] * pushed).min(1.0)
    }

    fn flees_below(&self, f: usize, usual: f64) -> f64 {
        usual / self.nerve[f]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::default_config;

    fn settings() -> MindsConfig {
        default_config().minds
    }

    #[test]
    fn a_family_that_has_seen_grain_get_dearer_holds_more_back() {
        let mut minds = Minds::new(2, &MindsConfig { trait_spread: 0.0, ..settings() }, &mut Rng::named(1, "minds"));
        for month in 0..6 {
            let rising = 1.0 + 0.1 * month as f64;
            minds.remember_month(&[1.0, 1.0], &[rising, 1.0]);
        }
        assert!(minds.trend[0] > 0.05 && minds.trend[1] == 0.0);
        assert!(minds.grain_margin(0, 1.0) > 1.3);
        assert_eq!(minds.grain_margin(1, 1.0), 1.0);
    }

    #[test]
    fn hunger_and_losses_are_remembered_and_fade() {
        let mut minds = Minds::new(1, &MindsConfig { trait_spread: 0.0, ..settings() }, &mut Rng::named(1, "minds"));
        minds.remember_month(&[0.5], &[1.0]);
        minds.remember_losses(&[100.0], &[40.0]);
        let (hunger, losses) = (minds.hunger[0], minds.losses[0]);
        assert!(hunger > 0.0 && (losses - 0.6).abs() < 1e-12);
        assert!(minds.savings_months(0, 3.0) > 3.0);
        assert!(minds.lend_share(0, 0.5) < 0.5);
        for _ in 0..60 {
            minds.remember_month(&[1.0], &[1.0]);
        }
        assert!(minds.hunger[0] < hunger / 10.0 && minds.losses[0] < losses / 10.0);
    }

    #[test]
    fn a_founded_family_takes_its_parents_mind_a_little_changed() {
        let mut minds = Minds::new(1, &settings(), &mut Rng::named(1, "minds"));
        minds.hunger[0] = 0.3;
        minds.push_blank();
        minds.inherit(1, 0, &mut Rng::named(2, "minds"));
        assert_eq!(minds.hunger[1], 0.3);
        let ratio = minds.caution[1] / minds.caution[0];
        assert!(ratio != 1.0 && (0.5..2.0).contains(&ratio));
    }

    #[test]
    fn traits_are_spread_around_the_rules() {
        let minds = Minds::new(4000, &settings(), &mut Rng::named(1, "minds"));
        let mean_log = minds.caution.iter().map(|c| c.ln()).sum::<f64>() / 4000.0;
        let sd_log = (minds.caution.iter().map(|c| (c.ln() - mean_log).powi(2)).sum::<f64>() / 4000.0).sqrt();
        assert!(mean_log.abs() < 0.03 && (sd_log - settings().trait_spread).abs() < 0.03);
    }
}
