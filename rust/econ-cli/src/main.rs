//! The command line: runs and paired what-ifs as Python's
//! (`python -m econ_sim`), and tools for checking the engine.

mod bench;
mod compare;
mod dump;
mod options;
mod report;
mod run;
mod sanity;
mod starts;
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
    /// What a switch changes (minds, harvest hands, refugees): each seed run
    /// without and with it, without and with the forced events
    Compare {
        switch: compare::Switch,
        #[command(flatten)]
        run: RunArgs,
    },
    /// The region run from several starting points (after a plague, crowded,
    /// short of coin...), each from the same seeds, side by side for up to
    /// ten years; with --event, what the event does from each
    Starts {
        /// Only these starting points, e.g. --only standard,crowded
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,
        /// List the starting points and exit
        #[arg(long)]
        list: bool,
        #[command(flatten)]
        run: RunArgs,
    },
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
        /// Time this config instead (a region on a map, say): set-up, a
        /// month on one thread and two runs at once, and memory
        #[arg(long, value_name = "FILE")]
        config: Option<String>,
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

/// Output piped into a program that stops reading (`econ-sim | head`) ends
/// the run quietly, as any command-line tool does, instead of panicking.
#[cfg(unix)]
fn quiet_on_broken_pipe() {
    extern "C" {
        fn signal(signum: i32, handler: usize) -> usize;
    }
    const SIGPIPE: i32 = 13;
    const DEFAULT: usize = 0;
    // SAFETY: restoring the default action of one signal at start-up, before any thread exists.
    unsafe {
        signal(SIGPIPE, DEFAULT);
    }
}

#[cfg(not(unix))]
fn quiet_on_broken_pipe() {}

/// A month makes many arrays of a value per person: 80 MB each at ten
/// million people. glibc gives a block that big back to the system when it
/// is freed and asks for fresh pages for the next, and on a virtual machine
/// each first touch of a page costs a few microseconds: about a second a
/// month at ten million. Keeping big blocks on the heap, to be reused, and
/// one heap for every thread (each thread's own heap would hand big blocks
/// to the system all the same) cuts the page faults to a fifth in paired
/// runs.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn keep_big_blocks() {
    extern "C" {
        fn mallopt(param: i32, value: i32) -> i32;
    }
    const M_MMAP_THRESHOLD: i32 = -3;
    const M_ARENA_MAX: i32 = -8;
    // SAFETY: malloc settings changed at start-up, before any thread exists.
    unsafe {
        mallopt(M_MMAP_THRESHOLD, i32::MAX);
        mallopt(M_ARENA_MAX, 1);
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn keep_big_blocks() {}

fn main() {
    quiet_on_broken_pipe();
    keep_big_blocks();
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
        Some(Command::Bench { config: Some(path), months, .. }) => {
            econ_core::Config::from_file(&path).map_err(|e| e.to_string()).and_then(|c| bench::run_config(c, months))
        }
        Some(Command::Bench { quick, country, months, workers, people, config: None }) => {
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
        Some(Command::Starts { list: true, .. }) => {
            starts::list();
            Ok(())
        }
        Some(Command::Starts { only, run, .. }) => {
            threads(run.workers);
            starts::run(&run, &only)
        }
        Some(Command::Minds(args)) => {
            threads(args.workers);
            compare::run(&args, compare::Switch::Minds)
        }
        Some(Command::Compare { switch, run }) => {
            threads(run.workers);
            compare::run(&run, switch)
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
