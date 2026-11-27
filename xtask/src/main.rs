//! Development commands of the Eario repository.
//!
//! Written in Rust so the same commands work on Windows and Linux without a
//! shell. Run `cargo xtask help` for the list.

mod bench;
mod fixtures;

use std::env;
use std::error::Error;
use std::fs;
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
  check        Run the CI checks: format, lints, tests, docs, type check
               (not the browser tests, cargo deny or the minimum Rust version)
  dist         Build the frontend, then the data-explorer example in release mode
  e2e          Build the frontend, the examples and the hub, then run the browser tests
               (Playwright, Chromium). --prebuilt-frontend: keep the frontend
               already in crates/eario-runtime/frontend-dist
  bench        Run the benchmarks and write docs/benchmarks/<date>.md with the
               results, the machine and the commit
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
        "bench" => bench::run(),
        "e2e" => e2e(env::args().skip(2).any(|argument| argument == "--prebuilt-frontend")),
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
    run_with_env(program, arguments, directory, &[])
}

/// Runs a program with extra environment variables and fails when it does.
fn run_with_env(
    program: &str,
    arguments: &[&str],
    directory: &Path,
    variables: &[(&str, &Path)],
) -> TaskResult {
    println!("> {program} {}", arguments.join(" "));
    let status = Command::new(program)
        .args(arguments)
        .envs(variables.iter().copied())
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

/// On Windows npm is a script, which `Command` only finds by its full name.
const NPM: &str = if cfg!(windows) { "npm.cmd" } else { "npm" };

fn npm(arguments: &[&str]) -> TaskResult {
    run(NPM, arguments, &repository_root().join("frontend"))
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

/// Builds the examples and the hub the browser tests start, prepares an empty
/// directory for their temporary hub directories, installs Chromium for
/// Playwright if it is missing, and runs the tests in `frontend/e2e`.
fn e2e(prebuilt_frontend: bool) -> TaskResult {
    if prebuilt_frontend {
        // Still needed: Playwright itself is a frontend dependency.
        if !repository_root().join("frontend/node_modules").exists() {
            setup()?;
        }
    } else {
        frontend()?;
    }
    cargo(&[
        "build",
        "--package",
        "data-explorer",
        "--package",
        "signal-lab",
        "--package",
        "eario-hub",
    ])?;
    npm(&["exec", "--", "playwright", "install", "chromium"])?;
    // Executables land in <target>/debug. A relative CARGO_TARGET_DIR is
    // relative to the repository root, where cargo ran; `join` keeps an
    // absolute one as is.
    let target = env::var_os("CARGO_TARGET_DIR").map_or_else(
        || repository_root().join("target"),
        |directory| repository_root().join(directory),
    );
    let bin_dir = target.join("debug");
    // Each hub test makes its own hub directory in here, installs the examples
    // into it and removes it again. Starting from an empty directory also
    // clears what an interrupted run left behind.
    let hub_root = target.join("e2e-hub");
    if hub_root.exists() {
        fs::remove_dir_all(&hub_root).map_err(|error| {
            format!("could not clear {}: {error}. Stop any hub it holds.", hub_root.display())
        })?;
    }
    fs::create_dir_all(&hub_root)
        .map_err(|error| format!("could not create {}: {error}", hub_root.display()))?;
    run_with_env(
        NPM,
        &["run", "e2e"],
        &repository_root().join("frontend"),
        &[("EARIO_E2E_BIN_DIR", &bin_dir), ("EARIO_E2E_HUB_ROOT", &hub_root)],
    )
}

fn dist() -> TaskResult {
    frontend()?;
    cargo(&["build", "--release", "--package", "data-explorer"])?;
    let extension = if cfg!(windows) { ".exe" } else { "" };
    println!("Standalone executable: target/release/data-explorer{extension}");
    Ok(())
}
