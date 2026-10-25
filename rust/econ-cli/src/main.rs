//! The command line: runs and paired what-ifs as Python's
//! (`python -m econ_sim`), and tools for checking the engine.

mod bench;
mod dump;
mod minds;
mod options;
mod report;
mod run;
mod sanity;
mod stress;

use clap::{Parser, Subcommand};
use options::RunArgs;

#[derive(Parser)]
#[command(
    name = "econ-sim",
    about = "Simulate a pre-industrial economy month by month, and see what an event does to the next few years.",
    after_help = "Example: econ-sim --event drought@4 --runs 200",
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    #[command(flatten)]
    run: RunArgs,
}

#[derive(Subcommand)]
enum Command {
    /// Every month of every run, as CSV on standard output (with --event, both arms of each pair)
    Records(RunArgs),
    /// The config these options describe, as JSON (e.g. a region laid out by
    /// --region), to keep and run again with --config
    Config(RunArgs),
    /// The starting state of each run, one line of JSON each
    World(RunArgs),
    /// What minds change: each seed run by the rules and with minds of the
    /// families' own, without and with the forced events, compared seed by seed
    Minds(RunArgs),
    /// Each run's measures per place, one line of JSON each (with --event, both arms)
    Places(RunArgs),
    /// Each run's log of notable happenings (with the forced events)
    Logs(RunArgs),
    /// Run many seeds and settings and flag outcomes no real village would show (as scripts/sanity_check.py)
    Sanity {
        /// Many more seeds
        #[arg(long)]
        full: bool,
        /// Threads to use (default: all cores)
        #[arg(long)]
        workers: Option<usize>,
    },
    /// How fast the engine is, from 1,000 to 10,000,000 people (as scripts/benchmark.py)
    Bench {
        /// Only up to 100,000 people
        #[arg(long)]
        quick: bool,
        /// Up to 10,000,000 people
        #[arg(long)]
        country: bool,
        /// Months to time at each size: a year, as months differ
        #[arg(long, default_value_t = 12)]
        months: i64,
        /// Threads for the projections
        #[arg(long, default_value_t = 4)]
        workers: usize,
        /// Time this one size only
        #[arg(long)]
        people: Option<usize>,
    },
    /// Run the village through many scenarios for decades and look for drift (as scripts/stress_test.py)
    Stress {
        /// 100 years, 16 seeds
        #[arg(long)]
        full: bool,
        #[arg(long)]
        years: Option<usize>,
        #[arg(long)]
        seeds: Option<usize>,
        /// Run only scenarios whose name contains this
        #[arg(long, default_value = "")]
        only: String,
        #[arg(long, default_value = "output/stress")]
        out: String,
        /// Threads to use (default: all cores)
        #[arg(long)]
        workers: Option<usize>,
        /// Print each seed's yearly measures, one line of JSON each, instead
        /// of the table (to compare runs with Python's)
        #[arg(long)]
        each_seed: bool,
    },
}

fn threads(workers: Option<usize>) {
    if let Some(n) = workers {
        rayon::ThreadPoolBuilder::new().num_threads(n.max(1)).build_global().ok();
    }
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Some(Command::Records(args)) => {
            threads(args.workers);
            args.config().and_then(|config| dump::records(&config, &args.forced()?, args.runs))
        }
        Some(Command::Places(args)) => {
            threads(args.workers);
            args.config().and_then(|config| dump::places(&config, &args.forced()?, args.runs))
        }
        Some(Command::Logs(args)) => {
            threads(args.workers);
            args.config().and_then(|config| dump::logs(&config, &args.forced()?, args.runs))
        }
        Some(Command::Bench { quick, country, months, workers, people }) => {
            let sizes: Vec<usize> = match people {
                Some(p) => vec![p],
                None if quick => vec![1_000, 10_000, 100_000],
                None if country => vec![1_000, 10_000, 100_000, 1_000_000, 10_000_000],
                None => vec![1_000, 10_000, 100_000, 1_000_000],
            };
            match bench::run(&sizes, months, workers) {
                Ok(true) => Ok(()),
                Ok(false) => std::process::exit(1),
                Err(e) => Err(e),
            }
        }
        Some(Command::Sanity { full, workers }) => {
            threads(workers);
            match sanity::run(full) {
                Ok(0) => Ok(()),
                Ok(_) => std::process::exit(1),
                Err(e) => Err(e),
            }
        }
        Some(Command::Stress { full, years, seeds, only, out, workers, each_seed }) => {
            threads(workers);
            let years = years.unwrap_or(if full { 100 } else { 40 });
            let seeds = seeds.unwrap_or(if full { 16 } else { 6 });
            if each_seed {
                stress::each_seed(years, seeds, &only)
            } else {
                match stress::run(years, seeds, &only, &out) {
                    Ok(0) => Ok(()),
                    Ok(_) => std::process::exit(1),
                    Err(e) => Err(e),
                }
            }
        }
        Some(Command::Config(args)) => args.config().and_then(|config| {
            serde_json::to_string_pretty(&config).map(|text| println!("{text}")).map_err(|e| e.to_string())
        }),
        Some(Command::Minds(args)) => {
            threads(args.workers);
            minds::run(&args)
        }
        Some(Command::World(args)) => {
            threads(args.workers);
            args.config().and_then(|config| dump::worlds(&config, args.runs))
        }
        None => {
            threads(cli.run.workers);
            run::run(&cli.run)
        }
    };
    if let Err(message) = result {
        eprintln!("econ-sim: {message}");
        std::process::exit(2);
    }
}
