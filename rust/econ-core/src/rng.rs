//! Named random streams.
//!
//! Each rule draws from its own stream, derived from the main seed and the
//! stream's name (as Python's `RandomStreams`: seed and crc32 of the name).
//! Adding a new rule or event therefore doesn't shift the random draws of
//! existing ones, and a paired run shares its baseline's luck until the
//! event starts. The draws differ from numpy's: Rust and Python agree in
//! distribution, not number for number.

use rand::{Rng as _, SeedableRng};
use rand_distr::{Binomial, Distribution, Normal, Poisson};
use rand_xoshiro::Xoshiro256PlusPlus;

/// One stream of random numbers, with the draws the rules use.
#[derive(Clone, Debug)]
pub struct Rng(Xoshiro256PlusPlus);

/// One step of SplitMix64: spreads a seed's bits over the whole state.
fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl Rng {
    /// The stream for (`seed`, `key`): as numpy's `default_rng([seed, key])`,
    /// two different pairs give unrelated streams.
    pub fn from_pair(seed: u64, key: u64) -> Rng {
        let mut state = seed ^ splitmix(&mut key.wrapping_mul(0xD6E8_FEB8_6659_FD93));
        let mut bytes = [0u8; 32];
        for chunk in bytes.chunks_mut(8) {
            chunk.copy_from_slice(&splitmix(&mut state).to_le_bytes());
        }
        Rng(Xoshiro256PlusPlus::from_seed(bytes))
    }

    /// The stream of rule `name` under `seed`.
    pub fn named(seed: i64, name: &str) -> Rng {
        Rng::from_pair(seed as u64, crc32fast::hash(name.as_bytes()) as u64)
    }

    /// Uniform in [0, 1).
    #[inline]
    pub fn random(&mut self) -> f64 {
        self.0.gen::<f64>()
    }

    /// Uniform in [low, high).
    #[inline]
    pub fn uniform(&mut self, low: f64, high: f64) -> f64 {
        low + (high - low) * self.random()
    }

    /// An integer in [low, high), as numpy's `integers(low, high)`.
    #[inline]
    pub fn integers(&mut self, low: i64, high: i64) -> i64 {
        if high <= low {
            return low;
        }
        self.0.gen_range(low..high)
    }

    /// An index below `n`.
    #[inline]
    pub fn below(&mut self, n: usize) -> usize {
        if n <= 1 {
            return 0;
        }
        self.0.gen_range(0..n)
    }

    pub fn normal(&mut self, mean: f64, sd: f64) -> f64 {
        if sd <= 0.0 {
            return mean;
        }
        Normal::new(mean, sd).map(|d| d.sample(&mut self.0)).unwrap_or(mean)
    }

    pub fn lognormal(&mut self, mean: f64, sigma: f64) -> f64 {
        self.normal(mean, sigma).exp()
    }

    /// How many of `n` trials succeed with chance `p` each: one roll per
    /// person, so a row of one person is a plain roll.
    #[inline]
    pub fn binomial(&mut self, n: u64, p: f64) -> u64 {
        if n == 0 || p <= 0.0 || p.is_nan() {
            return 0;
        }
        if p >= 1.0 {
            return n;
        }
        if n == 1 {
            return (self.random() < p) as u64;
        }
        Binomial::new(n, p).map(|d| d.sample(&mut self.0)).unwrap_or(0)
    }

    pub fn poisson(&mut self, lambda: f64) -> u64 {
        if lambda <= 0.0 || lambda.is_nan() {
            return 0;
        }
        Poisson::new(lambda).map(|d| d.sample(&mut self.0) as u64).unwrap_or(0)
    }

    /// An index drawn with chances in proportion to `weights` (as numpy's
    /// `choice(len(weights), p=weights / sum)`).
    pub fn choose_weighted(&mut self, cumulative: &[f64]) -> usize {
        let total = *cumulative.last().unwrap_or(&0.0);
        let at = self.random() * total;
        cumulative.partition_point(|&c| c <= at).min(cumulative.len().saturating_sub(1))
    }

    /// Shuffle in place (a random permutation).
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.0.gen_range(0..=i);
            items.swap(i, j);
        }
    }

    /// `k` of `items` at random, in random order (the first `k` of a
    /// shuffle, without shuffling the rest).
    pub fn choose_some<'a, T>(&mut self, items: &'a mut [T], k: usize) -> &'a [T] {
        let k = k.min(items.len());
        for i in 0..k {
            let j = self.0.gen_range(i..items.len());
            items.swap(i, j);
        }
        &items[..k]
    }
}

/// Running sums of `weights`, for `Rng::choose_weighted`.
pub fn cumulative(weights: &[f64]) -> Vec<f64> {
    let mut total = 0.0;
    weights
        .iter()
        .map(|w| {
            total += w.max(0.0);
            total
        })
        .collect()
}

/// The streams of one run, by name, made the first time each is asked for.
#[derive(Clone, Debug)]
pub struct RandomStreams {
    pub seed: i64,
    streams: Vec<(String, Rng)>,
}

impl RandomStreams {
    pub fn new(seed: i64) -> RandomStreams {
        RandomStreams { seed, streams: Vec::new() }
    }

    pub fn get(&mut self, name: &str) -> &mut Rng {
        let at = self.index(name);
        &mut self.streams[at].1
    }

    /// Two different streams at once.
    pub fn pair(&mut self, a: &str, b: &str) -> (&mut Rng, &mut Rng) {
        let (i, j) = (self.index(a), self.index(b));
        assert_ne!(i, j, "two different streams");
        if i < j {
            let (left, right) = self.streams.split_at_mut(j);
            (&mut left[i].1, &mut right[0].1)
        } else {
            let (left, right) = self.streams.split_at_mut(i);
            (&mut right[0].1, &mut left[j].1)
        }
    }

    fn index(&mut self, name: &str) -> usize {
        match self.streams.iter().position(|(n, _)| n == name) {
            Some(at) => at,
            None => {
                self.streams.push((name.to_string(), Rng::named(self.seed, name)));
                self.streams.len() - 1
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stream_depends_on_its_name_and_seed_only() {
        let mut a = RandomStreams::new(7);
        let mut b = RandomStreams::new(7);
        b.get("births").random(); // another rule drawing first changes nothing
        assert_eq!(a.get("deaths").random(), b.get("deaths").random());
        assert_ne!(Rng::named(7, "deaths").random(), Rng::named(8, "deaths").random());
        assert_ne!(Rng::named(7, "deaths").random(), Rng::named(7, "births").random());
    }

    #[test]
    fn binomial_draws_have_the_right_mean() {
        let mut rng = Rng::named(1, "test");
        let ones: u64 = (0..100_000).map(|_| rng.binomial(1, 0.3)).sum();
        assert!((ones as f64 / 100_000.0 - 0.3).abs() < 0.01);
        let many: u64 = (0..10_000).map(|_| rng.binomial(20, 0.25)).sum();
        assert!((many as f64 / 10_000.0 - 5.0).abs() < 0.1);
        assert_eq!(rng.binomial(5, 1.0), 5);
        assert_eq!(rng.binomial(5, 0.0), 0);
    }

    #[test]
    fn weighted_choice_follows_the_weights() {
        let mut rng = Rng::named(2, "test");
        let cum = cumulative(&[1.0, 0.0, 3.0]);
        let mut seen = [0usize; 3];
        for _ in 0..40_000 {
            seen[rng.choose_weighted(&cum)] += 1;
        }
        assert_eq!(seen[1], 0);
        assert!((seen[2] as f64 / 40_000.0 - 0.75).abs() < 0.01);
    }
}
