//! The command line, as Python's `tests/test_cli.py` checks it.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A fresh folder for one test's files.
fn folder(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("econ-sim-cli-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn econ_sim(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_econ-sim")).args(args).output().unwrap()
}

/// Runs with `--out` set to `out`, expecting success; the printed report.
fn run(args: &[&str], out: &Path) -> String {
    let mut all = args.to_vec();
    all.extend(["--out", out.to_str().unwrap()]);
    let output = econ_sim(&all);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap()
}

fn files(out: &Path) -> Vec<String> {
    let mut names: Vec<String> =
        std::fs::read_dir(out).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    names.sort();
    names
}

#[test]
fn the_standard_run_is_a_region_of_a_million_people() {
    let out = folder("standard");
    let report = run(&["--months", "1"], &out);
    assert!(report.starts_with("Region of 2460 villages"));
    assert!(report.contains("98 market towns"));
    assert_eq!(files(&out), ["log.json", "log.txt", "places.csv", "run1.csv", "summary.csv"]);
}

#[test]
fn a_smaller_standard_region_of_100000_people_is_at_hand() {
    let out = folder("standard-100k");
    let report = run(&["--size", "100k", "--months", "2"], &out);
    assert!(report.starts_with("Region of 240 villages"));
    assert!(report.contains("10 market towns"));
}

#[test]
fn a_single_village_run_writes_its_files() {
    let out = folder("single");
    let report = run(&["--population", "1000", "--months", "12"], &out);
    assert!(report.starts_with("Village of 1000 people"));
    assert!(report.contains("After 12 months"));
    assert_eq!(files(&out), ["log.json", "log.txt", "run1.csv", "summary.csv"]);
}

#[test]
fn an_event_comparison_reports_the_effect() {
    let out = folder("event");
    let report = run(&["--population", "1000", "--months", "18", "--runs", "5", "--event", "drought@4"], &out);
    assert!(report.contains("Effect of drought in month 4 over 18 months"));
    assert!(report.contains("Extra deaths"));
    assert!(out.join("run1_with_event.csv").exists());
}

#[test]
fn a_badly_written_event_is_rejected() {
    let out = folder("bad-event");
    for event in ["drought", "drought@", "drought@soon"] {
        let output = econ_sim(&["--event", event, "--out", out.to_str().unwrap()]);
        assert!(!output.status.success(), "{event} was accepted");
    }
}

