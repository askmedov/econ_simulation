use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut config = match args.iter().position(|a| a == "--config") {
        Some(i) => econ_core::Config::from_file(&args[i + 1]).expect("config"),
        None => econ_core::config::default_config(),
    };
    let seeds: usize = args.iter().position(|a| a == "--runs").map_or(1, |i| args[i + 1].parse().unwrap());
    if let Some(i) = args.iter().position(|a| a == "--months") {
        config.months = args[i + 1].parse().unwrap();
    }
    let first = config.seed;
    let mut out = std::io::BufWriter::new(std::io::stdout());
    for k in 0..seeds {
        let mut c = config.clone();
        c.seed = first + k as i64;
        let mut sim = econ_core::Simulation::new(c).expect("a valid config");
        sim.run();
        for (j, r) in sim.records.iter().enumerate() {
            let flat = r.flat();
            if k == 0 && j == 0 {
                let names: Vec<String> = flat.iter().map(|(n, _)| n.clone()).collect();
                writeln!(out, "seed,{}", names.join(",")).unwrap();
            }
            let row: Vec<String> = flat.iter().map(|(_, v)| v.csv()).collect();
            writeln!(out, "{},{}", first + k as i64, row.join(",")).unwrap();
        }
    }
}
