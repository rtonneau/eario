//! The launch contract on the real `signal-lab` executable, against its own
//! temporary hub directory given in `EARIO_HUB_DIR`. signal-lab gives its
//! first two arguments to two datasets, unlike data-explorer.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use eario::core::{LAUNCH_PROTOCOL_VERSION, Manifest, RunFile};
use eario::runtime::hub_dir::{self, HubDir};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const EXECUTABLE: &str = env!("CARGO_BIN_EXE_signal-lab");
const SLUG: &str = "signal-lab";
/// Longest wait for the child to write its run file, or to exit.
const TIMEOUT: Duration = Duration::from_secs(30);

/// The executable with `arguments`, the hub directory `hub` and no terminal.
fn command(hub: &Path, arguments: &[&str]) -> Command {
    let mut command = Command::new(EXECUTABLE);
    command
        .args(arguments)
        .env("EARIO_HUB_DIR", hub)
        .env("EARIO_NO_BROWSER", "1")
        .env_remove("EARIO_PORT")
        .stdin(Stdio::null());
    command
}

fn execute(hub: &Path, arguments: &[&str]) -> Result<Output, std::io::Error> {
    command(hub, arguments).output()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A child process, killed when dropped so a failed assertion leaves none
/// running.
struct Spawned(Child);

impl Drop for Spawned {
    fn drop(&mut self) {
        // Both fail once the process has exited, which is what is wanted.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Spawned {
    /// Starts the executable in child mode with the run file `run_file`.
    fn child(hub: &HubDir, run_file: &Path) -> Result<Self, std::io::Error> {
        let mut command = command(hub.root(), &["--eario-child"]);
        command.arg(run_file).stdout(Stdio::piped()).stderr(Stdio::piped());
        Ok(Self(command.spawn()?))
    }

    /// Waits for the run file of `slug`, or fails with the child's error
    /// output when it exits first or [`TIMEOUT`] passes.
    fn announced(
        &mut self,
        hub: &HubDir,
        slug: &str,
    ) -> Result<RunFile, Box<dyn std::error::Error>> {
        let started = Instant::now();
        loop {
            if let Some(announced) = hub.read_run_file(slug)? {
                return Ok(announced);
            }
            if self.0.try_wait()?.is_some() {
                let (_, _, stderr) = self.exit()?;
                return Err(format!("the child exited without a run file: {stderr}").into());
            }
            if started.elapsed() > TIMEOUT {
                return Err("the child wrote no run file in time".into());
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    /// Waits up to [`TIMEOUT`] for the process to exit; returns its status,
    /// standard output and standard error.
    fn exit(&mut self) -> Result<(ExitStatus, String, String), Box<dyn std::error::Error>> {
        let started = Instant::now();
        let status = loop {
            if let Some(status) = self.0.try_wait()? {
                break status;
            }
            if started.elapsed() > TIMEOUT {
                return Err("the child did not exit in time".into());
            }
            thread::sleep(Duration::from_millis(50));
        };
        let mut stdout = String::new();
        let mut stderr = String::new();
        if let Some(mut pipe) = self.0.stdout.take() {
            pipe.read_to_string(&mut stdout)?;
        }
        if let Some(mut pipe) = self.0.stderr.take() {
            pipe.read_to_string(&mut stderr)?;
        }
        Ok((status, stdout, stderr))
    }
}

/// Sends `POST /api/shutdown` with the session token; returns the status.
fn shutdown(address: &str, token: &str) -> Result<u16, Box<dyn std::error::Error>> {
    let mut stream = TcpStream::connect(address)?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    write!(
        stream,
        "POST /api/shutdown HTTP/1.1\r\nHost: {address}\r\nx-eario-token: {token}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    Ok(response.split_whitespace().nth(1).ok_or("response without a status")?.parse()?)
}

#[test]
fn manifest_is_printed_without_loading_any_file() -> TestResult {
    let hub = tempfile::tempdir()?;
    let output = execute(hub.path(), &["missing.csv", "--eario-manifest"])?;
    assert!(output.status.success(), "{}", text(&output.stderr));

    let manifest: Manifest = serde_json::from_slice(&output.stdout)?;
    assert_eq!(manifest.name, "Signal Lab");
    assert_eq!(manifest.slug, SLUG);
    assert_eq!(manifest.version.as_deref(), Some(env!("CARGO_PKG_VERSION")));
    assert_eq!(manifest.protocol, LAUNCH_PROTOCOL_VERSION);
    Ok(())
}

#[test]
fn install_and_uninstall_need_yes_without_a_terminal() -> TestResult {
    let hub_root = tempfile::tempdir()?;
    let hub = HubDir::at(hub_root.path());

    let output = execute(hub.root(), &["--eario-install"])?;
    assert!(!output.status.success());
    assert!(text(&output.stderr).contains("--yes"), "{}", text(&output.stderr));
    assert!(!hub.registry_path().exists());

    let output = execute(hub.root(), &["--eario-install", "--yes"])?;
    assert!(output.status.success(), "{}", text(&output.stderr));
    let registry = hub.read_registry()?;
    let entry = registry.get(SLUG).ok_or("no registry entry")?;
    assert!(entry.path.starts_with(hub.bin_dir(SLUG)?), "{}", entry.path.display());
    assert_eq!(hub_dir::file_digest(&entry.path)?, (entry.size, entry.sha256.clone()));
    assert_eq!(hub_dir::file_digest(Path::new(EXECUTABLE))?.1, entry.sha256);

    let output = execute(hub.root(), &["--eario-uninstall", "--yes"])?;
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(hub.read_registry()?.dashboards.is_empty());
    assert!(!hub.bin_dir(SLUG)?.exists());
    Ok(())
}

#[test]
fn child_mode_ignores_the_contract_arguments_and_stops_on_shutdown() -> TestResult {
    let hub_root = tempfile::tempdir()?;
    let hub = HubDir::at(hub_root.path());
    let run_file = hub.run_file_path(SLUG)?;

    // signal-lab loads its first argument into `series` and its second into
    // `groups`: here `--eario-child` and the run file, which must be dropped.
    let mut child = Spawned::child(&hub, &run_file)?;
    let announced = child.announced(&hub, SLUG)?;
    assert!(hub_dir::probe_blocking(&announced.address, &announced.token)?);

    let output = execute(hub.root(), &["--eario-install", "--yes"])?;
    assert!(!output.status.success(), "install must be refused while the dashboard runs");
    assert!(text(&output.stderr).contains("is running"), "{}", text(&output.stderr));

    assert_eq!(shutdown(&announced.address, &announced.token)?, 204);
    let (status, stdout, stderr) = child.exit()?;
    assert!(status.success(), "{stderr}");
    assert!(stdout.is_empty(), "nothing on stdout: {stdout}");
    assert!(!run_file.exists(), "the run file is removed");
    assert!(hub.log_path(SLUG)?.exists());
    Ok(())
}
