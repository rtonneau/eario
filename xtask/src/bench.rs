//! `cargo xtask bench`: runs the benchmarks and records the results, with the
//! machine and the commit they were measured on, in a dated Markdown file.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use sysinfo::System;

use crate::{TaskResult, cargo, repository_root};

/// One benchmark: its identifier and its mean time with the 95 % interval.
#[derive(Debug, PartialEq)]
struct Estimate {
    name: String,
    mean_ns: f64,
    lower_ns: f64,
    upper_ns: f64,
}

/// What the numbers depend on, besides the code.
struct Machine {
    cpu: String,
    physical_cores: String,
    logical_cores: usize,
    memory: String,
    os: String,
    rustc: String,
}

/// The commit measured, and whether the working tree differed from it.
struct Revision {
    commit: String,
    dirty: bool,
}

/// Runs `cargo bench -p eario-data` and writes `docs/benchmarks/<date>.md`.
pub fn run() -> TaskResult {
    let started = SystemTime::now();
    cargo(&["bench", "--package", "eario-data"])?;

    let root = repository_root();
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| root.join("target"), |directory| root.join(directory));
    let estimates = collect(&target.join("criterion"), started)?;
    if estimates.is_empty() {
        return Err("criterion wrote no result: check the output of `cargo bench` above".into());
    }

    let date = civil_date(started.duration_since(UNIX_EPOCH)?.as_secs());
    let report = render(&date, &machine()?, &revision()?, &estimates);
    let directory = root.join("docs/benchmarks");
    fs::create_dir_all(&directory)?;
    let path = directory.join(format!("{date}.md"));
    fs::write(&path, report)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    println!("Benchmark results: docs/benchmarks/{date}.md");
    Ok(())
}

/// Reads the estimates criterion wrote since `since`, sorted by name. Older
/// directories under `criterion` belong to earlier runs or removed benchmarks.
fn collect(
    criterion: &Path,
    since: SystemTime,
) -> Result<Vec<Estimate>, Box<dyn std::error::Error>> {
    let entries = fs::read_dir(criterion)
        .map_err(|error| format!("cannot read {}: {error}", criterion.display()))?;
    let mut estimates = Vec::new();
    for entry in entries {
        let latest = entry?.path().join("new");
        let estimates_file = latest.join("estimates.json");
        let Ok(metadata) = fs::metadata(&estimates_file) else {
            // `report` and other directories without results.
            continue;
        };
        if metadata.modified()? < since {
            continue;
        }
        let benchmark = read_json(&latest.join("benchmark.json"))?;
        let name = benchmark["full_id"]
            .as_str()
            .ok_or_else(|| format!("no full_id in {}", latest.join("benchmark.json").display()))?;
        estimates.push(parse_estimate(name, &read_json(&estimates_file)?)?);
    }
    estimates.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(estimates)
}

fn read_json(path: &Path) -> Result<Value, Box<dyn std::error::Error>> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    Ok(serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?)
}

/// Extracts the mean and its confidence interval from criterion's
/// `estimates.json`.
fn parse_estimate(name: &str, estimates: &Value) -> Result<Estimate, String> {
    let mean = &estimates["mean"];
    let number = |value: &Value, field: &str| {
        value.as_f64().ok_or_else(|| format!("benchmark {name}: no number for {field}"))
    };
    Ok(Estimate {
        name: name.to_owned(),
        mean_ns: number(&mean["point_estimate"], "mean")?,
        lower_ns: number(&mean["confidence_interval"]["lower_bound"], "lower bound")?,
        upper_ns: number(&mean["confidence_interval"]["upper_bound"], "upper bound")?,
    })
}

fn machine() -> Result<Machine, Box<dyn std::error::Error>> {
    let mut system = System::new();
    system.refresh_cpu_all();
    system.refresh_memory();
    let cpu = system.cpus().first().map_or("unknown", |cpu| cpu.brand()).trim().to_owned();
    // Memory in bytes; shown in GiB, rounded, as the machine is sold.
    let memory = format!("{:.0} GiB", system.total_memory() as f64 / f64::from(1u32 << 30));
    let rustc = command_output("rustc", &["--version"])?;
    Ok(Machine {
        cpu,
        physical_cores: System::physical_core_count()
            .map_or_else(|| "unknown".to_owned(), |count| count.to_string()),
        logical_cores: system.cpus().len(),
        memory,
        os: System::long_os_version().unwrap_or_else(|| std::env::consts::OS.to_owned()),
        rustc,
    })
}

fn revision() -> Result<Revision, Box<dyn std::error::Error>> {
    Ok(Revision {
        commit: command_output("git", &["rev-parse", "--short=12", "HEAD"])?,
        dirty: !command_output("git", &["status", "--porcelain"])?.is_empty(),
    })
}

