//! Families: who lives, earns and eats together.
//!
//! Each person row has a `household` index into the Households table.
//! Families are formed at the start; newborns join their mother's family,
//! and young people marry: the first of a family to marry stays and brings
//! their spouse home (the heir), later ones set up a household of their own.

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::population::Population;
use crate::rng::Rng;
use crate::rules;

pub const ADULT_AGE: i32 = 18;
pub const HEAD_AGES: (i32, i32) = (18, 64); // married women of these ages head a household at the start
pub const PARTNER_MAX_AGE: i32 = 64;
pub const MOTHER_AGE_GAP: (i32, i32) = (18, 45); // a child joins a woman this much older

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Households {
    pub location: Vec<u32>, // place of each household
    pub money: Vec<f64>,    // savings, in coins
    pub land: Vec<f64>,     // farmland held, in plots
    pub grain: Vec<f64>,    // food in the family's own store, in rations
    pub animals: Vec<f64>,  // livestock units
    pub debt: Vec<f64>,     // coins owed to the place's lenders
    pub lent: Vec<f64>,     // coins the family is owed (its claim on the place's borrowers)
    pub kin: Vec<i32>,      // the family this one came from (-1: none known)
}

impl Households {
    pub fn with_locations(location: Vec<u32>) -> Households {
        let n = location.len();
        Households {
            location,
            money: vec![0.0; n],
            land: vec![0.0; n],
            grain: vec![0.0; n],
            animals: vec![0.0; n],
            debt: vec![0.0; n],
            lent: vec![0.0; n],
            kin: vec![-1; n],
        }
    }

    pub fn len(&self) -> usize {
        self.location.len()
    }

    pub fn is_empty(&self) -> bool {
        self.location.is_empty()
    }

    /// Append an empty household in this place; returns its id.
    pub fn add(&mut self, location: u32) -> usize {
        self.location.push(location);
        self.money.push(0.0);
        self.land.push(0.0);
        self.grain.push(0.0);
        self.animals.push(0.0);
        self.debt.push(0.0);
        self.lent.push(0.0);
        self.kin.push(-1);
        self.len() - 1
    }

    /// Sum a per-household value for each place.
    pub fn by_place(&self, n: usize, value: impl Fn(usize) -> f64) -> Vec<f64> {
        crate::grid::sum_by(n, &self.location, value)
    }
}

/// People in each household.
pub fn sizes(population: &Population, n_households: usize) -> Vec<f64> {
    let mut size = vec![0.0; n_households];
    for (i, &h) in population.household.iter().enumerate() {
        size[h as usize] += population.count[i] as f64;
    }
    size
}

/// Group the starting villagers into families and set `household`,
/// `married` and `couple`.
///
/// Women of family age head a household, with a husband of similar age if
/// there is one. A young woman has married with `yearly_marriage` for each
/// year since turning 18; the others, like children and young men without a
/// wife, live with a woman old enough to be their mother.
pub fn form_households(
    population: &mut Population,
    n_locations: usize,
    rng: &mut Rng,
    yearly_marriage: f64,
) -> Households {
    let n = population.len();
    let mut household = vec![-1i64; n];
    population.married = (0..n).map(|i| population.age_years(i) >= ADULT_AGE).collect();
    population.couple = vec![-1; n];
    let mut by_place: Vec<Vec<usize>> = vec![Vec::new(); n_locations];
    for i in 0..n {
        by_place[population.location[i] as usize].push(i);
    }
    let ages: Vec<i32> = (0..n).map(|i| population.age_years(i)).collect();
    let mut locations: Vec<u32> = Vec::new();
    for (location, rows) in by_place.iter().enumerate() {
        if rows.is_empty() {
            continue;
        }
        let first = locations.len();
        let count = form_village(population, &ages, rows, first, &mut household, yearly_marriage, rng);
        locations.extend(std::iter::repeat_n(location as u32, count));
    }
    population.household = household.iter().map(|&h| h as u32).collect();
    Households::with_locations(locations)
}

