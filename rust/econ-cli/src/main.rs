//! The command line: runs and paired what-ifs as Python's
//! (`python -m econ_sim`), and tools for checking the engine.

mod dump;
mod options;

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
    /// The starting state of each run, one line of JSON each
    World(RunArgs),
    /// Each run's measures per place, one line of JSON each (with --event, both arms)
    Places(RunArgs),
    /// Each run's log of notable happenings (with the forced events)
    Logs(RunArgs),
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
        Some(Command::World(args)) => {
            threads(args.workers);
            args.config().and_then(|config| dump::worlds(&config, args.runs))
        }
        None => Err("running and reporting what-ifs comes with the runner milestone; use `records`".into()),
    };
    if let Err(message) = result {
        eprintln!("econ-sim: {message}");
        std::process::exit(2);
    }
}