#[test]
fn an_unknown_event_is_rejected() {
    let out = folder("unknown-event");
    let output = econ_sim(&["--event", "meteor@3", "--out", out.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown village event"));
}

#[test]
fn the_events_can_be_listed() {
    let output = econ_sim(&["--list-events"]);
    assert!(String::from_utf8_lossy(&output.stdout).contains("drought"));
}

#[test]
fn a_council_can_form_during_the_run() {
    let out = folder("council");
    let report = run(&["--population", "1000", "--months", "8", "--council-forms-later"], &out);
    assert!(report.contains("A council forms once") && report.contains("formed a council"));
}

#[test]
fn drought_runs_can_be_switched_off() {
    let out = folder("drought-runs");
    assert!(run(&["--population", "1000", "--months", "6", "--no-drought-runs"], &out).contains("Village of"));
}

#[test]
fn a_region_what_if_is_reported_by_place() {
    let out = folder("region");
    let report = run(
        &[
            "--region",
            "8",
            "--population",
            "120",
            "--months",
            "6",
            "--runs",
            "2",
            "--workers",
            "1",
            "--event",
            "drought@2@3,3,4",
        ],
        &out,
    );
    assert!(report.starts_with("Region of 8 villages"));
    assert!(report.contains("By where the forced events struck") && report.contains("By terrain"));
    let places = std::fs::read_to_string(out.join("places.csv")).unwrap();
    let rows: Vec<&str> = places.lines().collect();
    assert_eq!(rows.len(), 10); // 8 villages and their town
    assert!(rows[0].starts_with("village,kind,terrain,x_km"));
    assert!(report.contains("By kind of place") && report.contains("market towns"));
}

#[test]
fn a_region_gives_each_village_land_for_its_size() {
    let out = folder("region-land");
    run(&["--region", "6", "--population", "120", "--months", "1", "--workers", "1"], &out);
    let places = std::fs::read_to_string(out.join("places.csv")).unwrap();
    for row in places.lines().skip(1).map(|l| l.split(',').collect::<Vec<_>>()).filter(|r| r[1] == "village") {
        let people: i64 = row[6].parse().unwrap();
        assert!((80..=400).contains(&people), "a village of {people}");
    }
    let output = econ_sim(&["--region", "6", "--land", "50", "--out", out.to_str().unwrap()]);
    assert!(!output.status.success());
}

#[test]
fn a_badly_written_place_is_rejected() {
    let out = folder("bad-place");
    for event in ["drought@4@1,2", "drought@4@east"] {
        let output = econ_sim(&["--region", "4", "--event", event, "--out", out.to_str().unwrap()]);
        assert!(!output.status.success(), "{event} was accepted");
    }
}

#[test]
fn a_region_kept_as_a_config_runs_the_same_again() {
    let out = folder("config");
    let output = econ_sim(&["config", "--region", "10", "--map-seed", "3"]);
    assert!(output.status.success());
    let kept = out.join("region.json");
    std::fs::write(&kept, &output.stdout).unwrap();
    let laid_out = run(&["--region", "10", "--map-seed", "3", "--months", "3"], &out);
    let again = run(&["--config", kept.to_str().unwrap(), "--months", "3"], &out);
    assert_eq!(laid_out, again);
}

#[test]
fn starting_points_are_compared_side_by_side() {
    let out = folder("starts");
    let report = run(&["starts", "--size", "100k", "--months", "12", "--only", "standard,crowded"], &out);
    assert!(report.contains("2 starting points") && report.contains("Where each start leads"));
    assert!(report.lines().any(|l| l.starts_with("crowded ")));
    assert!(report.contains("People at the end of each year"));
    let csv = std::fs::read_to_string(out.join("starts.csv")).unwrap();
    assert_eq!(csv.lines().count(), 3);
    let output = econ_sim(&["starts", "--list"]);
    assert!(String::from_utf8_lossy(&output.stdout).contains("after-plague"));
}

#[test]
fn starting_points_are_compared_over_ten_years_at_most() {
    let out = folder("starts-long");
    let output = econ_sim(&["starts", "--size", "100k", "--months", "180", "--out", out.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("10 years"));
}

#[test]
fn a_run_starts_from_a_starting_point_with_changes_of_its_own() {
    let out = folder("start");
    let config = |args: &[&str]| -> serde_json::Value {
        let output = econ_sim(&[&["config"], args].concat());
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        serde_json::from_slice(&output.stdout).unwrap()
    };
    let plague = config(&["--population", "1000", "--start", "after-plague"]);
    assert_eq!(plague["villages"][0]["population"], 667);
    let changed = config(&["--population", "1000", "--set", "lord.demesne=0.4", "--scale", "villages.land=2"]);
    assert_eq!(changed["lord"]["demesne"], 0.4);
    assert_eq!(changed["villages"][0]["land"], 700.0);
    for bad in [&["--start", "golden-age"][..], &["--set", "lord.demesnee=0.4"][..]] {
        let output =
            econ_sim(&[&["--population", "1000", "--months", "1", "--out", out.to_str().unwrap()], bad].concat());
        assert!(!output.status.success(), "{bad:?} was accepted");
    }
}

#[test]
fn the_log_goes_year_by_year_with_the_most_important_first() {
    let out = folder("log");
    run(&["--size", "100k", "--months", "24", "--event", "drought@4"], &out);
    let log = std::fs::read_to_string(out.join("log.txt")).unwrap();
    assert!(log.contains("Without forced events") && log.contains("With forced events"));
    assert!(log.contains("Year 1 (Jan to Dec)") && log.contains("Year 2") && log.contains("In brief:"));
    assert!(log.contains("[1] Apr: Drought in"), "the forced drought is in the year's brief");
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("log.json")).unwrap()).unwrap();
    let forced = json["with_forced_events"].as_array().unwrap();
    assert!(forced.iter().any(|e| e["rank"] == 1 && e["what"].as_str().unwrap().contains("(forced)")));
    // Asked for the first rank only, the months tell nothing below it.
    let output = econ_sim(&["logs", "--size", "100k", "--months", "24", "--event", "drought@4", "--log-level", "1"]);
    let told = String::from_utf8_lossy(&output.stdout);
    assert!(told.contains("[1]") && !told.lines().any(|l| l.starts_with("    [3]") || l.starts_with("    [4]")));
    let output = econ_sim(&["logs", "--population", "1000", "--months", "12", "--flat"]);
    assert!(String::from_utf8_lossy(&output.stdout).contains("Year 1, "));
}

/// The data a viewer page carries.
fn page_data(html: &str) -> serde_json::Value {
    let start = "<script id=\"run\" type=\"application/json\">";
    let from = html.find(start).expect("the page has its data") + start.len();
    let to = from + html[from..].find("</script>").unwrap();
    serde_json::from_str(&html[from..to]).unwrap()
}

#[test]
fn the_viewer_writes_one_page_with_the_run_and_its_pair_in_it() {
    let out = folder("view");
    let page = out.join("drought.html");
    let report = run(
        &["view", "--size", "100k", "--months", "6", "--event", "drought@4", "--page", page.to_str().unwrap()],
        &out,
    );
    assert!(report.contains("Wrote"));
    let html = std::fs::read_to_string(&page).unwrap();
    assert!(html.starts_with("<!doctype html>"));
    assert!(html.contains("<title>Drought in month 4</title>"));
    let data = page_data(&html);
    assert_eq!(data["places"]["name"].as_array().unwrap().len(), 250);
    assert_eq!(data["months"], 6);
    assert_eq!(data["arms"][1]["label"], "With the drought");
    assert_eq!(data["records"][1]["month_number"].as_array().unwrap().len(), 6);
    assert_eq!(files(&out), ["drought.html"], "nothing else is written");
}

#[test]
fn a_big_region_can_be_viewed_by_its_market_towns() {
    let out = folder("view-by-town");
    run(&["view", "--size", "100k", "--months", "3", "--by-town"], &out);
    let data = page_data(&std::fs::read_to_string(out.join("viewer.html")).unwrap());
    assert_eq!(data["by_town"], true);
    assert_eq!(data["places"]["name"].as_array().unwrap().len(), 10);
    let people: i64 = data["places"]["people"].as_array().unwrap().iter().map(|p| p.as_i64().unwrap()).sum();
    assert_eq!(people, data["about"]["people"].as_i64().unwrap(), "every village is in its town's countryside");
    assert_eq!(data["arms"].as_array().unwrap().len(), 1, "without forced events there is one run");
}

#[test]
fn the_viewer_shows_one_run_of_a_region_on_a_map() {
    let output = econ_sim(&["view", "--population", "1000", "--months", "3"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("a region on a map"));
    let output = econ_sim(&["view", "--size", "100k", "--months", "3", "--runs", "2"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--seed"));
}