/// Form one place's households, numbered from `first`; returns how many.
/// `ages` are everyone's ages in years.
fn form_village(
    population: &mut Population,
    ages: &[i32],
    rows: &[usize],
    first: usize,
    household: &mut [i64],
    yearly_marriage: f64,
    rng: &mut Rng,
) -> usize {
    let age = |i: usize| ages[i];
    let women: Vec<usize> = rows
        .iter()
        .cloned()
        .filter(|&i| population.female[i] && age(i) >= HEAD_AGES.0 && age(i) <= HEAD_AGES.1)
        .collect();
    let mut still_single = vec![false; population.len()];
    for &i in rows {
        still_single[i] = rng.random() < (1.0 - yearly_marriage).powf((age(i) - ADULT_AGE) as f64 + 0.5);
    }
    let mut heads: Vec<usize> = women.iter().cloned().filter(|&i| !still_single[i]).collect();
    if heads.is_empty() {
        heads = women.clone();
    }
    if heads.is_empty() {
        // no women of family age: everyone lives together
        for &i in rows {
            household[i] = first as i64;
        }
        return 1;
    }
    heads.sort_by_key(|&i| age(i)); // (a stable sort)
    for (k, &i) in heads.iter().enumerate() {
        household[i] = (first + k) as i64;
    }

    // Men pair with women in age order, so partners are of similar age. If
    // there are more men than wives, the youngest stay single.
    let mut men: Vec<usize> = rows
        .iter()
        .cloned()
        .filter(|&i| !population.female[i] && age(i) >= ADULT_AGE && age(i) <= PARTNER_MAX_AGE)
        .collect();
    men.sort_by_key(|&i| age(i));
    let pairs = men.len().min(heads.len());
    let unpaired = men.len() - pairs;
    for k in 0..pairs {
        let id = (first + k) as i64;
        household[men[unpaired + k]] = id;
        population.couple[heads[k]] = id as i32;
        population.couple[men[unpaired + k]] = id as i32;
    }
    let mut single: Vec<usize> = women.iter().cloned().filter(|&i| household[i] < 0).collect();
    single.extend_from_slice(&men[..unpaired]);
    for &i in &single {
        population.married[i] = false;
    }

    // Children and the single young join a woman old enough to be their
    // mother (heads are in order of age, so those are a run of them), or
    // any family if there is none.
    let head_ages: Vec<i32> = heads.iter().map(|&i| age(i)).collect();
    let mut children: Vec<usize> = rows.iter().cloned().filter(|&i| age(i) < ADULT_AGE).collect();
    children.extend_from_slice(&single);
    for &child in &children {
        let a = age(child);
        let mut low = head_ages.partition_point(|&h| h < a + MOTHER_AGE_GAP.0);
        let mut high = head_ages.partition_point(|&h| h <= a + MOTHER_AGE_GAP.1);
        if high <= low {
            low = 0;
            high = heads.len();
        }
        let pick = low + (rng.random() * (high - low) as f64).floor() as usize;
        household[child] = (first + pick) as i64;
    }

    // Everyone else (older people) lives with a family.
    for &i in rows {
        if household[i] < 0 {
            household[i] = (first + rng.below(heads.len())) as i64;
        }
    }
    heads.len()
}

/// At the start, link each family to a family of its place a generation
/// older (its oldest member 18 to 45 years older), if there is one: the
/// family it came from.
pub fn link_kin(households: &mut Households, population: &Population, rng: &mut Rng) {
    let n = households.len();
    let mut oldest = vec![-1i64; n];
    for i in 0..population.len() {
        let h = population.household[i] as usize;
        oldest[h] = oldest[h].max(population.age_years(i) as i64);
    }
    // Families in order of place, then of their oldest member's age: each
    // family's possible kin are one run of that order.
    let key: Vec<i64> = (0..n).map(|h| households.location[h] as i64 * 1000 + oldest[h]).collect();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|&h| key[h]);
    let ordered: Vec<i64> = order.iter().map(|&h| key[h]).collect();
    for h in 0..n {
        let low = ordered.partition_point(|&k| k < key[h] + 18);
        let high = ordered.partition_point(|&k| k <= key[h] + 45);
        let pick = low as f64 + (rng.random() * (high as f64 - low as f64)).floor();
        households.kin[h] = if oldest[h] >= 0 && high > low { order[(pick as usize).min(n - 1)] as i32 } else { -1 };
    }
}

