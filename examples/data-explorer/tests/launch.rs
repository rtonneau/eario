//! The launch contract on the real `data-explorer` executable: manifest,
//! install, uninstall and child mode, each against its own temporary hub
//! directory given in `EARIO_HUB_DIR`.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use eario::core::{LAUNCH_PROTOCOL_VERSION, Manifest, RunFile, Source};
use eario::runtime::hub_dir::{self, HubDir};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const EXECUTABLE: &str = env!("CARGO_BIN_EXE_data-explorer");
const SLUG: &str = "data-explorer";
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

/// Sends one request with the session token; returns the status and body.
fn request(
    address: &str,
    method: &str,
    path: &str,
    token: &str,
) -> Result<(u16, String), Box<dyn std::error::Error>> {
    let mut stream = TcpStream::connect(address)?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nx-eario-token: {token}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let status = response.split_whitespace().nth(1).ok_or("response without a status")?.parse()?;
    let body = response.split_once("\r\n\r\n").map(|(_, body)| body.to_owned()).unwrap_or_default();
    Ok((status, body))
}

#[test]
fn manifest_is_printed_without_loading_any_file() -> TestResult {
    let hub = tempfile::tempdir()?;
    let output = execute(hub.path(), &["--eario-manifest", "missing.csv", "also-missing.csv"])?;
    assert!(output.status.success(), "{}", text(&output.stderr));

    let manifest: Manifest = serde_json::from_slice(&output.stdout)?;
    assert_eq!(manifest.name, "Data Explorer");
    assert_eq!(manifest.slug, SLUG);
    assert_eq!(manifest.version.as_deref(), Some(env!("CARGO_PKG_VERSION")));
    assert_eq!(manifest.protocol, LAUNCH_PROTOCOL_VERSION);
    assert_eq!(std::fs::read_dir(hub.path())?.count(), 0, "the hub directory is not touched");
    Ok(())
}

#[test]
fn install_without_a_terminal_needs_yes() -> TestResult {
    let hub = tempfile::tempdir()?;
    let output = execute(hub.path(), &["--eario-install"])?;
    assert!(!output.status.success());
    let error = text(&output.stderr);
    assert!(error.contains("--yes") && error.contains(SLUG), "{error}");
    assert!(!HubDir::at(hub.path()).registry_path().exists());
    Ok(())
}

#[test]
fn install_copies_and_records_the_executable_and_uninstall_removes_it() -> TestResult {
    let hub_root = tempfile::tempdir()?;
    let hub = HubDir::at(hub_root.path());

    let output = execute(hub.root(), &["--eario-install", "--yes"])?;
    assert!(output.status.success(), "{}", text(&output.stderr));
    let registry = hub.read_registry()?;
    assert_eq!(registry.dashboards.len(), 1);
    let entry = registry.get(SLUG).ok_or("no registry entry")?;
    assert!(entry.path.is_absolute(), "{}", entry.path.display());
    assert!(entry.path.starts_with(hub.bin_dir(SLUG)?), "{}", entry.path.display());
    assert_eq!(hub_dir::file_digest(&entry.path)?, (entry.size, entry.sha256.clone()));
    assert_eq!(hub_dir::file_digest(Path::new(EXECUTABLE))?.1, entry.sha256, "an exact copy");
    assert_eq!(entry.manifest.slug, SLUG);
    assert_eq!(entry.manifest.version.as_deref(), Some(env!("CARGO_PKG_VERSION")));

    let output = execute(hub.root(), &["--eario-install", "--yes"])?;
    assert!(output.status.success(), "a reinstall replaces the copy: {}", text(&output.stderr));
    assert_eq!(hub.read_registry()?.dashboards.len(), 1);

    let output = execute(hub.root(), &["--eario-uninstall"])?;
    assert!(!output.status.success(), "uninstall without a terminal needs --yes");
    assert!(hub.read_registry()?.get(SLUG).is_some());

    let output = execute(hub.root(), &["--eario-uninstall", "--yes"])?;
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(hub.read_registry()?.get(SLUG).is_none());
    assert!(!hub.bin_dir(SLUG)?.exists(), "the copy is removed");

    let output = execute(hub.root(), &["--eario-uninstall", "--yes"])?;
    assert!(!output.status.success());
    assert!(text(&output.stderr).contains("not installed"), "{}", text(&output.stderr));
    Ok(())
}

