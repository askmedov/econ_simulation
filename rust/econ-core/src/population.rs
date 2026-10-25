//! The population table.
//!
//! People are stored as columns (one vector per attribute), not one struct
//! per person, as in Python. Each row stands for `count` identical people.
//! Today every row has count 1, but every rule is written against counts,
//! so the same code can later run a country where one row is thousands of
//! similar people.

use serde::{Deserialize, Serialize};

/// The job of people who are not working (children, the old).
pub const NO_JOB: i16 = -1;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Population {
    pub count: Vec<u32>,      // people this row stands for
    pub age_months: Vec<i32>, //
    pub female: Vec<bool>,    //
    pub health: Vec<f64>,     // 0-100, group average
    pub skill: Vec<f64>,      // work productivity multiplier, group average
    pub location: Vec<u32>,   // place index
    pub household: Vec<u32>,  // family the row's people belong to
    pub job: Vec<i16>,        // business they work in (council jobs past them), or NO_JOB
    pub married: Vec<bool>,   // married (or widowed): no longer looking for a spouse
    pub couple: Vec<i32>,     // id shared with their spouse; -1 if never had one here
}

impl Population {
    pub fn len(&self) -> usize {
        self.count.len()
    }

    pub fn is_empty(&self) -> bool {
        self.count.is_empty()
    }

    /// Number of people (not rows).
    pub fn size(&self) -> u64 {
        self.count.iter().map(|&c| c as u64).sum()
    }

    #[inline]
    pub fn age_years(&self, row: usize) -> i32 {
        self.age_months[row] / 12
    }

    /// Add `other`'s rows at the end.
    pub fn append(&mut self, other: &Population) {
        self.count.extend_from_slice(&other.count);
        self.age_months.extend_from_slice(&other.age_months);
        self.female.extend_from_slice(&other.female);
        self.health.extend_from_slice(&other.health);
        self.skill.extend_from_slice(&other.skill);
        self.location.extend_from_slice(&other.location);
        self.household.extend_from_slice(&other.household);
        self.job.extend_from_slice(&other.job);
        self.married.extend_from_slice(&other.married);
        self.couple.extend_from_slice(&other.couple);
    }

    /// Add a copy of row `row`, standing for `count` people; returns its index.
    pub fn push_copy(&mut self, row: usize, count: u32) -> usize {
        self.count.push(count);
        self.age_months.push(self.age_months[row]);
        self.female.push(self.female[row]);
        self.health.push(self.health[row]);
        self.skill.push(self.skill[row]);
        self.location.push(self.location[row]);
        self.household.push(self.household[row]);
        self.job.push(self.job[row]);
        self.married.push(self.married[row]);
        self.couple.push(self.couple[row]);
        self.len() - 1
    }

    /// Drop rows with nobody left, keeping the others in order.
    pub fn remove_empty(&mut self) {
        if self.count.iter().all(|&c| c > 0) {
            return;
        }
        // The runs of rows kept, as (from, to, length): few, as rows empty
        // here and there, so each column moves in a few block copies.
        let mut runs = Vec::new();
        let (mut i, mut at) = (0, 0);
        while i < self.count.len() {
            if self.count[i] == 0 {
                i += 1;
                continue;
            }
            let from = i;
            while i < self.count.len() && self.count[i] > 0 {
                i += 1;
            }
            runs.push((from, at, i - from));
            at += i - from;
        }
        fn keep<T: Copy>(column: &mut Vec<T>, runs: &[(usize, usize, usize)], kept: usize) {
            for &(from, to, length) in runs {
                if from != to {
                    column.copy_within(from..from + length, to);
                }
            }
            column.truncate(kept);
        }
        keep(&mut self.count, &runs, at);
        keep(&mut self.age_months, &runs, at);
        keep(&mut self.female, &runs, at);
        keep(&mut self.health, &runs, at);
        keep(&mut self.skill, &runs, at);
        keep(&mut self.location, &runs, at);
        keep(&mut self.household, &runs, at);
        keep(&mut self.job, &runs, at);
        keep(&mut self.married, &runs, at);
        keep(&mut self.couple, &runs, at);
    }