/// Share each place's farmland among its households at the start.
///
/// `landless_share` of households hold none (they live by wages and
/// crafts); the others' holdings are spread log-normally (a few big farms,
/// many small ones), larger for families with more workers.
pub fn assign_land(
    households: &mut Households,
    population: &Population,
    village_land: &[f64],
    config: &Config,
    rng: &mut Rng,
) {
    let cfg = &config.land;
    let n = households.len();
    let size = sizes(population, n);
    let mut adults = vec![0.0; n];
    for i in 0..population.len() {
        if population.age_years(i) >= 15 {
            adults[population.household[i] as usize] += population.count[i] as f64;
        }
    }
    let mut weight: Vec<f64> = (0..n).map(|h| adults[h].max(1.0) * rng.lognormal(0.0, cfg.holding_spread)).collect();
    for w in weight.iter_mut() {
        if rng.random() < cfg.landless_share {
            *w = 0.0;
        }
    }
    for h in 0..n {
        if size[h] == 0.0 {
            weight[h] = 0.0;
        }
    }
    let places = village_land.len();
    let total = households.by_place(places, |h| weight[h]);
    // Every place has someone holding its land: the family with most adults.
    for (village, &t) in total.iter().enumerate() {
        if t > 0.0 {
            continue;
        }
        let mut best: Option<usize> = None;
        for h in 0..n {
            if households.location[h] as usize == village && size[h] > 0.0 && best.is_none_or(|b| adults[h] > adults[b])
            {
                best = Some(h);
            }
        }
        if let Some(h) = best {
            weight[h] = 1.0;
        }
    }
    let total = households.by_place(places, |h| weight[h]);
    for h in 0..n {
        let loc = households.location[h] as usize;
        households.land[h] = if total[loc] > 0.0 { weight[h] * village_land[loc] / total[loc] } else { 0.0 };
    }
}

/// Land and grain of families with nobody left.
///
/// A vacant holding, with its animals, goes whole to a landless family of
/// the place (the newest, usually a young couple), or if there is none is
/// shared among the other families by size. Grain goes to the council's
/// reserve where there is a council, otherwise to the other families by
/// size. Returns grain added to the reserve, per place.
pub fn pass_on_land_and_grain(
    households: &mut Households,
    size: &[f64],
    reserve: &mut [f64],
    council: &[bool],
) -> Vec<f64> {
    let n = reserve.len();
    let n_hh = households.len();
    let gone: Vec<bool> = size.iter().map(|&s| s == 0.0).collect();
    let vacant: Vec<usize> =
        (0..n_hh).filter(|&h| gone[h] && (households.land[h] > 0.0 || households.animals[h] > 0.0)).collect();
    if !vacant.is_empty() {
        let mut living: Vec<Vec<usize>> = vec![Vec::new(); n];
        for h in 0..n_hh {
            if !gone[h] {
                living[households.location[h] as usize].push(h);
            }
        }
        for &household in &vacant {
            let here = &living[households.location[household] as usize];
            let newest_landless = here.iter().rev().find(|&&h| households.land[h] <= 0.0).cloned();
            let people: f64 = here.iter().map(|&h| size[h]).sum();
            for held in [&mut households.land, &mut households.animals] {
                if let Some(to) = newest_landless {
                    held[to] += held[household];
                } else if !here.is_empty() {
                    let given = held[household];
                    for &h in here {
                        held[h] += given * size[h] / people;
                    }
                } else {
                    continue; // nobody left in the place: it waits
                }
                held[household] = 0.0;
            }
        }
    }

    let mut to_reserve = vec![0.0; n];
    let mut left = vec![false; n_hh];
    let mut any = false;
    for h in 0..n_hh {
        if gone[h] && households.grain[h] > 0.0 {
            let loc = households.location[h] as usize;
            if council[loc] {
                to_reserve[loc] += households.grain[h];
                households.grain[h] = 0.0;
            } else {
                left[h] = true;
                any = true;
            }
        }
    }
    for (r, add) in reserve.iter_mut().zip(&to_reserve) {
        *r += add;
    }
    if !any {
        return to_reserve;
    }
    let grain = households.by_place(n, |h| if left[h] { households.grain[h] } else { 0.0 });
    let people = households.by_place(n, |h| if gone[h] { 0.0 } else { size[h] });
    let share: Vec<f64> = (0..n).map(|v| if people[v] > 0.0 { grain[v] / people[v] } else { 0.0 }).collect();
    for h in 0..n_hh {
        let loc = households.location[h] as usize;
        if left[h] && people[loc] > 0.0 {
            households.grain[h] = 0.0;
        }
        if !gone[h] {
            households.grain[h] += size[h] * share[loc];
        }
    }
    to_reserve
}

