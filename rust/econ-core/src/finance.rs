//! Moneylenders in the market towns, and an interest rate that moves
//! (Python's `FinanceConfig`; Rust only).
//!
//! A village's own lenders are its families with coins to spare (see
//! `credit`). In a bad year everyone there is short at once, and their
//! spare coins are gone when they are most wanted. A town's merchants lend
//! too: to the families of the town and of the villages that market there,
//! for what no neighbour will lend. They start with part of the coins the
//! town's richest families hold, and they hold claims on each place's
//! borrowers, so every repayment, loss and decree there is shared between
//! the neighbours who lent and the town's lenders by their claims. They
//! lend on bonds, not on land: what they can't recover is written off.
//! They live on their profits, paying townsfolk.
//!
//! Each place's interest rate follows the borrowing asked there against
//! the coins on offer to lend there (its families' and its town lenders'),
//! both averaged over `rate_months`: the usual rate when they match,
//! higher when borrowers ask for more, lower when coins go begging.

use crate::config::Config;
use crate::geography::Geography;
use crate::grid::div_or;
use crate::households::Households;
use crate::market::by_village;

/// The towns' lenders, and what each place asks of them and is offered.
#[derive(Clone, Debug, Default)]
pub struct Lenders {
    /// Per place: the town whose lenders lend there (-1: none).
    pub town: Vec<i64>,
    /// Per place: the purse of a town's lenders (nothing elsewhere).
    pub cash: Vec<f64>,
    /// Per place: what a town's lenders started with.
    pub capital: Vec<f64>,
    /// Per place: what its borrowers owe its town's lenders.
    pub claims: Vec<f64>,
    /// Per place: borrowing asked a month, averaged over `rate_months`.
    pub asked: Vec<f64>,
    /// Per place: coins on offer to lend a month, averaged likewise.
    pub offered: Vec<f64>,
    /// This month's coins lent, repaid to them, lost by them (written off or
    /// cancelled) and spent by them, over all towns.
    pub flows: Flows,
}

/// A month's coins through the towns' lenders.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Flows {
    pub lent: f64,
    pub repaid: f64,
    pub lost: f64,
    pub spent: f64,
}

impl Lenders {
    /// No town lenders (and no averages yet).
    pub fn none(n: usize) -> Lenders {
        Lenders {
            town: vec![-1; n],
            cash: vec![0.0; n],
            capital: vec![0.0; n],
            claims: vec![0.0; n],
            asked: vec![0.0; n],
            offered: vec![0.0; n],
            flows: Flows::default(),
        }
    }

    /// The lenders at the start: in each market town, the richest fifth of
    /// its families (by their coins, at least one) put `capital_share` of
    /// their coins into lending. Without towns, or with finance off, none.
    pub fn start(households: &mut Households, geo: &Geography, config: &Config) -> Lenders {
        let n = geo.x.len();
        let mut lenders = Lenders::none(n);
        if !config.finance.enabled || !geo.has_towns() {
            return lenders;
        }
        for v in 0..n {
            let m = geo.market[v];
            if m >= 0 && geo.town[m as usize] {
                lenders.town[v] = m;
            }
        }
        let mut families: Vec<Vec<usize>> = vec![Vec::new(); n];
        for h in 0..households.len() {
            let v = households.location[h] as usize;
            if geo.town[v] && households.money[h] > 0.0 {
                families[v].push(h);
            }
        }
        for (t, list) in families.iter_mut().enumerate() {
            list.sort_by(|&a, &b| households.money[b].total_cmp(&households.money[a]));
            for &h in list.iter().take(list.len().div_ceil(5)) {
                let put = config.finance.capital_share * households.money[h];
                households.money[h] -= put;
                lenders.cash[t] += put;
            }
            lenders.capital[t] = lenders.cash[t];
        }
        lenders
    }

    /// Whether any place has town lenders.
    pub fn any(&self) -> bool {
        self.town.iter().any(|&t| t >= 0)
    }

    /// Each place's share of its debts owed to its own families (by their
    /// `local` claims), the rest to its town's lenders: 1 without them.
    pub fn local_share(&self, local: &[f64]) -> Vec<f64> {
        (0..local.len())
            .map(|v| if self.claims[v] > 0.0 { div_or(local[v], local[v] + self.claims[v], 1.0) } else { 1.0 })
            .collect()
    }

    /// What each town's lenders may lend in a month: `lend_share` of their purse.
    fn budget(&self, config: &Config) -> Vec<f64> {
        self.cash.iter().map(|c| config.finance.lend_share * c.max(0.0)).collect()
    }

    /// What the town's lenders offer each place this month: the borrowing
    /// its neighbours can't meet (`unmet`), shared among the places of the
    /// town by it as far as their budget goes.
    pub fn offers(&self, unmet: &[f64], config: &Config) -> Vec<f64> {
        let n = unmet.len();
        let budget = self.budget(config);
        let mut wanted = vec![0.0; n];
        for v in 0..n {
            if self.town[v] >= 0 {
                wanted[self.town[v] as usize] += unmet[v].max(0.0);
            }
        }
        (0..n)
            .map(|v| {
                let t = self.town[v];
                if t < 0 || unmet[v] <= 0.0 {
                    return 0.0;
                }
                let t = t as usize;
                unmet[v].min(budget[t] * unmet[v] / wanted[t])
            })
            .collect()
    }

