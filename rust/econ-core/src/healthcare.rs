//! Basic healthcare: healers paid by the village council see the people
//! most likely to die first, as far as their time allows.

use crate::config::Config;
use crate::population::Population;

pub fn healer_job(config: &Config) -> i16 {
    config.businesses.len() as i16 + 1 // just past the council's officials
}

/// Treat patients up to each place's capacity (`healers` per place).
///
/// Healers triage: they see the people most likely to die this month first
/// (`risk` per row), down to `min_risk`: infants, the old, the starving and
/// anyone hit by an outbreak. Treated people gain health now. Returns (care
/// multiplier on each row's risk of dying, people treated per place).
/// Splits groups when only some of a row can be seen.
pub fn treat(population: &mut Population, healers: &[f64], risk: &[f64], config: &Config) -> (Vec<f64>, Vec<f64>) {
    let cfg = &config.healthcare;
    let n = healers.len();
    let mut treated = vec![0.0; n];
    if !cfg.enabled || !healers.iter().any(|&h| h > 0.0) {
        return (vec![1.0; population.len()], treated);
    }
    let capacity: Vec<f64> = healers.iter().map(|h| h * cfg.patients_per_healer).collect();
    // Patients by place, the most at risk first; each place's healers see
    // them in that order until their time runs out.
    let mut patients: Vec<Vec<usize>> = vec![Vec::new(); n];
    for i in 0..population.len() {
        let v = population.location[i] as usize;
        if capacity[v] > 0.0 && risk[i] >= cfg.min_risk {
            patients[v].push(i);
        }
    }
    let mut take = vec![0u32; population.len()];
    for (v, rows) in patients.iter_mut().enumerate() {
        rows.sort_by(|&a, &b| risk[b].partial_cmp(&risk[a]).unwrap_or(std::cmp::Ordering::Equal));
        let mut before = 0.0;
        for &i in rows.iter() {
            let count = population.count[i] as f64;
            if before >= capacity[v] {
                break;
            }
            take[i] = (capacity[v] - before).clamp(0.0, count) as u32;
            before += count;
        }
    }
    let rows = population.split(&take);
    let mut care = vec![1.0; population.len()];
    for &i in &rows {
        care[i] = cfg.treatment_mortality;
        population.health[i] = (population.health[i] + cfg.treatment_recovery).min(config.health.maximum);
        treated[population.location[i] as usize] += population.count[i] as f64;
    }
    (care, treated)
}