/// Savings of families with nobody left go to the place's council if there
/// is one (`council` per place, paid into `treasury`), otherwise to the
/// other families of their place.
pub fn pass_on_savings(households: &mut Households, size: &[f64], treasury: &mut [f64], council: &[bool]) {
    let n_hh = households.len();
    let mut gone: Vec<bool> = (0..n_hh).map(|h| size[h] == 0.0 && households.money[h] > 0.0).collect();
    if !gone.iter().any(|&g| g) {
        return;
    }
    let n = treasury.len();
    if council.iter().any(|&c| c) {
        for h in 0..n_hh {
            let loc = households.location[h] as usize;
            if gone[h] && council[loc] {
                treasury[loc] += households.money[h];
                households.money[h] = 0.0;
                gone[h] = false;
            }
        }
        if !gone.iter().any(|&g| g) {
            return;
        }
    }
    let left = households.by_place(n, |h| if gone[h] { households.money[h] } else { 0.0 });
    let heirs = households.by_place(n, |h| if size[h] > 0.0 { 1.0 } else { 0.0 });
    for h in 0..n_hh {
        let loc = households.location[h] as usize;
        if size[h] > 0.0 {
            if heirs[loc] > 0.0 {
                households.money[h] += left[loc] / heirs[loc];
            }
        } else if gone[h] && heirs[loc] > 0.0 {
            households.money[h] = 0.0; // (kept where nobody is left in the place at all)
        }
    }
}

