//! Invariants of the world's state: things that must hold every month, or
//! the model has a bug. Used by the tests and the sanity check.

use std::collections::HashMap;

use crate::population::NO_JOB;
use crate::world::World;

/// Equal up to rounding: a millionth of `scale`, or of `floor` when the
/// values are small (rounding left from when they were large).
fn close(a: &[f64], b: &[f64], scale: &[f64], floor: f64) -> bool {
    (0..a.len()).all(|i| (a[i] - b[i]).abs() <= 1e-6 * scale[i].max(floor))
}

/// Names of the invariants the world breaks right now (empty if none).
pub fn broken_invariants(world: &World) -> Vec<String> {
    let mut broken = Vec::new();
    let hh = &world.households;
    let pop = &world.population;
    let n = world.n_locations();
    let n_hh = hh.len();
    let per_place = |values: &[f64]| hh.by_place(n, |h| values[h]);

    // Land: families' holdings plus the lord's demesne are the place's land.
    let held: Vec<f64> = per_place(&hh.land).iter().zip(&world.lord.land).map(|(a, b)| a + b).collect();
    if !close(&held, &world.land, &world.land, 1.0) {
        broken.push("land not conserved".into());
    }
    // Debts and claims on them balance in every place.
    let debt = per_place(&hh.debt);
    let lent = per_place(&hh.lent);
    let bigger: Vec<f64> = debt.iter().zip(&lent).map(|(a, b)| a.max(*b)).collect();
    if !close(&debt, &lent, &bigger, 1000.0) {
        // (a thousandth of a coin)
        broken.push("debts and claims don't balance".into());
    }
    // Nothing held is negative.
    let held: [(&str, &[f64]); 16] = [
        ("grain", &hh.grain),
        ("money", &hh.money),
        ("land", &hh.land),
        ("animals", &hh.animals),
        ("debt", &hh.debt),
        ("lent", &hh.lent),
        ("business cash", &world.cash.data),
        ("tools", &world.tools.data),
        ("stock", &world.stock.data),
        ("supplies", &world.supplies.data),
        ("lord's barn", &world.lord.barn),
        ("council reserve", &world.council.reserve),
        ("treasury", &world.council.treasury),
        ("woods", &world.woods),
        ("dealers' grain", &world.dealers_grain),
        ("dealers' coins", &world.dealers_cash),
    ];
    for (name, values) in held {
        if values.iter().any(|v| !v.is_finite() || *v < -1e-9) {
            broken.push(format!("negative or not-a-number {name}"));
        }
    }
    if world.soil.iter().any(|s| !(s.is_finite() && *s > 0.0)) || world.sown.iter().any(|s| *s < 0.0) {
        broken.push("soil or sowing out of range".into());
    }
    // People: whole, of a real age, in a household of their own place.
    if !pop.is_empty() {
        if pop.count.iter().any(|&c| c < 1) || pop.age_months.iter().any(|&a| a < 0) {
            broken.push("empty rows or negative ages".into());
        }
        if pop.household.iter().any(|&h| h as usize >= n_hh) {
            broken.push("people in no household".into());
        } else if (0..pop.len()).any(|i| hh.location[pop.household[i] as usize] != pop.location[i]) {
            broken.push("people living outside their household's place".into());
        }
        if pop.job.iter().any(|&j| j != NO_JOB && j < 0) {
            broken.push("unknown job".into());
        }
        // Couples: at most two people, a man and a woman, both married.
        if (0..pop.len()).any(|i| pop.couple[i] >= 0 && !pop.married[i]) {
            broken.push("unmarried people in a couple".into());
        }
        let mut couples: HashMap<i32, (u32, u32)> = HashMap::new(); // id -> (members, women)
        for i in 0..pop.len() {
            if pop.couple[i] >= 0 {
                let entry = couples.entry(pop.couple[i]).or_insert((0, 0));
                entry.0 += 1;
                entry.1 += pop.female[i] as u32;
            }
        }
        if couples.values().any(|&(members, women)| members > 2 || women > 1 || members - women > 1) {
            broken.push("couples of more than a man and a woman".into());
        }
    }
    // Kin links point at real families of the same place.
    if (0..n_hh).any(|h| {
        let k = hh.kin[h];
        k >= n_hh as i32 || (k >= 0 && hh.location[k as usize] != hh.location[h])
    }) {
        broken.push("kin links to no family or another place".into());
    }
    broken
}