#[test]
fn install_never_replaces_a_dashboard_installed_from_github() -> TestResult {
    let hub_root = tempfile::tempdir()?;
    let hub = HubDir::at(hub_root.path());
    let output = execute(hub.root(), &["--eario-install", "--yes"])?;
    assert!(output.status.success(), "{}", text(&output.stderr));
    // The hub's registry says the same slug came from a GitHub release.
    let mut registry = hub.read_registry()?;
    let mut entry = registry.get(SLUG).cloned().ok_or("no registry entry")?;
    entry.source = Source::Github { repo: "lab/data-explorer".to_owned() };
    entry.pinned_key = Some("RWQabc".to_owned());
    registry.insert(entry);
    hub.write_registry(&registry)?;
    let copy = std::fs::read(&registry.get(SLUG).ok_or("no registry entry")?.path)?;

    let output = execute(hub.root(), &["--eario-install", "--yes"])?;
    assert!(!output.status.success(), "a GitHub install must not be replaced");
    let error = text(&output.stderr);
    assert!(error.contains("launch_installed_from_github"), "{error}");
    assert!(error.contains(SLUG) && error.contains("lab/data-explorer"), "{error}");
    assert!(error.contains("uninstall it in the hub first"), "{error}");
    assert_eq!(hub.read_registry()?, registry, "the registry is untouched");
    let entry = registry.get(SLUG).ok_or("no registry entry")?;
    assert_eq!(std::fs::read(&entry.path)?, copy, "the hub's copy is untouched");
    Ok(())
}

#[test]
fn install_waits_while_another_process_changes_the_hub_directory() -> TestResult {
    let hub_root = tempfile::tempdir()?;
    let hub = HubDir::at(hub_root.path());
    let lock = hub.lock()?;
    let mut install = command(hub.root(), &["--eario-install", "--yes"]);
    let mut child = Spawned(install.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?);

    // While this process holds the lock, the install cannot read the
    // registry to change it: it is still waiting, whatever time it took to
    // start, and has copied nothing (the copy into `bin/` comes right after
    // the lock, long before the registry is written).
    thread::sleep(Duration::from_secs(2));
    assert!(child.0.try_wait()?.is_none(), "the install did not wait for the lock");
    assert!(!hub.bin_dir(SLUG)?.exists(), "the copy was made under the lock");
    assert!(!hub.registry_path().exists(), "the registry was written under the lock");

    drop(lock);
    let (status, _, stderr) = child.exit()?;
    assert!(status.success(), "{stderr}");
    assert!(hub.read_registry()?.get(SLUG).is_some());
    Ok(())
}

#[test]
fn child_mode_announces_itself_in_the_run_file_until_shut_down() -> TestResult {
    let hub_root = tempfile::tempdir()?;
    let hub = HubDir::at(hub_root.path());
    let run_file = hub.run_file_path(SLUG)?;

    // data-explorer turns every argument into a file to load: both contract
    // arguments must be dropped, or the start fails on a missing file.
    let mut child = Spawned::child(&hub, &run_file)?;
    let announced = child.announced(&hub, SLUG)?;
    assert_eq!(announced.pid, child.0.id());
    assert_eq!(announced.protocol, LAUNCH_PROTOCOL_VERSION);
    assert!(
        hub_dir::probe_blocking(&announced.address, &announced.token)?,
        "the run file's token answers"
    );
    let (status, body) = request(&announced.address, "GET", "/api/app", &announced.token)?;
    assert_eq!(status, 200);
    assert!(body.contains("Data Explorer"), "{body}");

    for action in ["--eario-install", "--eario-uninstall"] {
        let output = execute(hub.root(), &[action, "--yes"])?;
        assert!(!output.status.success(), "{action} must be refused while the dashboard runs");
        assert!(text(&output.stderr).contains("is running"), "{}", text(&output.stderr));
    }

    let (status, _) = request(&announced.address, "POST", "/api/shutdown", &announced.token)?;
    assert_eq!(status, 204);
    let (status, stdout, stderr) = child.exit()?;
    assert!(status.success(), "{stderr}");
    assert!(stdout.is_empty(), "nothing on stdout: {stdout}");
    assert!(!run_file.exists(), "the run file is removed");

    let log = std::fs::read_to_string(hub.log_path(SLUG)?)?;
    assert!(log.contains("run file written") && log.contains("stopped"), "{log}");
    Ok(())
}