/// Young people marry, and widows and widowers remarry; returns the number
/// of weddings.
///
/// Each month a single woman of `bride_ages` (or a widow up to
/// `widow_ages[0]`) marries with `marriage_chance` (times `willingness` in
/// her place: people wait when a wage can barely feed a family), to a
/// single man of `groom_ages` (or a widower up to `widow_ages[1]`) from
/// another family of her place while any are left. A widow or widower
/// stays in their household and the new spouse moves in. Otherwise the
/// couple live with the groom's family if it has no heir yet, else with the
/// bride's if that has none, else in a new household. Whoever moves takes
/// their share of their family's savings and grain (savings / family size),
/// and of its land and animals if land is split among children
/// (`LandConfig.partible`); otherwise the heir keeps them all. A new
/// household's kin is the groom's family.
pub fn marry(
    population: &mut Population,
    households: &mut Households,
    config: &Config,
    rng: &mut Rng,
    willingness: Option<&[f64]>,
) -> usize {
    let demo = &config.demography;
    let n = households.len();
    let n_places = population.location.iter().map(|&l| l as usize + 1).max().unwrap_or(0);
    let partnered = rules::spouse_alive(population);
    let age = |i: usize| population.age_years(i) as i64;
    let mut brides_by_place: Vec<Vec<usize>> = vec![Vec::new(); n_places];
    let mut grooms_by_place: Vec<Vec<usize>> = vec![Vec::new(); n_places];
    let mut widowed = vec![false; population.len()];
    let mut any_bride = false;
    let mut any_groom = false;
    for i in 0..population.len() {
        if population.count[i] != 1 {
            continue;
        }
        let single = !population.married[i];
        widowed[i] = population.married[i] && !partnered[i];
        let a = age(i);
        if population.female[i] {
            let may = (single && a <= demo.bride_ages.1) || (widowed[i] && a <= demo.widow_ages.0);
            if may && a >= demo.bride_ages.0 {
                let chance = demo.marriage_chance * willingness.map_or(1.0, |w| w[population.location[i] as usize]);
                if rng.random() < chance {
                    brides_by_place[population.location[i] as usize].push(i);
                    any_bride = true;
                }
            }
        } else {
            let may = (single && a <= demo.groom_ages.1) || (widowed[i] && a <= demo.widow_ages.1);
            if may && a >= demo.groom_ages.0 {
                grooms_by_place[population.location[i] as usize].push(i);
                any_groom = true;
            }
        }
    }
    if !any_bride || !any_groom {
        return 0;
    }
    let mut couples: Vec<(usize, usize, u32)> = Vec::new();
    for location in 0..n_places {
        let here_b = &mut brides_by_place[location];
        if here_b.is_empty() {
            continue;
        }
        let here_g = &mut grooms_by_place[location];
        rng.shuffle(here_b);
        rng.shuffle(here_g);
        // Each bride in turn takes the first free groom not of her own family:
        // unless some pair would be of one family, that is simply in order.
        let k = here_b.len().min(here_g.len());
        let clash = (0..k).any(|j| population.household[here_b[j]] == population.household[here_g[j]]);
        if !clash {
            couples.extend((0..k).map(|j| (here_b[j], here_g[j], location as u32)));
            continue;
        }
        let mut free = here_g.clone();
        for &bride in here_b.iter() {
            let home = population.household[bride];
            if let Some(m) = free.iter().position(|&g| population.household[g] != home) {
                couples.push((bride, free.remove(m), location as u32));
            }
        }
    }
    if couples.is_empty() {
        return 0;
    }
    let family_b: Vec<usize> = couples.iter().map(|c| population.household[c.0] as usize).collect();
    let family_g: Vec<usize> = couples.iter().map(|c| population.household[c.1] as usize).collect();

    // A family takes in a couple (at most one a month) if it holds no more
    // than one couple and the newlywed is a generation (15 years) younger
    // than its married members; so heirs stay, their brothers and sisters leave.
    let mut married = vec![0.0; n];
    let mut elder = vec![-1i64; n];
    for i in 0..population.len() {
        if population.married[i] {
            let h = population.household[i] as usize;
            married[h] += population.count[i] as f64;
            elder[h] = elder[h].max(age(i));
        }
    }
    let mut room: Vec<bool> = married.iter().map(|&m| m <= 2.0).collect();
    let can_stay = |room: &[bool], person: usize, family: usize| {
        room[family] && (elder[family] < 0 || age(person) <= elder[family] - 15)
    };

    // Widows and widowers stay where they are, with their children.
    let k = couples.len();
    let mut at_bride: Vec<bool> = couples.iter().map(|c| widowed[c.0]).collect();
    let mut at_groom: Vec<bool> = (0..k).map(|j| !at_bride[j] && widowed[couples[j].1]).collect();
    let mut rest: Vec<bool> = (0..k).map(|j| !at_bride[j] && !at_groom[j]).collect();
    let first_g = first_of(&(0..k).map(|j| if rest[j] { family_g[j] as i64 } else { -1 }).collect::<Vec<_>>());
    let heir_g: Vec<bool> =
        (0..k).map(|j| rest[j] && can_stay(&room, couples[j].1, family_g[j]) && first_g[j]).collect();
    for j in 0..k {
        if heir_g[j] {
            room[family_g[j]] = false;
        }
        rest[j] &= !heir_g[j];
    }
    let first_b = first_of(&(0..k).map(|j| if rest[j] { family_b[j] as i64 } else { -1 }).collect::<Vec<_>>());
    let heir_b: Vec<bool> =
        (0..k).map(|j| rest[j] && can_stay(&room, couples[j].0, family_b[j]) && first_b[j]).collect();
    let mut home = vec![0usize; k];
    let mut next_new = n;
    for j in 0..k {
        at_groom[j] |= heir_g[j];
        at_bride[j] |= heir_b[j];
        home[j] = if at_groom[j] { family_g[j] } else { family_b[j] };
        if !at_groom[j] && !at_bride[j] {
            home[j] = next_new;
            next_new += 1;
        }
    }

    // Whoever leaves takes their share of their family's savings and grain
    // (and land, where it is split among children).
    let size = sizes(population, n);
    let mut leaving_from: Vec<usize> = Vec::new();
    let mut going_to: Vec<usize> = Vec::new();
    for j in 0..k {
        if !at_bride[j] {
            leaving_from.push(family_b[j]);
            going_to.push(home[j]);
        }
    }
    for j in 0..k {
        if !at_groom[j] {
            leaving_from.push(family_g[j]);
            going_to.push(home[j]);
        }
    }
    for j in 0..k {
        if !at_groom[j] && !at_bride[j] {
            let founded = households.add(couples[j].2);
            households.kin[founded] = family_g[j] as i32;
        }
    }
    let move_shares = |held: &mut Vec<f64>| {
        let portions: Vec<f64> = leaving_from.iter().map(|&f| held[f] / size[f]).collect();
        for ((&from, &to), &p) in leaving_from.iter().zip(&going_to).zip(&portions) {
            held[from] -= p;
            held[to] += p;
        }
    };
    move_shares(&mut households.money);
    move_shares(&mut households.grain);
    if config.land.partible {
        move_shares(&mut households.land);
        move_shares(&mut households.animals);
    }
    // A family whose last members married out lives on in their new home:
    // all it holds and owes goes with them, and its kin are theirs.
    let mut leavers = vec![0.0; n];
    for &f in &leaving_from {
        leavers[f] += 1.0;
    }
    let emptied: Vec<usize> = (0..n).filter(|&h| size[h] > 0.0 && size[h] - leavers[h] <= 0.0).collect();
    if !emptied.is_empty() {
        let mut target = vec![-1i64; n];
        for (&from, &to) in leaving_from.iter().zip(&going_to) {
            target[from] = to as i64;
        }
        for held in [
            &mut households.money,
            &mut households.grain,
            &mut households.land,
            &mut households.animals,
            &mut households.debt,
            &mut households.lent,
        ] {
            for &h in &emptied {
                let value = held[h];
                held[target[h] as usize] += value;
                held[h] = 0.0;
            }
        }
        let mut remap: Vec<i32> = (0..households.len() as i32).collect();
        for &h in &emptied {
            remap[h] = target[h] as i32;
        }
        for h in 0..households.len() {
            if households.kin[h] >= 0 {
                households.kin[h] = remap[households.kin[h] as usize];
            }
            if households.kin[h] == h as i32 {
                households.kin[h] = -1;
            }
        }
    }

    let next_couple = population.couple.iter().cloned().max().unwrap_or(-1) + 1;
    for (j, &(bride, groom, _)) in couples.iter().enumerate() {
        population.household[bride] = home[j] as u32;
        population.household[groom] = home[j] as u32;
        population.married[bride] = true;
        population.married[groom] = true;
        population.couple[bride] = next_couple + j as i32;
        population.couple[groom] = next_couple + j as i32;
    }
    k
}

/// True at the first place each id (>= 0) appears.
fn first_of(ids: &[i64]) -> Vec<bool> {
    let mut seen = std::collections::HashSet::new();
    ids.iter().map(|&id| id >= 0 && seen.insert(id)).collect()
}
