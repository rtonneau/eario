//! Development commands of the Eario repository.
//!
//! Written in Rust so the same commands work on Windows and Linux without a
//! shell. Run `cargo xtask help` for the list.

mod fixtures;

use std::env;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

type TaskResult = Result<(), Box<dyn Error>>;

const HELP: &str = "\
Usage: cargo xtask <command>

Commands:
  setup        Install the frontend dependencies (npm ci)
  frontend     Build the frontend into crates/eario-runtime/frontend-dist
  gen-types    Regenerate the TypeScript bindings of the contract types
  fixtures     Regenerate the test data in fixtures/
  large-csv    Generate fixtures/generated/large.csv (1,000,000 rows, not tracked)
  test         Run the Rust and frontend tests
  check        Run every check CI runs: format, lints, tests, type check
  dist         Build the frontend, then the data-explorer example in release mode
  help         Show this message
";

fn main() -> ExitCode {
    let command = env::args().nth(1).unwrap_or_else(|| "help".to_owned());
    let result = match command.as_str() {
        "setup" => setup(),
        "frontend" => frontend(),
        "gen-types" => gen_types(),
        "fixtures" => fixtures::write_all(&repository_root().join("fixtures")),
        "large-csv" => fixtures::write_large_csv(&repository_root().join("fixtures/generated")),
        "test" => test(),
        "check" => check(),
        "dist" => dist(),
        "help" | "--help" | "-h" => {
            print!("{HELP}");
            Ok(())
        }
        other => Err(format!("unknown command `{other}`\n\n{HELP}").into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

/// The repository root: the parent of the directory holding this crate.
fn repository_root() -> PathBuf {
    let xtask = Path::new(env!("CARGO_MANIFEST_DIR"));
    xtask.parent().unwrap_or(xtask).to_path_buf()
}

/// Runs a program and fails when it does.
fn run(program: &str, arguments: &[&str], directory: &Path) -> TaskResult {
    println!("> {program} {}", arguments.join(" "));
    let status = Command::new(program)
        .args(arguments)
        .current_dir(directory)
        .status()
        .map_err(|error| format!("could not start `{program}`: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("`{program} {}` failed with {status}", arguments.join(" ")).into())
    }
}

fn cargo(arguments: &[&str]) -> TaskResult {
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    run(&cargo, arguments, &repository_root())
}

fn npm(arguments: &[&str]) -> TaskResult {
    // On Windows npm is a script, which `Command` only finds by its full name.
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    run(npm, arguments, &repository_root().join("frontend"))
}

fn setup() -> TaskResult {
    npm(&["ci"])
}

fn frontend() -> TaskResult {
    if !repository_root().join("frontend/node_modules").exists() {
        setup()?;
    }
    npm(&["run", "build"])
}

fn gen_types() -> TaskResult {
    // ts-rs writes one file per type while running the generated export
    // tests. The destination is set in .cargo/config.toml.
    cargo(&["test", "--package", "eario-core", "--features", "ts", "export_bindings"])
}

fn test() -> TaskResult {
    cargo(&["test", "--workspace", "--all-features"])?;
    npm(&["run", "test"])
}

fn check() -> TaskResult {
    cargo(&["fmt", "--all", "--check"])?;
    cargo(&["clippy", "--workspace", "--all-targets", "--all-features", "--", "-D", "warnings"])?;
    cargo(&["test", "--workspace", "--all-features"])?;
    cargo(&["doc", "--workspace", "--no-deps", "--all-features"])?;
    npm(&["run", "check"])?;
    npm(&["run", "test"])
}

fn dist() -> TaskResult {
    frontend()?;
    cargo(&["build", "--release", "--package", "data-explorer"])?;
    let extension = if cfg!(windows) { ".exe" } else { "" };
    println!("Standalone executable: target/release/data-explorer{extension}");
    Ok(())
}