/// Runs a program in the repository root and returns its trimmed standard output.
fn command_output(program: &str, arguments: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new(program)
        .args(arguments)
        .current_dir(repository_root())
        .output()
        .map_err(|error| format!("could not start `{program}`: {error}"))?;
    if !output.status.success() {
        return Err(
            format!("`{program} {}` failed with {}", arguments.join(" "), output.status).into()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// `YYYY-MM-DD` (UTC) of a Unix time, by the days-to-civil algorithm of
/// Howard Hinnant (proleptic Gregorian calendar).
fn civil_date(unix_seconds: u64) -> String {
    let days = (unix_seconds / 86_400) as i64 + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// A duration in nanoseconds, with three significant digits and a fitting unit.
fn format_duration(nanoseconds: f64) -> String {
    let (value, unit) = if nanoseconds >= 1e9 {
        (nanoseconds / 1e9, "s")
    } else if nanoseconds >= 1e6 {
        (nanoseconds / 1e6, "ms")
    } else if nanoseconds >= 1e3 {
        (nanoseconds / 1e3, "µs")
    } else {
        (nanoseconds, "ns")
    };
    let decimals = if value >= 100.0 {
        0
    } else if value >= 10.0 {
        1
    } else {
        2
    };
    format!("{value:.decimals$} {unit}")
}

fn render(date: &str, machine: &Machine, revision: &Revision, estimates: &[Estimate]) -> String {
    let dirty = if revision.dirty { " (with uncommitted changes)" } else { "" };
    let mut report = format!(
        "# Benchmarks, {date}\n\n\
         Produced by `cargo xtask bench` (`cargo bench -p eario-data`, criterion).\n\
         Each time is the mean of criterion's samples, with its 95 % confidence\n\
         interval. Compare results only between runs on the same machine.\n\n\
         | | |\n| --- | --- |\n\
         | Commit | `{}`{dirty} |\n\
         | CPU | {} |\n\
         | Cores | {} physical, {} logical |\n\
         | Memory | {} |\n\
         | Operating system | {} |\n\
         | Compiler | {} |\n\n\
         | Benchmark | Mean | 95 % interval |\n| --- | ---: | ---: |\n",
        revision.commit,
        machine.cpu,
        machine.physical_cores,
        machine.logical_cores,
        machine.memory,
        machine.os,
        machine.rustc,
    );
    for estimate in estimates {
        // Writing to a String cannot fail.
        let _ = writeln!(
            report,
            "| `{}` | {} | {} – {} |",
            estimate.name,
            format_duration(estimate.mean_ns),
            format_duration(estimate.lower_ns),
            format_duration(estimate.upper_ns),
        );
    }
    report
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn civil_date_of_known_days() {
        assert_eq!(civil_date(0), "1970-01-01");
        assert_eq!(civil_date(951_782_400), "2000-02-29");
        assert_eq!(civil_date(1_798_761_599), "2026-12-31");
    }

    #[test]
    fn durations_use_a_fitting_unit() {
        assert_eq!(format_duration(512.0), "512 ns");
        assert_eq!(format_duration(1_234.0), "1.23 µs");
        assert_eq!(format_duration(45_600_000.0), "45.6 ms");
        assert_eq!(format_duration(2_500_000_000.0), "2.50 s");
    }

    #[test]
    fn estimate_is_read_from_criterion_output() {
        let estimates = json!({
            "mean": {
                "point_estimate": 1500.0,
                "confidence_interval": { "lower_bound": 1400.0, "upper_bound": 1600.0 }
            }
        });
        assert_eq!(
            parse_estimate("filter", &estimates),
            Ok(Estimate {
                name: "filter".into(),
                mean_ns: 1500.0,
                lower_ns: 1400.0,
                upper_ns: 1600.0
            })
        );
        assert!(parse_estimate("filter", &json!({})).is_err());
    }

    #[test]
    fn report_lists_the_machine_and_every_benchmark() {
        let machine = Machine {
            cpu: "Test CPU".into(),
            physical_cores: "4".into(),
            logical_cores: 8,
            memory: "16 GiB".into(),
            os: "TestOS 1".into(),
            rustc: "rustc 1.99.1".into(),
        };
        let revision = Revision { commit: "abc123".into(), dirty: true };
        let estimates = [
            Estimate { name: "a".into(), mean_ns: 2e6, lower_ns: 1.9e6, upper_ns: 2.1e6 },
            Estimate { name: "b".into(), mean_ns: 3e3, lower_ns: 2.9e3, upper_ns: 3.1e3 },
        ];
        let report = render("2026-11-26", &machine, &revision, &estimates);
        assert!(report.starts_with("# Benchmarks, 2026-11-26\n"));
        assert!(report.contains("| Commit | `abc123` (with uncommitted changes) |"));
        assert!(report.contains("| Cores | 4 physical, 8 logical |"));
        assert!(report.contains("| `a` | 2.00 ms | 1.90 ms – 2.10 ms |"));
        assert!(report.contains("| `b` | 3.00 µs | 2.90 µs – 3.10 µs |"));
    }
}
