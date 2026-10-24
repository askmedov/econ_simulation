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
        let kept: Vec<bool> = self.count.iter().map(|&c| c > 0).collect();
        fn keep<T: Copy>(column: &mut Vec<T>, kept: &[bool]) {
            let mut at = 0;
            for i in 0..column.len() {
                if kept[i] {
                    column[at] = column[i];
                    at += 1;
                }
            }
            column.truncate(at);
        }
        keep(&mut self.count, &kept);
        keep(&mut self.age_months, &kept);
        keep(&mut self.female, &kept);
        keep(&mut self.health, &kept);
        keep(&mut self.skill, &kept);
        keep(&mut self.location, &kept);
        keep(&mut self.household, &kept);
        keep(&mut self.job, &kept);
        keep(&mut self.married, &kept);
        keep(&mut self.couple, &kept);
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

#[cfg(test)]
mod tests {
    use super::*;

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