    /// The coins the town's lenders could lend each place in a month (their
    /// budget shared by `people`), for the interest rate.
    pub fn could_lend(&self, people: &[f64], config: &Config) -> Vec<f64> {
        let n = people.len();
        let budget = self.budget(config);
        let mut served = vec![0.0; n];
        for v in 0..n {
            if self.town[v] >= 0 {
                served[self.town[v] as usize] += people[v];
            }
        }
        (0..n)
            .map(|v| match self.town[v] {
                t if t >= 0 => div_or(budget[t as usize] * people[v], served[t as usize], 0.0),
                _ => 0.0,
            })
            .collect()
    }

    /// Coins lent to each place's borrowers: out of the town's purse, into
    /// its claims there.
    pub fn lend(&mut self, given: &[f64]) {
        for (v, &g) in given.iter().enumerate() {
            if g > 0.0 {
                self.cash[self.town[v] as usize] -= g;
                self.claims[v] += g;
                self.flows.lent += g;
            }
        }
    }

    /// Coins repaid to the town's lenders by each place's borrowers.
    pub fn receive(&mut self, paid: &[f64]) {
        for (v, &p) in paid.iter().enumerate() {
            if p > 0.0 {
                self.cash[self.town[v] as usize] += p;
                self.claims[v] = (self.claims[v] - p).max(0.0);
                self.flows.repaid += p;
            }
        }
    }

    /// The town's lenders' share (by their claims against the families'
    /// `local` ones) of each place's `total` repaid or lost.
    pub fn their_part(&self, total: &[f64], local: &[f64]) -> Vec<f64> {
        let share = self.local_share(local);
        (0..total.len()).map(|v| total[v] * (1.0 - share[v])).collect()
    }

    /// Debts lost (written off, cancelled) from their claims.
    pub fn lose(&mut self, lost: &[f64]) {
        for (c, l) in self.claims.iter_mut().zip(lost) {
            self.flows.lost += l.min(*c).max(0.0);
            *c = (*c - l).max(0.0);
        }
    }

    /// The lenders live on their profits: each month they spend
    /// `spend_share` of what they own beyond their starting capital (their
    /// purse and their claims) in their town, as far as their purse goes,
    /// paying its families by their members. Returns coins each household
    /// earned, and the coins spent per town.
    pub fn spend(&mut self, location: &[u32], members: &[f64], config: &Config) -> (Vec<f64>, Vec<f64>) {
        let n = self.cash.len();
        let mut owned = self.cash.clone();
        for v in 0..n {
            if self.town[v] >= 0 {
                owned[self.town[v] as usize] += self.claims[v];
            }
        }
        let people = by_village(members, location, n);
        let spent: Vec<f64> = (0..n)
            .map(|t| {
                let beyond = (owned[t] - self.capital[t]).max(0.0);
                if people[t] > 0.0 {
                    (config.finance.spend_share * beyond).min(self.cash[t].max(0.0))
                } else {
                    0.0
                }
            })
            .collect();
        for (c, s) in self.cash.iter_mut().zip(&spent) {
            *c -= s;
        }
        self.flows.spent += spent.iter().sum::<f64>();
        let earned = (0..members.len())
            .map(|h| {
                let v = location[h] as usize;
                div_or(spent[v] * members[h], people[v], 0.0)
            })
            .collect();
        (earned, spent)
    }

    /// This month's borrowing `asked` and coins on `offer` in each place
    /// move the averages, and the averages set each place's yearly
    /// `interest` rate: `credit.interest` times (asked over offered, over
    /// `usual_share`) to the power `rate_response`, between `lowest_rate`
    /// and `highest_rate`.
    pub fn move_rates(&mut self, interest: &mut [f64], asked: &[f64], offer: &[f64], config: &Config) {
        let cfg = &config.finance;
        let keep = 1.0 - 1.0 / cfg.rate_months.max(1.0);
        for v in 0..interest.len() {
            self.asked[v] = keep * self.asked[v] + (1.0 - keep) * asked[v].max(0.0);
            self.offered[v] = keep * self.offered[v] + (1.0 - keep) * offer[v].max(0.0);
            let ratio = if self.asked[v] <= 0.0 {
                0.0
            } else if self.offered[v] <= 0.0 {
                f64::INFINITY
            } else {
                self.asked[v] / self.offered[v] / cfg.usual_share
            };
            let rate = config.credit.interest * ratio.powf(cfg.rate_response);
            interest[v] = rate.clamp(cfg.lowest_rate, cfg.highest_rate);
        }
    }
}

/// How much more of their spare coins families lend at each place's
/// interest rate than at the usual one: (rate over usual) to the power
/// `supply_response`.
pub fn willing(interest: &[f64], config: &Config) -> Vec<f64> {
    interest.iter().map(|r| div_or(*r, config.credit.interest, 1.0).powf(config.finance.supply_response)).collect()
}