    /// Put the rows in order of place, then family (keeping their order
    /// within a family). Rows carry no meaning in their order; this one
    /// keeps a family's members side by side, so the many rules that go
    /// from people to their families read memory in order rather than
    /// jumping about it.
    pub fn sort_by_family(&mut self) {
        let order = stable_order(&self.location, &stable_order(&self.household, &(0..self.len()).collect::<Vec<_>>()));
        fn gather<T: Copy>(column: &mut Vec<T>, order: &[usize]) {
            *column = order.iter().map(|&i| column[i]).collect();
        }
        gather(&mut self.count, &order);
        gather(&mut self.age_months, &order);
        gather(&mut self.female, &order);
        gather(&mut self.health, &order);
        gather(&mut self.skill, &order);
        gather(&mut self.location, &order);
        gather(&mut self.household, &order);
        gather(&mut self.job, &order);
        gather(&mut self.married, &order);
        gather(&mut self.couple, &order);
    }

    /// Separate `take[i]` people out of each row i, e.g. those hit by an event.
    ///
    /// Returns the indices of rows that now hold exactly the taken people, so
    /// the caller can change their attributes. A row taken in full is
    /// returned as is; otherwise its taken people move to a new copy of it
    /// (rows taken in full first, then the new copies, as in Python).
    pub fn split(&mut self, take: &[u32]) -> Vec<usize> {
        let mut whole = Vec::new();
        let mut partial = Vec::new();
        for (i, &t) in take.iter().enumerate() {
            if t > 0 {
                if t == self.count[i] {
                    whole.push(i);
                } else if t < self.count[i] {
                    partial.push(i);
                }
            }
        }
        for &i in &partial {
            self.count[i] -= take[i];
            let new = self.push_copy(i, take[i]);
            whole.push(new);
        }
        whole
    }

    /// Whether each row's people are of working age.
    #[inline]
    pub fn working_age(&self, row: usize, adult_age: i64, retirement_age: i64) -> bool {
        let age = self.age_years(row) as i64;
        age >= adult_age && age < retirement_age
    }
}

/// `order` rearranged by `keys` (a counting sort, so rows with equal keys
/// keep their order in `order`).
fn stable_order(keys: &[u32], order: &[usize]) -> Vec<usize> {
    let groups = keys.iter().map(|&k| k as usize + 1).max().unwrap_or(0);
    let mut start = vec![0usize; groups + 1];
    for &i in order {
        start[keys[i] as usize + 1] += 1;
    }
    for g in 0..groups {
        start[g + 1] += start[g];
    }
    let mut sorted = vec![0usize; order.len()];
    for &i in order {
        let g = keys[i] as usize;
        sorted[start[g]] = i;
        start[g] += 1;
    }
    sorted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_sorted_by_family_sit_by_place_then_family_and_keep_their_people() {
        let mut p = people(&[1, 1, 1, 1, 1]);
        p.location = vec![1, 0, 1, 0, 1];
        p.household = vec![7, 2, 3, 2, 7];
        p.age_months = vec![10, 20, 30, 40, 50];
        p.sort_by_family();
        assert_eq!(p.location, [0, 0, 1, 1, 1]);
        assert_eq!(p.household, [2, 2, 3, 7, 7]);
        assert_eq!(p.age_months, [20, 40, 30, 10, 50]);
    }

    fn people(counts: &[u32]) -> Population {
        let n = counts.len();
        Population {
            count: counts.to_vec(),
            age_months: (0..n as i32).map(|i| i * 12).collect(),
            female: vec![false; n],
            health: vec![90.0; n],
            skill: vec![1.0; n],
            location: vec![0; n],
            household: vec![0; n],
            job: vec![NO_JOB; n],
            married: vec![false; n],
            couple: vec![-1; n],
        }
    }

    #[test]
    fn splitting_a_row_moves_the_taken_people_to_a_copy() {
        let mut pop = people(&[1, 5, 3]);
        let rows = pop.split(&[1, 2, 0]);
        assert_eq!(rows, vec![0, 3]);
        assert_eq!(pop.count, vec![1, 3, 3, 2]);
        assert_eq!(pop.age_months[3], pop.age_months[1]);
        assert_eq!(pop.size(), 9);
    }

    #[test]
    fn emptied_rows_are_dropped_in_order() {
        let mut pop = people(&[1, 0, 2, 0, 4]);
        pop.remove_empty();
        assert_eq!(pop.count, vec![1, 2, 4]);
        assert_eq!(pop.age_months, vec![0, 24, 48]);
    }
}
