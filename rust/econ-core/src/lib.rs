//! The simulation engine: a month-by-month simulation of a pre-industrial
//! economy, ported from the Python model (`econ_sim/`), which stays the
//! reference it is checked against. Modules keep Python's names, so the two
//! can be read side by side.

// The rules loop over parallel arrays by place or family index, as the
// Python reads them; iterator chains over several arrays read worse.
#![allow(clippy::needless_range_loop)]

pub mod checks;
pub mod config;
pub mod council;
pub mod credit;
pub mod economy;
pub mod environment;
pub mod events;
pub mod farms;
pub mod geography;
pub mod grid;
pub mod healthcare;
pub mod households;
pub mod livestock;
pub mod lords;
pub mod market;
pub mod metrics;
pub mod migration;
pub mod minds;
pub mod population;
pub mod region;
pub mod rng;
pub mod rules;
pub mod scenarios;
pub mod simulation;
pub mod starts;
pub mod town;
pub mod towns;
pub mod work;
pub mod world;

pub use config::Config;
pub use simulation::Simulation;
