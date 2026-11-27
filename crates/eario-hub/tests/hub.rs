//! The hub on the real example executables: each test installs them into a
//! temporary hub directory, with `--eario-install --yes` or from a fake
//! GitHub serving them as signed releases, then drives a hub through its
//! HTTP API, its supervisor or its command line.
//!
//! The examples are not executables of this package, so they are looked up
//! beside `eario-hub` in the target directory (which follows
//! `CARGO_TARGET_DIR` and the profile). Build them first:
//! `cargo build -p data-explorer -p signal-lab`. A hub in a test stops every
//! dashboard of its directory when it is dropped, pass or fail, so no
//! process outlives the test.

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use eario_core::{
    ApiError, AppResponse, DashboardState, HubCatalog, HubDashboard, HubInfo, HubOpen,
    HubSelfUpdate, HubSettings, HubUpdate, InstallPreview, KeyStatus, LAUNCH_PROTOCOL_VERSION,
    RunFile, Source,
};
use eario_hub::fake_github::{self, FakeAsset, FakeGitHub, TestKey};
use eario_hub::github::{ClientOptions, Endpoint, GitHub, GitHubClient};
use eario_hub::self_update::{HUB_REPO, HUB_SLUG, SelfUpdateConfig};
use eario_hub::verify::{self, Target};
use eario_hub::{RunningHub, Supervisor, SupervisorOptions};
use eario_runtime::ServerConfig;
use eario_runtime::hub_dir::{self, HubDir};

type TestResult = Result<(), Box<dyn std::error::Error>>;
type Failure = Box<dyn std::error::Error>;

const HUB_EXECUTABLE: &str = env!("CARGO_BIN_EXE_eario-hub");
const DATA_EXPLORER: &str = "data-explorer";
const SIGNAL_LAB: &str = "signal-lab";
/// Version of every example's manifest: the workspace version, which the
/// examples and this crate both declare.
const EXAMPLE_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Tag of a release of [`EXAMPLE_VERSION`].
const TAG: &str = concat!("v", env!("CARGO_PKG_VERSION"));
/// Pre-releases of [`EXAMPLE_VERSION`], so older than it: what a test sets
/// the installed version back to before an update.
const EARLIER_1: &str = concat!(env!("CARGO_PKG_VERSION"), "-test.1");
const EARLIER_2: &str = concat!(env!("CARGO_PKG_VERSION"), "-test.2");
/// A version newer than every example's.
const AHEAD: &str = "500.0.0";
/// Longest wait for a state, a file or a process.
const TIMEOUT: Duration = Duration::from_secs(45);
/// How long a dashboard waits for open connections once asked to stop
/// (`SHUTDOWN_GRACE` in eario-runtime's server).
const DASHBOARD_SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

/// An example executable, built beside the hub.
fn example(name: &str) -> Result<PathBuf, Failure> {
    let directory =
        Path::new(HUB_EXECUTABLE).parent().ok_or("the hub executable has no directory")?;
    let path = directory.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!(
            "{} does not exist: build the examples first with `cargo build -p data-explorer -p signal-lab`",
            path.display()
        )
        .into())
    }
}

/// Installs an example into the hub directory `hub`, as a user would.
fn install(hub: &Path, name: &str) -> TestResult {
    let output = Command::new(example(name)?)
        .args(["--eario-install", "--yes"])
        .env("EARIO_HUB_DIR", hub)
        .stdin(Stdio::null())
        .output()?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        return Err(format!("installing {name} failed: {error}").into());
    }
    Ok(())
}

/// One HTTP answer.
struct Answer {
    status: u16,
    body: String,
}

impl Answer {
    fn json<T: serde::de::DeserializeOwned>(&self) -> Result<T, Failure> {
        serde_json::from_str(&self.body)
            .map_err(|error| format!("{error} in answer {}: {}", self.status, self.body).into())
    }

    /// The error code of a refusal with `status`.
    fn refusal(&self, status: u16) -> Result<ApiError, Failure> {
        if self.status != status {
            return Err(format!("expected {status}, got {}: {}", self.status, self.body).into());
        }
        self.json()
    }
}

/// Sends one request without a body; `token` is left out when `None`.
fn request(
    address: &str,
    method: &str,
    path: &str,
    token: Option<&str>,
) -> Result<Answer, Failure> {
    request_with_body(address, method, path, token, "")
}

/// Sends one request with a JSON body.
fn request_with_body(
    address: &str,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: &str,
) -> Result<Answer, Failure> {
    let mut stream = TcpStream::connect(address)?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    let token = token.map(|token| format!("x-eario-token: {token}\r\n")).unwrap_or_default();
    let length = body.len();
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\n{token}Content-Type: application/json\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n{body}"
    )?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let status = response.split_whitespace().nth(1).ok_or("answer without a status")?.parse()?;
    let body = response.split_once("\r\n\r\n").map(|(_, body)| body.to_owned()).unwrap_or_default();
    Ok(Answer { status, body })
}

/// A hub served in this process on a temporary hub directory. Dropping it
/// stops every dashboard of the directory and kills those it started that
/// do not stop.
struct TestHub {
    supervisor: Supervisor,
    running: Option<RunningHub>,
    address: String,
    token: String,
    runtime: tokio::runtime::Runtime,
}

impl Drop for TestHub {
    fn drop(&mut self) {
        // A failure here is a dashboard that could not be stopped; the test
        // has failed already or reports it itself.
        let _ = self.runtime.block_on(self.supervisor.stop_all());
    }
}

impl TestHub {
    fn start(root: &Path) -> Result<Self, Failure> {
        Self::with_options(root, SupervisorOptions::default())
    }

    fn with_options(root: &Path, options: SupervisorOptions) -> Result<Self, Failure> {
        Self::with_supervisor(Supervisor::new(HubDir::at(root), options)?)
    }

    fn with_supervisor(supervisor: Supervisor) -> Result<Self, Failure> {
        let runtime =
            tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()?;
        let config = ServerConfig { open_browser: false, ..ServerConfig::default() };
        let running = runtime.block_on(eario_hub::start(supervisor.clone(), config))?;
        let address = running.address().to_string();
        let token = running.token().to_owned();
        Ok(Self { supervisor, running: Some(running), address, token, runtime })
    }

    fn post(&self, path: &str) -> Result<Answer, Failure> {
        request(&self.address, "POST", path, Some(&self.token))
    }

    fn action(&self, slug: &str, action: &str) -> Result<Answer, Failure> {
        self.post(&format!("/api/dashboards/{slug}/{action}"))
    }

    fn list(&self) -> Result<HubInfo, Failure> {
        let answer = request(&self.address, "GET", "/api/app", Some(&self.token))?;
        match answer.json()? {
            AppResponse::Hub(info) => Ok(info),
            AppResponse::Dashboard(_) => Err("the hub answered as a dashboard".into()),
        }
    }

    fn dashboard(&self, slug: &str) -> Result<HubDashboard, Failure> {
        let info = self.list()?;
        let found = info.dashboards.into_iter().find(|dashboard| dashboard.slug == slug);
        found.ok_or_else(|| format!("{slug} is not listed").into())
    }

    /// Waits until `slug` reaches `state`, failing early on a failed start.
    fn wait_for(&self, slug: &str, state: DashboardState) -> Result<HubDashboard, Failure> {
        let started = Instant::now();
        loop {
            let dashboard = self.dashboard(slug)?;
            if dashboard.state == state {
                return Ok(dashboard);
            }
            if dashboard.state == DashboardState::Failed {
                return Err(format!("{slug} failed: {:?}", dashboard.log_lines).into());
            }
            if started.elapsed() > TIMEOUT {
                return Err(format!("{slug} is still {:?}", dashboard.state).into());
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    /// Quits the hub as the user does, through the API, and waits for it.
    fn quit(&mut self) -> TestResult {
        assert_eq!(self.post("/api/shutdown")?.status, 204);
        if let Some(running) = self.running.take() {
            self.runtime.block_on(running.wait())?;
        }
        Ok(())
    }
}

/// The run file of `slug` in `root`, which must be there.
fn run_file(root: &Path, slug: &str) -> Result<RunFile, Failure> {
    HubDir::at(root).read_run_file(slug)?.ok_or_else(|| format!("{slug} has no run file").into())
}

/// Opens `slug` through the hub and checks that the URL reaches the
/// dashboard with its token. Returns the dashboard's address and token.
fn open_and_reach(hub: &TestHub, slug: &str, title: &str) -> Result<(String, String), Failure> {
    let opened: HubOpen = hub.action(slug, "open")?.json()?;
    let rest = opened.url.strip_prefix("http://").ok_or("not an http URL")?;
    let (address, token) = rest.split_once("/#token=").ok_or("no token in the fragment")?;
    assert_ne!(address, hub.address, "the URL is the dashboard's, not the hub's");
    let answer = request(address, "GET", "/api/app", Some(token))?;
    assert_eq!(answer.status, 200, "{}", answer.body);
    match answer.json()? {
        AppResponse::Dashboard(info) => assert_eq!(info.spec.title, title),
        AppResponse::Hub(_) => return Err("the URL reached a hub".into()),
    }
    Ok((address.to_owned(), token.to_owned()))
}

#[test]
fn both_examples_start_open_and_stop_through_the_hub() -> TestResult {
    let root = tempfile::tempdir()?;
    install(root.path(), DATA_EXPLORER)?;
    install(root.path(), SIGNAL_LAB)?;
    let hub = TestHub::start(root.path())?;

    let info = hub.list()?;
    let slugs: Vec<_> = info.dashboards.iter().map(|dashboard| dashboard.slug.as_str()).collect();
    assert_eq!(slugs, [DATA_EXPLORER, SIGNAL_LAB], "registry order");
    assert_eq!(info.eario_version, env!("CARGO_PKG_VERSION"));
    for dashboard in &info.dashboards {
        assert_eq!(dashboard.state, DashboardState::Stopped);
        assert_eq!(dashboard.version.as_deref(), Some(EXAMPLE_VERSION));
    }
    assert_eq!(hub.action(DATA_EXPLORER, "open")?.refusal(409)?.code, "hub_not_running");

    for slug in [DATA_EXPLORER, SIGNAL_LAB] {
        let answer = hub.action(slug, "start")?;
        assert_eq!(answer.status, 200, "{}", answer.body);
        let after: HubInfo = answer.json()?;
        let state = after.dashboards.iter().find(|dashboard| dashboard.slug == slug);
        assert!(
            state.is_some_and(|dashboard| dashboard.state != DashboardState::Stopped),
            "{after:?}"
        );
    }
    hub.wait_for(DATA_EXPLORER, DashboardState::Running)?;
    hub.wait_for(SIGNAL_LAB, DashboardState::Running)?;

    let run = run_file(root.path(), DATA_EXPLORER)?;
    assert_eq!(run.protocol, LAUNCH_PROTOCOL_VERSION);
    let (address, token) = open_and_reach(&hub, DATA_EXPLORER, "Data Explorer")?;
    assert_eq!((address.as_str(), token.as_str()), (run.address.as_str(), run.token.as_str()));
    open_and_reach(&hub, SIGNAL_LAB, "Signal Lab")?;

    // A running dashboard is neither started twice nor uninstalled.
    assert_eq!(hub.action(DATA_EXPLORER, "start")?.refusal(409)?.code, "hub_already_running");
    assert_eq!(hub.action(DATA_EXPLORER, "uninstall")?.refusal(409)?.code, "hub_running");
    assert_eq!(run_file(root.path(), DATA_EXPLORER)?, run, "the run file is untouched");

    let answer = hub.action(DATA_EXPLORER, "stop")?;
    assert_eq!(answer.status, 200, "{}", answer.body);
    let after: HubInfo = answer.json()?;
    assert_eq!(after.dashboards[0].state, DashboardState::Stopped, "{after:?}");
    assert_eq!(after.dashboards[1].state, DashboardState::Running, "{after:?}");
    assert_eq!(HubDir::at(root.path()).read_run_file(DATA_EXPLORER)?, None);
    assert!(request(&run.address, "GET", "/api/app", Some(&run.token)).is_err(), "port closed");

    let answer = hub.post("/api/stop-all")?;
    assert_eq!(answer.status, 200, "{}", answer.body);
    let after: HubInfo = answer.json()?;
    assert!(after.dashboards.iter().all(|dashboard| dashboard.state == DashboardState::Stopped));
    assert!(HubDir::at(root.path()).run_file_slugs()?.is_empty());

    let answer = hub.action(DATA_EXPLORER, "uninstall")?;
    assert_eq!(answer.status, 200, "{}", answer.body);
    let after: HubInfo = answer.json()?;
    let slugs: Vec<_> = after.dashboards.iter().map(|dashboard| dashboard.slug.as_str()).collect();
    assert_eq!(slugs, [SIGNAL_LAB]);
    assert!(!HubDir::at(root.path()).bin_dir(DATA_EXPLORER)?.exists(), "the copy is removed");
    assert_eq!(hub.action(DATA_EXPLORER, "start")?.refusal(404)?.code, "hub_not_installed");
    Ok(())
}

#[test]
fn a_restarted_hub_reattaches_to_the_dashboards_left_running() -> TestResult {
    let root = tempfile::tempdir()?;
    install(root.path(), DATA_EXPLORER)?;
    let mut first = TestHub::start(root.path())?;
    assert_eq!(first.action(DATA_EXPLORER, "start")?.status, 200);
    first.wait_for(DATA_EXPLORER, DashboardState::Running)?;
    let run = run_file(root.path(), DATA_EXPLORER)?;

    first.quit()?;
    assert!(
        hub_dir::probe_blocking(&run.address, &run.token)?,
        "quitting the hub leaves the dashboard running"
    );

    let second = TestHub::start(root.path())?;
    assert_eq!(second.dashboard(DATA_EXPLORER)?.state, DashboardState::Running);
    let (address, _) = open_and_reach(&second, DATA_EXPLORER, "Data Explorer")?;
    assert_eq!(address, run.address);

    // The second hub did not start it: it can only ask it to stop.
    let answer = second.action(DATA_EXPLORER, "stop")?;
    assert_eq!(answer.status, 200, "{}", answer.body);
    assert_eq!(second.dashboard(DATA_EXPLORER)?.state, DashboardState::Stopped);
    assert_eq!(HubDir::at(root.path()).read_run_file(DATA_EXPLORER)?, None);
    assert!(!hub_dir::probe_blocking(&run.address, &run.token)?);
    Ok(())
}

#[test]
fn stale_run_files_are_deleted_when_the_hub_starts() -> TestResult {
    let root = tempfile::tempdir()?;
    let hub_dir = HubDir::at(root.path());
    // A port that was free a moment ago: nobody answers there.
    let closed = TcpListener::bind("127.0.0.1:0")?.local_addr()?.to_string();
    let stale = RunFile {
        protocol: LAUNCH_PROTOCOL_VERSION,
        pid: 1,
        address: closed,
        token: "gone".to_owned(),
        started_at: 0,
    };
    hub_dir.write_run_file("crashed", &stale)?;
    fs::write(hub_dir.run_dir().join("garbage.json"), "not a run file")?;
    fs::write(hub_dir.run_dir().join("Not A Slug.json"), "{}")?;

    let _hub = TestHub::start(root.path())?;
    assert_eq!(hub_dir.read_run_file("crashed")?, None, "no dashboard answers it");
    assert!(!hub_dir.run_dir().join("garbage.json").exists());
    assert!(hub_dir.run_dir().join("Not A Slug.json").exists(), "not a run file: left alone");
    Ok(())
}

#[test]
fn a_changed_copy_is_refused_before_it_starts() -> TestResult {
    let root = tempfile::tempdir()?;
    install(root.path(), DATA_EXPLORER)?;
    let hub = TestHub::start(root.path())?;
    let hub_dir = HubDir::at(root.path());
    let registry = hub_dir.read_registry()?;
    let entry = registry.get(DATA_EXPLORER).ok_or("not installed")?.clone();

    // Same size, one byte changed: only the hash tells.
    let mut bytes = fs::read(&entry.path)?;
    if let Some(last) = bytes.last_mut() {
        *last ^= 0xff;
    }
    fs::write(&entry.path, &bytes)?;
    let refusal = hub.action(DATA_EXPLORER, "start")?.refusal(409)?;
    assert_eq!(refusal.code, "hub_modified_executable");
    assert!(refusal.message.contains("--eario-install"), "{}", refusal.message);

    // One byte more: the size tells.
    bytes.push(0);
    fs::write(&entry.path, &bytes)?;
    assert_eq!(hub.action(DATA_EXPLORER, "start")?.refusal(409)?.code, "hub_modified_executable");

    // The registry may only name the hub's own copy.
    let mut foreign = registry.clone();
    let original = example(DATA_EXPLORER)?;
    let (size, sha256) = hub_dir::file_digest(&original)?;
    foreign.insert(eario_runtime::hub_dir::RegistryEntry {
        path: original,
        size,
        sha256,
        ..entry.clone()
    });
    hub_dir.write_registry(&foreign)?;
    assert_eq!(hub.action(DATA_EXPLORER, "start")?.refusal(409)?.code, "hub_foreign_executable");

    // The copy deleted by hand.
    hub_dir.write_registry(&registry)?;
    fs::remove_file(&entry.path)?;
    let refusal = hub.action(DATA_EXPLORER, "start")?.refusal(409)?;
    assert_eq!(refusal.code, "hub_missing_executable");
    assert!(refusal.message.contains("--eario-install"), "{}", refusal.message);

    // A launch protocol this hub does not speak.
    let mut newer = registry.clone();
    let mut changed = entry.clone();
    changed.manifest.protocol = LAUNCH_PROTOCOL_VERSION + 1;
    newer.insert(changed);
    hub_dir.write_registry(&newer)?;
    let refusal = hub.action(DATA_EXPLORER, "start")?.refusal(409)?;
    assert_eq!(refusal.code, "hub_unsupported_protocol");
    assert!(refusal.message.contains(&format!("protocol {}", LAUNCH_PROTOCOL_VERSION + 1)));

    assert_eq!(hub.dashboard(DATA_EXPLORER)?.state, DashboardState::Stopped);
    assert!(hub_dir.run_file_slugs()?.is_empty(), "nothing was started");
    Ok(())
}

#[test]
fn routes_take_a_slug_and_nothing_else() -> TestResult {
    let root = tempfile::tempdir()?;
    let hub = TestHub::start(root.path())?;

    let not_slugs = [
        "..",
        "%2E%2E",
        "..%2F..%2Fbin",
        "C:%5CWindows%5Cnotepad.exe",
        "%2Fbin%2Fsh",
        "Data%20Explorer",
        "data-explorer.exe",
        "-data-explorer",
        "data-explorer%00",
    ];
    for slug in not_slugs {
        for action in ["start", "stop", "open", "uninstall"] {
            let answer = hub.action(slug, action)?;
            let refusal =
                answer.refusal(400).map_err(|error| format!("{slug} {action}: {error}"))?;
            assert_eq!(refusal.code, "hub_invalid_slug", "{slug} {action}");
        }
    }
    // A query or a body is never read: the slug alone decides.
    let answer = hub.post("/api/dashboards/nope/start?path=C%3A%5Cx.exe&args=--evil")?;
    assert_eq!(answer.refusal(404)?.code, "hub_not_installed");
    let body = r#"{"slug":"nope","path":"C:\\x.exe","executable":"/bin/sh","args":["--evil"]}"#;
    let path = "/api/dashboards/nope/start";
    let answer = request_with_body(&hub.address, "POST", path, Some(&hub.token), body)?;
    assert_eq!(answer.refusal(404)?.code, "hub_not_installed");
    // A path in place of a slug matches no route.
    let answer = hub.post("/api/dashboards/bin/data-explorer/start")?;
    assert_eq!(answer.refusal(404)?.code, "unknown_endpoint");

    // Every hub route sits behind the session token.
    for (method, path) in [("GET", "/api/app"), ("POST", "/api/stop-all")] {
        let answer = request(&hub.address, method, path, None)?;
        assert_eq!(answer.status, 401, "{method} {path}");
        let answer = request(&hub.address, method, path, Some("wrong"))?;
        assert_eq!(answer.status, 401, "{method} {path}");
    }
    assert!(hub.list()?.dashboards.is_empty());
    assert!(HubDir::at(root.path()).run_file_slugs()?.is_empty());
    Ok(())
}

#[test]
fn a_start_that_fails_reports_why_with_the_log() -> TestResult {
    let root = tempfile::tempdir()?;
    install(root.path(), DATA_EXPLORER)?;

    // A file where the log directory should be: the dashboard exits at once.
    let logs = HubDir::at(root.path()).logs_dir();
    fs::write(&logs, "")?;
    let hub = TestHub::start(root.path())?;
    assert_eq!(hub.action(DATA_EXPLORER, "start")?.status, 200);
    let failed = wait_for_failure(&hub, DATA_EXPLORER)?;
    let last = failed.log_lines.last().map(String::as_str).unwrap_or_default();
    assert!(last.contains("exited"), "{:?}", failed.log_lines);
    // Once the cause is gone, a new start clears the failure.
    fs::remove_file(&logs)?;
    assert_eq!(hub.action(DATA_EXPLORER, "start")?.status, 200);
    hub.wait_for(DATA_EXPLORER, DashboardState::Running)?;
    assert_eq!(hub.action(DATA_EXPLORER, "stop")?.status, 200);
    drop(hub);

    // No time to answer: the dashboard is killed, and its log is reported.
    let options = SupervisorOptions { start_timeout: Duration::ZERO, ..Default::default() };
    let hub = TestHub::with_options(root.path(), options)?;
    assert_eq!(hub.action(DATA_EXPLORER, "start")?.status, 200);
    let failed = wait_for_failure(&hub, DATA_EXPLORER)?;
    let last = failed.log_lines.last().map(String::as_str).unwrap_or_default();
    assert!(last.contains("did not answer"), "{:?}", failed.log_lines);
    assert_eq!(HubDir::at(root.path()).read_run_file(DATA_EXPLORER)?, None);
    Ok(())
}

fn wait_for_failure(hub: &TestHub, slug: &str) -> Result<HubDashboard, Failure> {
    let started = Instant::now();
    loop {
        let dashboard = hub.dashboard(slug)?;
        match dashboard.state {
            DashboardState::Failed => return Ok(dashboard),
            DashboardState::Running => return Err(format!("{slug} started").into()),
            _ if started.elapsed() > TIMEOUT => return Err(format!("{slug} never failed").into()),
            _ => thread::sleep(Duration::from_millis(100)),
        }
    }
}

/// The `eario-hub` executable, killed when dropped.
struct HubProcess(Child);

impl Drop for HubProcess {
    fn drop(&mut self) {
        // Both fail once the process has exited, which is what is wanted.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn hub_command(root: &Path) -> Command {
    let mut command = Command::new(HUB_EXECUTABLE);
    command
        .env("EARIO_HUB_DIR", root)
        .env("EARIO_NO_BROWSER", "1")
        .env_remove("EARIO_PORT")
        .stdin(Stdio::null());
    command
}

/// Waits up to [`TIMEOUT`] for `child` to exit.
fn exit_status(child: &mut Child) -> Result<ExitStatus, Failure> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if started.elapsed() > TIMEOUT {
            return Err("the hub did not exit in time".into());
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_second_hub_finds_the_first_and_exits() -> TestResult {
    let root = tempfile::tempdir()?;
    let hub_dir = HubDir::at(root.path());
    let mut first =
        HubProcess(hub_command(root.path()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?);

    let started = Instant::now();
    let recorded = loop {
        if let Some(recorded) = hub_dir::read_run_file_at(&hub_dir.hub_file_path())?
            && hub_dir::probe_blocking(&recorded.address, &recorded.token)?
        {
            break recorded;
        }
        if let Some(status) = first.0.try_wait()? {
            return Err(format!("the hub exited with {status}").into());
        }
        if started.elapsed() > TIMEOUT {
            return Err("the hub never recorded itself in hub.json".into());
        }
        thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(recorded.pid, first.0.id());
    let second_start = || -> TestResult {
        let command =
            hub_command(root.path()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
        let mut second = HubProcess(command?);
        // A second hub that missed the first would serve until killed.
        let status = exit_status(&mut second.0)
            .map_err(|error| format!("{error}: the second start missed the first hub"))?;
        let (mut printed, mut errors) = (String::new(), String::new());
        if let Some(mut stdout) = second.0.stdout.take() {
            stdout.read_to_string(&mut printed)?;
        }
        if let Some(mut stderr) = second.0.stderr.take() {
            stderr.read_to_string(&mut errors)?;
        }
        assert!(status.success(), "{errors}");
        assert!(printed.contains("already running"), "{printed}");
        assert!(printed.contains(&recorded.address), "{printed}");
        let still = hub_dir::read_run_file_at(&hub_dir.hub_file_path())?;
        assert_eq!(still, Some(recorded.clone()), "the first hub stays recorded");
        Ok(())
    };
    second_start()?;

    // An installed dashboard whose run file names a server that never
    // answers: listing waits the whole probe timeout on it. The second start
    // must not depend on that.
    install(root.path(), DATA_EXPLORER)?;
    let silent = TcpListener::bind("127.0.0.1:0")?;
    let stale = RunFile {
        protocol: LAUNCH_PROTOCOL_VERSION,
        pid: 1,
        address: silent.local_addr()?.to_string(),
        token: "gone".to_owned(),
        started_at: 0,
    };
    hub_dir.write_run_file(DATA_EXPLORER, &stale)?;
    second_start()?;
    // Listing finds it stale and deletes it.
    let answer = request(&recorded.address, "GET", "/api/app", Some(&recorded.token))?;
    assert_eq!(answer.status, 200, "{}", answer.body);
    assert_eq!(hub_dir.read_run_file(DATA_EXPLORER)?, None, "the stale run file is deleted");

    // A registry that does not parse: listing fails, and says how to recover;
    // the second start still finds the hub.
    fs::write(hub_dir.registry_path(), "not json")?;
    second_start()?;
    let answer = request(&recorded.address, "GET", "/api/app", Some(&recorded.token))?;
    let refusal = answer.refusal(500)?;
    assert_eq!(refusal.code, "hub_registry_invalid");
    assert!(refusal.message.contains("registry.json"), "{}", refusal.message);
    assert!(refusal.message.contains("delete"), "{}", refusal.message);
    drop(silent);

    let answer = request(&recorded.address, "POST", "/api/shutdown", Some(&recorded.token))?;
    assert_eq!(answer.status, 204);
    let status = exit_status(&mut first.0)?;
    let mut printed = String::new();
    if let Some(mut stdout) = first.0.stdout.take() {
        stdout.read_to_string(&mut printed)?;
    }
    assert!(status.success(), "{printed}");
    assert!(printed.contains(&format!("http://{}/#token=", recorded.address)), "{printed}");
    assert!(!hub_dir.hub_file_path().exists(), "hub.json is removed when the hub quits");
    Ok(())
}

#[test]
fn a_dashboard_that_does_not_stop_in_time_is_killed_or_reported() -> TestResult {
    let root = tempfile::tempdir()?;
    install(root.path(), DATA_EXPLORER)?;
    let quick = SupervisorOptions { stop_timeout: Duration::from_secs(1), ..Default::default() };
    let hub = TestHub::with_options(root.path(), quick)?;
    let log = HubDir::at(root.path()).log_path(DATA_EXPLORER)?;

    // Started by this hub: killed once the stop timeout passes. A request
    // left half-sent keeps the dashboard in its five-second shutdown grace,
    // longer than the hub waits.
    assert_eq!(hub.action(DATA_EXPLORER, "start")?.status, 200);
    hub.wait_for(DATA_EXPLORER, DashboardState::Running)?;
    let run = run_file(root.path(), DATA_EXPLORER)?;
    let mut held = TcpStream::connect(&run.address)?;
    write!(held, "GET /api/app HTTP/1.1\r\nHost: {}\r\n", run.address)?;
    held.flush()?;
    let asked = Instant::now();
    let answer = hub.action(DATA_EXPLORER, "stop")?;
    assert_eq!(answer.status, 200, "{}", answer.body);
    // The stop timeout, then a probe of the closed port (about two seconds on
    // Windows) before the stale run file is removed; well short of the
    // grace period the dashboard would have taken on its own.
    let bound = quick.stop_timeout + hub_dir::PROBE_TIMEOUT + Duration::from_millis(1500);
    assert!(bound < quick.stop_timeout + DASHBOARD_SHUTDOWN_GRACE);
    let took = asked.elapsed();
    assert!(took < bound, "killed, not left to its grace period: {took:?}");
    assert_eq!(HubDir::at(root.path()).read_run_file(DATA_EXPLORER)?, None, "stale file removed");
    assert!(!fs::read_to_string(&log)?.contains("stopped"), "it never finished stopping");
    drop(held);

    // Not started by this hub and still answering after the timeout: it
    // cannot be killed, so the failure is reported.
    let stubborn = RunFile {
        protocol: LAUNCH_PROTOCOL_VERSION,
        pid: 1,
        address: answer_everything()?,
        token: "any".to_owned(),
        started_at: 0,
    };
    HubDir::at(root.path()).write_run_file(DATA_EXPLORER, &stubborn)?;
    assert_eq!(hub.dashboard(DATA_EXPLORER)?.state, DashboardState::Running);
    let refusal = hub.action(DATA_EXPLORER, "stop")?.refusal(504)?;
    assert_eq!(refusal.code, "hub_stop_timeout");
    assert!(refusal.message.contains(DATA_EXPLORER), "{}", refusal.message);
    assert_eq!(HubDir::at(root.path()).read_run_file(DATA_EXPLORER)?, Some(stubborn));
    Ok(())
}

/// A server that answers `200` to every request and so never stops when
/// asked. Returns its address; it serves until the test process exits.
fn answer_everything() -> Result<String, Failure> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?.to_string();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut request = Vec::new();
            let mut byte = [0_u8; 1];
            while !request.ends_with(b"\r\n\r\n")
                && stream.read(&mut byte).is_ok_and(|read| read == 1)
            {
                request.push(byte[0]);
            }
            // The client may be gone already; there is nobody to tell.
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        }
    });
    Ok(address)
}

// Installs and updates from GitHub (ADR 0013), against a fake GitHub on the
// loopback interface. The asset is the real data-explorer executable, signed
// with test keys. Every example declares the workspace version
// ([`EXAMPLE_VERSION`]), so a test that needs an older installed version sets
// it back in the registry to a pre-release of it; and each release
// appends a few bytes to the executable (which still runs), so every version
// is a different file.

/// Repository the tests publish data-explorer from.
const REPO: &str = "lab/data-explorer";

/// A test signing key; `seed` tells keys, and their key ids, apart.
fn test_key(seed: u8) -> Result<TestKey, Failure> {
    Ok(TestKey::new([seed; 32], [b'e', b'a', b'r', b'i', b'o', 0, 0, seed])?)
}

/// The data-explorer executable, with `marker` appended unless it is empty.
fn data_explorer_build(marker: &str) -> Result<Vec<u8>, Failure> {
    let mut bytes = fs::read(example(DATA_EXPLORER)?)?;
    bytes.extend_from_slice(marker.as_bytes());
    Ok(bytes)
}

/// Name of the release asset of `slug` for this machine.
fn asset_of(slug: &str, target: Target) -> Result<String, Failure> {
    Ok(verify::asset_name(slug, target)?)
}

/// Serves `assets` as the latest release `tag` of `repo`, with their
/// checksum list signed by `key`, and `key`'s `minisign.pub` when
/// `with_key_file`.
fn publish(
    fake: &FakeGitHub,
    repo: &str,
    tag: &str,
    key: &TestKey,
    assets: Vec<FakeAsset>,
    with_key_file: bool,
) {
    let mut files = assets.clone();
    files.extend(
        key.release_files(&assets)
            .into_iter()
            .filter(|file| with_key_file || file.name != verify::KEY_FILE),
    );
    fake.release(repo, tag, &files);
}

/// Serves a catalog at `catalog` listing `entries` (repository, key).
fn serve_catalog(fake: &FakeGitHub, catalog: &str, entries: &[(&str, String)]) {
    let dashboards: Vec<serde_json::Value> = entries
        .iter()
        .map(|(repo, key)| {
            serde_json::json!({
                "repo": repo, "name": "Data Explorer", "description": "Explore a table",
                "public_key": key,
            })
        })
        .collect();
    let catalog_json =
        serde_json::json!({ "format_version": 1, "name": "Lab", "dashboards": dashboards });
    fake.catalog(catalog, catalog_json.to_string());
}

/// A client of the fake GitHub.
fn fake_client(fake: &FakeGitHub) -> Result<Arc<dyn GitHub>, Failure> {
    let endpoint = Endpoint::loopback(&fake.base_url()).ok_or("the fake is not on loopback")?;
    Ok(Arc::new(GitHubClient::with_options(endpoint, ClientOptions::default())?))
}

/// A supervisor of `root` that reaches the fake GitHub, without a server.
/// Dropping it stops every dashboard of the directory.
struct RemoteHub {
    supervisor: Supervisor,
    runtime: tokio::runtime::Runtime,
}

impl Drop for RemoteHub {
    fn drop(&mut self) {
        // A failure here is a dashboard that could not be stopped; the test
        // has failed already or reports it itself.
        let _ = self.runtime.block_on(self.supervisor.stop_all());
    }
}

impl RemoteHub {
    fn new(root: &Path, fake: &FakeGitHub) -> Result<Self, Failure> {
        let supervisor = Supervisor::with_github(
            HubDir::at(root),
            SupervisorOptions::default(),
            fake_client(fake)?,
        )?;
        Self::with_supervisor(supervisor)
    }

    fn with_supervisor(supervisor: Supervisor) -> Result<Self, Failure> {
        let runtime =
            tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()?;
        Ok(Self { supervisor, runtime })
    }

    fn run<F: std::future::Future>(&self, future: F) -> F::Output {
        self.runtime.block_on(future)
    }

    fn state(&self, slug: &str) -> Result<HubDashboard, Failure> {
        let info = self.run(self.supervisor.list())?;
        let found = info.dashboards.into_iter().find(|dashboard| dashboard.slug == slug);
        found.ok_or_else(|| format!("{slug} is not listed").into())
    }

    fn wait_for(&self, slug: &str, state: DashboardState) -> Result<(), Failure> {
        let started = Instant::now();
        loop {
            let dashboard = self.state(slug)?;
            if dashboard.state == state {
                return Ok(());
            }
            if dashboard.state == DashboardState::Failed || started.elapsed() > TIMEOUT {
                return Err(
                    format!("{slug} is {:?}: {:?}", dashboard.state, dashboard.log_lines).into()
                );
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

/// Code of a refusal; `None` on success.
fn refusal<T>(result: Result<T, eario_hub::HubError>) -> Option<&'static str> {
    result.err().map(|error| error.code())
}

/// Sets the version the registry records for `slug`.
fn set_installed_version(root: &Path, slug: &str, version: &str) -> TestResult {
    let hub = HubDir::at(root);
    let mut registry = hub.read_registry()?;
    let mut entry = registry.get(slug).ok_or("not installed")?.clone();
    entry.manifest.version = Some(version.to_owned());
    registry.insert(entry);
    hub.write_registry(&registry)?;
    Ok(())
}

/// The files in `bin/<slug>/`, sorted.
fn bin_files(root: &Path, slug: &str) -> Result<Vec<String>, Failure> {
    let directory = HubDir::at(root).bin_dir(slug)?;
    let mut names = Vec::new();
    match fs::read_dir(&directory) {
        Ok(entries) => {
            for entry in entries {
                names.push(entry?.file_name().to_string_lossy().into_owned());
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    names.sort();
    Ok(names)
}

/// Files under `staging/`, at any depth, except the `lock` of each staging
/// area: the downloaded releases.
fn staged_paths(root: &Path) -> Result<Vec<PathBuf>, Failure> {
    fn walk(directory: &Path, found: &mut Vec<PathBuf>) -> TestResult {
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        for entry in entries {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                walk(&entry.path(), found)?;
            } else if entry.file_name() != "lock" {
                found.push(entry.path());
            }
        }
        Ok(())
    }
    let mut found = Vec::new();
    walk(&HubDir::at(root).staging_dir(), &mut found)?;
    Ok(found)
}

/// Number of [`staged_paths`].
fn staged_files(root: &Path) -> Result<usize, Failure> {
    Ok(staged_paths(root)?.len())
}

/// Nothing of `slug` was installed: no registry entry, no `bin/<slug>/`, and
/// nothing left in the staging directory.
fn assert_nothing_installed(root: &Path, slug: &str) -> TestResult {
    assert!(HubDir::at(root).read_registry()?.get(slug).is_none(), "{slug} is in the registry");
    assert!(!HubDir::at(root).bin_dir(slug)?.exists(), "bin/{slug} exists");
    assert_eq!(staged_files(root)?, 0, "the staging directory is not empty");
    Ok(())
}

/// Requests of the fake for the asset itself (not its redirect).
fn asset_downloads(fake: &FakeGitHub, asset: &str) -> usize {
    fake.requests()
        .iter()
        .filter(|path| path.starts_with("/assets/") && path.ends_with(asset))
        .count()
}

#[test]
fn a_release_listed_by_a_catalog_installs_and_runs() -> TestResult {
    let Some(target) = Target::CURRENT else { return Ok(()) };
    let root = tempfile::tempdir()?;
    let fake = FakeGitHub::start()?;
    let key = test_key(1)?;
    let asset = asset_of(DATA_EXPLORER, target)?;
    let executable = data_explorer_build("")?;
    // No minisign.pub: the catalog's key verifies the release.
    publish(&fake, REPO, TAG, &key, vec![FakeAsset::new(&asset, executable.clone())], false);
    serve_catalog(&fake, eario_hub::remote::DEFAULT_CATALOG, &[(REPO, key.public_key())]);
    let hub = RemoteHub::new(root.path(), &fake)?;

    let catalogs = hub.run(hub.supervisor.catalog_listing())?;
    assert_eq!(catalogs.len(), 1, "the default catalog");
    assert_eq!(catalogs[0].dashboards[0].repo, REPO);
    assert_eq!(catalogs[0].dashboards[0].installed_slug, None);

    let preview = hub.run(hub.supervisor.preview(REPO, None))?;
    assert_eq!(preview.key_status, KeyStatus::Catalog);
    assert_eq!(preview.slug, DATA_EXPLORER);
    // A trusted key: the verified file was run for its manifest.
    let manifest = preview.manifest.clone().ok_or("the preview of a trusted key is complete")?;
    assert_eq!((manifest.slug.as_str(), manifest.name.as_str()), (DATA_EXPLORER, "Data Explorer"));
    assert_eq!(manifest.version.as_deref(), Some(EXAMPLE_VERSION));
    assert_eq!((preview.tag.as_str(), preview.asset.as_str()), (TAG, asset.as_str()));
    assert_eq!(preview.size, executable.len() as u64);
    assert_eq!(preview.public_key, key.public_key());
    let key_line = key.public_key();
    let parsed = verify::PublicKey::from_base64(REPO, &key_line)?;
    assert_eq!(preview.key_sha256, parsed.sha256());
    // Nothing is installed by a preview.
    assert!(bin_files(root.path(), DATA_EXPLORER)?.is_empty());
    assert_eq!(staged_files(root.path())?, 1, "the verified file waits in staging");

    // The confirmation must repeat the previewed SHA-256.
    let wrong = "0".repeat(64);
    assert_eq!(
        refusal(hub.run(hub.supervisor.install(REPO, None, &wrong))),
        Some("hub_preview_mismatch")
    );
    let not_a_hash = hub.run(hub.supervisor.install(REPO, None, "../../bin"));
    assert_eq!(refusal(not_a_hash), Some("hub_invalid_sha256"));
    let other_repo = hub.run(hub.supervisor.install("lab/other", None, &preview.sha256));
    assert_eq!(refusal(other_repo), Some("hub_no_preview"));

    let slug = hub.run(hub.supervisor.install(REPO, None, &preview.sha256))?;
    assert_eq!(slug, DATA_EXPLORER);
    let entry =
        HubDir::at(root.path()).read_registry()?.get(DATA_EXPLORER).cloned().ok_or("entry")?;
    assert_eq!(entry.source, Source::Github { repo: REPO.to_owned() });
    assert_eq!(entry.pinned_key.as_deref(), Some(key_line.as_str()), "the key is pinned");
    assert_eq!(entry.sha256, preview.sha256);
    assert_eq!(entry.previous, None);
    assert_eq!(
        entry.path.parent(),
        Some(HubDir::at(root.path()).bin_dir(DATA_EXPLORER)?.as_path())
    );
    assert_eq!(fs::read(&entry.path)?, executable);
    assert_eq!(staged_files(root.path())?, 0, "the staging directory is emptied");
    // The preview was used up.
    let again = hub.run(hub.supervisor.install(REPO, None, &preview.sha256));
    assert_eq!(refusal(again), Some("hub_no_preview"));

    let catalogs = hub.run(hub.supervisor.catalog_listing())?;
    assert_eq!(catalogs[0].dashboards[0].installed_slug.as_deref(), Some(DATA_EXPLORER));
    assert_eq!(refusal(hub.run(hub.supervisor.preview(REPO, None))), Some("hub_already_installed"));

    // It starts like a local install.
    hub.run(hub.supervisor.start(DATA_EXPLORER))?;
    hub.wait_for(DATA_EXPLORER, DashboardState::Running)?;
    let listed = hub.state(DATA_EXPLORER)?;
    assert_eq!(listed.source, Source::Github { repo: REPO.to_owned() });
    assert!(!listed.has_previous);
    hub.run(hub.supervisor.stop(DATA_EXPLORER))?;
    Ok(())
}

#[test]
fn a_repository_no_catalog_lists_is_trusted_once_its_key_is_accepted() -> TestResult {
    let Some(target) = Target::CURRENT else { return Ok(()) };
    let root = tempfile::tempdir()?;
    let fake = FakeGitHub::start()?;
    let key = test_key(2)?;
    let asset = asset_of(DATA_EXPLORER, target)?;
    publish(&fake, REPO, TAG, &key, vec![FakeAsset::new(&asset, data_explorer_build("")?)], true);
    // The default catalog does not exist: its key vouches for nothing.
    let hub = RemoteHub::new(root.path(), &fake)?;

    let preview = hub.run(hub.supervisor.preview(REPO, None))?;
    assert_eq!(preview.key_status, KeyStatus::Unknown);
    assert_eq!(preview.manifest, None, "not run before its key is trusted");
    assert_eq!(preview.slug, DATA_EXPLORER, "from the asset's name");
    assert_eq!(staged_files(root.path())?, 1, "the verified file waits in staging");
    let refused = hub.run(hub.supervisor.install(REPO, None, &preview.sha256));
    let error = refused.err().ok_or("an unknown key must be refused")?;
    assert_eq!(error.code(), "hub_key_not_accepted");
    assert!(error.to_string().contains(&preview.key_sha256), "{error}");
    assert!(HubDir::at(root.path()).read_registry()?.get(DATA_EXPLORER).is_none());

    // Only the previewed key's SHA-256 accepts it; the minisign key id is not
    // enough.
    let wrong = hub.run(hub.supervisor.accept_key(REPO, None, &"ab".repeat(32)));
    assert_eq!(refusal(wrong), Some("hub_key_mismatch"));
    let key_id = verify::PublicKey::from_base64(REPO, &key.public_key())?.fingerprint().to_owned();
    let id_only = hub.run(hub.supervisor.accept_key(REPO, None, &key_id));
    assert_eq!(refusal(id_only), Some("hub_invalid_sha256"));
    let accepted = hub.run(hub.supervisor.accept_key(REPO, None, &preview.key_sha256))?;
    assert_eq!(accepted.key_status, KeyStatus::Accepted);
    assert_eq!(accepted.sha256, preview.sha256);
    // Accepted: only now was it run, for the full preview.
    let manifest = accepted.manifest.clone().ok_or("the full preview")?;
    assert_eq!((manifest.slug.as_str(), manifest.name.as_str()), (DATA_EXPLORER, "Data Explorer"));
    let again = hub.run(hub.supervisor.accept_key(REPO, None, &preview.key_sha256))?;
    assert_eq!(again, accepted, "accepting again changes nothing");

    hub.run(hub.supervisor.install(REPO, None, &preview.sha256))?;
    let entry =
        HubDir::at(root.path()).read_registry()?.get(DATA_EXPLORER).cloned().ok_or("entry")?;
    assert_eq!(entry.pinned_key, Some(key.public_key()), "the key seen at install is pinned");
    assert_eq!(staged_files(root.path())?, 0);
    Ok(())
}

#[test]
fn a_changed_key_is_refused_until_accepted() -> TestResult {
    let Some(target) = Target::CURRENT else { return Ok(()) };
    let root = tempfile::tempdir()?;
    let fake = FakeGitHub::start()?;
    let (first, second) = (test_key(3)?, test_key(4)?);
    let asset = asset_of(DATA_EXPLORER, target)?;
    publish(&fake, REPO, TAG, &first, vec![FakeAsset::new(&asset, data_explorer_build("")?)], true);
    serve_catalog(&fake, eario_hub::remote::DEFAULT_CATALOG, &[(REPO, first.public_key())]);
    let hub = RemoteHub::new(root.path(), &fake)?;
    let preview = hub.run(hub.supervisor.preview(REPO, None))?;
    assert_eq!(preview.key_status, KeyStatus::Catalog);
    hub.run(hub.supervisor.install(REPO, None, &preview.sha256))?;
    set_installed_version(root.path(), DATA_EXPLORER, EARLIER_1)?;
    let installed = fs::read(
        &HubDir::at(root.path()).read_registry()?.get(DATA_EXPLORER).ok_or("entry")?.path,
    )?;

    // A newer release signed with another key, which it carries.
    let newer = FakeAsset::new(&asset, data_explorer_build("v2")?);
    publish(&fake, REPO, TAG, &second, vec![newer.clone()], true);
    let changed = hub.run(hub.supervisor.preview_update(DATA_EXPLORER))?;
    assert_eq!(changed.key_status, KeyStatus::Changed);
    assert_eq!(changed.public_key, second.public_key());
    assert_eq!(changed.manifest, None, "not run before the new key is accepted");
    let refused = hub.run(hub.supervisor.apply_update(DATA_EXPLORER, &changed.sha256));
    let error = refused.err().ok_or("a changed key must be refused")?;
    assert_eq!(error.code(), "hub_key_changed");
    assert!(error.to_string().contains(&changed.key_sha256), "{error}");
    assert_eq!(refusal(hub.run(hub.supervisor.update(DATA_EXPLORER))), Some("hub_key_changed"));
    let entry =
        HubDir::at(root.path()).read_registry()?.get(DATA_EXPLORER).cloned().ok_or("entry")?;
    assert_eq!(entry.pinned_key, Some(first.public_key()), "still pinned to the first key");
    assert_eq!(fs::read(&entry.path)?, installed, "the installed copy is untouched");

    // Signed with the second key but presenting the first: refused outright.
    let mut files = vec![newer.clone()];
    let [signature, sums, _] = second.release_files(&files);
    files.extend([signature, sums, FakeAsset::new(verify::KEY_FILE, first.pub_file())]);
    fake.release(REPO, TAG, &files);
    let forged = hub.run(hub.supervisor.preview_update(DATA_EXPLORER));
    assert_eq!(refusal(forged), Some("release_wrong_key"));

    // Accepted by its SHA-256, the new key is pinned by the update.
    publish(&fake, REPO, TAG, &second, vec![newer], true);
    let changed = hub.run(hub.supervisor.preview_update(DATA_EXPLORER))?;
    assert_eq!(changed.manifest, None);
    let accepted = hub.run(hub.supervisor.accept_key(REPO, None, &changed.key_sha256))?;
    assert_eq!(accepted.key_status, KeyStatus::Accepted);
    let manifest = accepted.manifest.ok_or("the full preview once the key is accepted")?;
    assert_eq!(manifest.version.as_deref(), Some(EXAMPLE_VERSION), "newer than its pre-release");
    hub.run(hub.supervisor.apply_update(DATA_EXPLORER, &changed.sha256))?;
    let entry =
        HubDir::at(root.path()).read_registry()?.get(DATA_EXPLORER).cloned().ok_or("entry")?;
    assert_eq!(entry.pinned_key, Some(second.public_key()));
    assert_eq!(entry.sha256, changed.sha256);
    Ok(())
}

#[test]
fn a_slug_installed_from_elsewhere_is_refused() -> TestResult {
    let Some(target) = Target::CURRENT else { return Ok(()) };
    let root = tempfile::tempdir()?;
    let fake = FakeGitHub::start()?;
    let key = test_key(5)?;
    let asset = asset_of(DATA_EXPLORER, target)?;
    let release = vec![FakeAsset::new(&asset, data_explorer_build("")?)];
    publish(&fake, REPO, TAG, &key, release.clone(), true);
    publish(&fake, "elsewhere/data-explorer", TAG, &key, release, true);
    let hub = RemoteHub::new(root.path(), &fake)?;

    // Installed locally.
    install(root.path(), DATA_EXPLORER)?;
    let local = HubDir::at(root.path()).read_registry()?;
    let error = hub.run(hub.supervisor.preview(REPO, None)).err().ok_or("slug clash")?;
    assert_eq!(error.code(), "hub_slug_taken");
    assert!(error.to_string().contains("--eario-install"), "{error}");
    assert_eq!(HubDir::at(root.path()).read_registry()?, local, "the local install is untouched");
    assert_eq!(asset_downloads(&fake, &asset), 0, "refused before the download");

    // Installed from another repository.
    hub.run(hub.supervisor.uninstall(DATA_EXPLORER))?;
    let preview = hub.run(hub.supervisor.preview(REPO, None))?;
    hub.run(hub.supervisor.accept_key(REPO, None, &preview.key_sha256))?;
    hub.run(hub.supervisor.install(REPO, None, &preview.sha256))?;
    let error =
        hub.run(hub.supervisor.preview("elsewhere/data-explorer", None)).err().ok_or("clash")?;
    assert_eq!(error.code(), "hub_slug_taken");
    assert!(error.to_string().contains(REPO), "{error}");
    // The same repository, in another case, is the one installed: named by
    // its slug, refused before any request.
    let requests = fake.requests().len();
    let named = hub.run(hub.supervisor.preview("Lab/Data-Explorer", Some(DATA_EXPLORER)));
    let error = named.err().ok_or("installed")?;
    assert_eq!(error.code(), "hub_already_installed");
    assert!(error.to_string().contains("update it instead"), "{error}");
    assert_eq!(fake.requests().len(), requests, "refused before any request");
    // Without a slug, once the release shows it is the dashboard installed.
    let error = hub.run(hub.supervisor.preview(REPO, None)).err().ok_or("installed")?;
    assert_eq!(error.code(), "hub_already_installed");
    // Named by its slug, a dashboard installed from another repository.
    let error = hub
        .run(hub.supervisor.preview("elsewhere/data-explorer", Some(DATA_EXPLORER)))
        .err()
        .ok_or("clash")?;
    assert_eq!(error.code(), "hub_slug_taken");
    assert_eq!(fake.requests().len(), requests + 1, "only the release listing of `{REPO}`");
    // A local install is never updated from GitHub.
    hub.run(hub.supervisor.uninstall(DATA_EXPLORER))?;
    install(root.path(), DATA_EXPLORER)?;
    assert_eq!(refusal(hub.run(hub.supervisor.update(DATA_EXPLORER))), Some("hub_not_from_github"));
    Ok(())
}

#[test]
fn a_tampered_asset_or_an_unsigned_release_installs_nothing() -> TestResult {
    let Some(target) = Target::CURRENT else { return Ok(()) };
    let root = tempfile::tempdir()?;
    let fake = FakeGitHub::start()?;
    let key = test_key(6)?;
    let asset = asset_of(DATA_EXPLORER, target)?;
    let genuine = FakeAsset::new(&asset, data_explorer_build("")?);
    let hub = RemoteHub::new(root.path(), &fake)?;

    // Same size, one byte changed after signing.
    let mut tampered = genuine.clone();
    if let Some(last) = tampered.content.last_mut() {
        *last ^= 0xff;
    }
    let mut files = vec![tampered];
    files.extend(key.release_files(std::slice::from_ref(&genuine)));
    fake.release(REPO, TAG, &files);
    let error = hub.run(hub.supervisor.preview(REPO, None)).err().ok_or("a tampered asset")?;
    assert_eq!(error.code(), "release_hash_mismatch");
    assert!(error.to_string().contains(&asset), "{error}");
    assert_nothing_installed(root.path(), DATA_EXPLORER)?;

    // No signature: refused before the asset is downloaded.
    let mut unsigned = vec![genuine.clone()];
    let [_, sums, pub_file] = key.release_files(std::slice::from_ref(&genuine));
    unsigned.extend([sums, pub_file]);
    let downloads = asset_downloads(&fake, &asset);
    fake.release(REPO, TAG, &unsigned);
    let error = hub.run(hub.supervisor.preview(REPO, None)).err().ok_or("an unsigned release")?;
    assert_eq!(error.code(), "release_missing_signature");
    assert!(error.to_string().contains(REPO), "{error}");
    assert_eq!(asset_downloads(&fake, &asset), downloads);
    assert_nothing_installed(root.path(), DATA_EXPLORER)?;

    // A checksum list changed after signing.
    let mut files = vec![genuine.clone()];
    let [signature, sums, pub_file] = key.release_files(std::slice::from_ref(&genuine));
    let mut changed = sums.content.clone();
    changed.extend_from_slice(format!("{}  evil.exe\n", "ab".repeat(32)).as_bytes());
    files.extend([signature, FakeAsset::new(verify::SUMS_FILE, changed), pub_file]);
    fake.release(REPO, TAG, &files);
    assert_eq!(refusal(hub.run(hub.supervisor.preview(REPO, None))), Some("release_bad_signature"));
    assert_nothing_installed(root.path(), DATA_EXPLORER)?;
    Ok(())
}

#[test]
fn an_asset_that_is_another_dashboard_or_not_one_is_refused() -> TestResult {
    let Some(target) = Target::CURRENT else { return Ok(()) };
    let root = tempfile::tempdir()?;
    let fake = FakeGitHub::start()?;
    let key = test_key(7)?;
    // The catalog vouches for the key, so the verified file is run at once.
    let listed = [("lab/signal-lab", key.public_key()), (REPO, key.public_key())];
    serve_catalog(&fake, eario_hub::remote::DEFAULT_CATALOG, &listed);
    let hub = RemoteHub::new(root.path(), &fake)?;

    // data-explorer published as signal-lab: its manifest tells.
    let disguised = FakeAsset::new(&asset_of(SIGNAL_LAB, target)?, data_explorer_build("")?);
    publish(&fake, "lab/signal-lab", "v1.0.0", &key, vec![disguised], false);
    let error = hub.run(hub.supervisor.preview("lab/signal-lab", None)).err().ok_or("disguised")?;
    assert_eq!(error.code(), "hub_manifest_mismatch");
    let message = error.to_string();
    assert!(message.contains(SIGNAL_LAB) && message.contains(DATA_EXPLORER), "{message}");
    assert_nothing_installed(root.path(), SIGNAL_LAB)?;
    assert_nothing_installed(root.path(), DATA_EXPLORER)?;

    // Signed, but not an executable at all.
    let text = FakeAsset::new(&asset_of(DATA_EXPLORER, target)?, "not an executable");
    publish(&fake, REPO, "v1.0.0", &key, vec![text], false);
    let error = hub.run(hub.supervisor.preview(REPO, None)).err().ok_or("not an executable")?;
    assert_eq!(error.code(), "hub_manifest_failed");
    assert_nothing_installed(root.path(), DATA_EXPLORER)?;
    Ok(())
}

#[test]
fn a_release_signed_with_an_untrusted_key_is_not_run_before_it_is_accepted() -> TestResult {
    let Some(target) = Target::CURRENT else { return Ok(()) };
    let root = tempfile::tempdir()?;
    let fake = FakeGitHub::start()?;
    let untrusted = test_key(17)?;
    let asset = asset_of(DATA_EXPLORER, target)?;
    // Signed and listed in the checksums, but not an executable: running it
    // fails, so a preview that ran it would fail with `hub_manifest_failed`.
    let not_executable = FakeAsset::new(&asset, "not an executable");
    publish(&fake, REPO, TAG, &untrusted, vec![not_executable.clone()], true);
    let hub = RemoteHub::new(root.path(), &fake)?;

    // An unknown key: verified, staged, not run.
    let pending = hub.run(hub.supervisor.preview(REPO, None))?;
    assert_eq!(pending.key_status, KeyStatus::Unknown);
    assert_eq!(pending.manifest, None);
    assert_eq!(pending.slug, DATA_EXPLORER);
    assert_eq!(pending.size, not_executable.content.len() as u64);
    assert_eq!(pending.public_key, untrusted.public_key());
    assert_eq!(staged_files(root.path())?, 1);
    let refused = hub.run(hub.supervisor.install(REPO, None, &pending.sha256));
    assert_eq!(refusal(refused), Some("hub_key_not_accepted"));
    // Accepted: only now is it run, and refused for what it is.
    let error = hub.run(hub.supervisor.accept_key(REPO, None, &pending.key_sha256)).err();
    assert_eq!(error.map(|error| error.code()), Some("hub_manifest_failed"));
    assert_nothing_installed(root.path(), DATA_EXPLORER)?;
    let dropped = hub.run(hub.supervisor.install(REPO, None, &pending.sha256));
    assert_eq!(refusal(dropped), Some("hub_no_preview"), "the failed preview is dropped");

    // A changed key on an update: the same.
    let trusted = test_key(18)?;
    install_from_github(&hub, &fake, &trusted, "build 1")?;
    set_installed_version(root.path(), DATA_EXPLORER, EARLIER_1)?;
    let before = HubDir::at(root.path()).read_registry()?;
    let copies = bin_files(root.path(), DATA_EXPLORER)?;
    publish(&fake, REPO, TAG, &untrusted, vec![not_executable], true);
    let pending = hub.run(hub.supervisor.preview_update(DATA_EXPLORER))?;
    assert_eq!(pending.key_status, KeyStatus::Changed);
    assert_eq!(pending.manifest, None);
    let refused = hub.run(hub.supervisor.apply_update(DATA_EXPLORER, &pending.sha256));
    assert_eq!(refusal(refused), Some("hub_key_changed"));
    let error = hub.run(hub.supervisor.accept_key(REPO, None, &pending.key_sha256)).err();
    assert_eq!(error.map(|error| error.code()), Some("hub_manifest_failed"));
    assert_eq!(HubDir::at(root.path()).read_registry()?, before, "nothing was changed");
    assert_eq!(bin_files(root.path(), DATA_EXPLORER)?, copies);
    assert_eq!(staged_files(root.path())?, 0);

    // A staged file that went away is not accepted: preview it again.
    let newer = FakeAsset::new(&asset, data_explorer_build("build 2")?);
    publish(&fake, REPO, TAG, &untrusted, vec![newer], true);
    let pending = hub.run(hub.supervisor.preview_update(DATA_EXPLORER))?;
    for path in staged_paths(root.path())? {
        fs::remove_file(path)?;
    }
    let error = hub.run(hub.supervisor.accept_key(REPO, None, &pending.key_sha256)).err();
    let error = error.ok_or("a staged file that is gone")?;
    assert_eq!(error.code(), "hub_preview_gone");
    assert!(error.to_string().contains("preview it again"), "{error}");

    // Previewed again and accepted, it is run, then installed.
    let pending = hub.run(hub.supervisor.preview_update(DATA_EXPLORER))?;
    let full = hub.run(hub.supervisor.accept_key(REPO, None, &pending.key_sha256))?;
    let manifest = full.manifest.ok_or("the full preview")?;
    assert_eq!(manifest.slug, DATA_EXPLORER);
    hub.run(hub.supervisor.apply_update(DATA_EXPLORER, &pending.sha256))?;
    let entry =
        HubDir::at(root.path()).read_registry()?.get(DATA_EXPLORER).cloned().ok_or("entry")?;
    assert_eq!(entry.pinned_key, Some(untrusted.public_key()), "the accepted key is pinned");
    assert_eq!(entry.sha256, pending.sha256);

    // An install whose staged file went away is refused the same way.
    hub.run(hub.supervisor.uninstall(DATA_EXPLORER))?;
    publish(
        &fake,
        REPO,
        TAG,
        &trusted,
        vec![FakeAsset::new(&asset, data_explorer_build("")?)],
        true,
    );
    let preview = hub.run(hub.supervisor.preview(REPO, None))?;
    assert_eq!(preview.key_status, KeyStatus::Catalog);
    for path in staged_paths(root.path())? {
        fs::remove_file(path)?;
    }
    let refused = hub.run(hub.supervisor.install(REPO, None, &preview.sha256));
    assert_eq!(refusal(refused), Some("hub_preview_gone"));
    assert!(HubDir::at(root.path()).read_registry()?.get(DATA_EXPLORER).is_none());
    Ok(())
}

#[test]
fn a_starting_hub_keeps_the_previews_of_running_processes() -> TestResult {
    let Some(target) = Target::CURRENT else { return Ok(()) };
    let root = tempfile::tempdir()?;
    let fake = FakeGitHub::start()?;
    let key = test_key(19)?;
    let asset = asset_of(DATA_EXPLORER, target)?;
    let build = FakeAsset::new(&asset, data_explorer_build("")?);
    publish(&fake, REPO, TAG, &key, vec![build], true);

    // Left by processes that ended (no process has id 1 or 2 here): an area
    // whose lock nobody holds, an area without a lock, a stray file.
    let staging = HubDir::at(root.path()).staging_dir();
    let unlocked = staging.join("1-0");
    fs::create_dir_all(unlocked.join("3"))?;
    fs::write(unlocked.join("lock"), "")?;
    fs::write(unlocked.join("3").join(&asset), "old")?;
    let lockless = staging.join("2-0");
    fs::create_dir_all(&lockless)?;
    fs::write(lockless.join(&asset), "old")?;
    let stray = staging.join("stray");
    fs::write(&stray, "old")?;

    // Another process's preview, waiting for its key to be accepted, as an
    // `eario-hub install` waits for its terminal.
    let command = RemoteHub::new(root.path(), &fake)?;
    let preview = command.run(command.supervisor.preview(REPO, None))?;
    assert_eq!(staged_files(root.path())?, 4);

    // The hub below reaches the real GitHub; it must not check for updates.
    command.run(command.supervisor.set_check_updates_at_start(false))?;
    let hub = TestHub::start(root.path())?;
    assert!(!unlocked.exists() && !lockless.exists() && !stray.exists(), "stale ones are removed");
    assert_eq!(staged_files(root.path())?, 1, "the running process's preview is kept");
    command.run(command.supervisor.accept_key(REPO, None, &preview.key_sha256))?;
    command.run(command.supervisor.install(REPO, None, &preview.sha256))?;
    assert_eq!(hub.dashboard(DATA_EXPLORER)?.source, Source::Github { repo: REPO.to_owned() });

    // A process removes its own staging area when it ends.
    drop(command);
    assert_eq!(fs::read_dir(&staging)?.count(), 0, "no staging area is left");
    Ok(())
}

#[test]
fn a_command_waits_while_another_process_changes_the_hub_directory() -> TestResult {
    let root = tempfile::tempdir()?;
    let hub_dir = HubDir::at(root.path());
    let lock = hub_dir.lock()?;
    let child = hub_command(root.path())
        .args(["check-at-start", "off"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let mut command = HubProcess(child);

    // While this process holds the lock, the command cannot read the
    // settings to change them: it is still waiting, whatever time it took
    // to start, and has written nothing.
    thread::sleep(Duration::from_secs(1));
    assert!(command.0.try_wait()?.is_none(), "the command did not wait for the lock");
    assert!(!hub_dir.settings_path().exists(), "the settings were written under the lock");

    drop(lock);
    let status = exit_status(&mut command.0)?;
    assert!(status.success(), "{status}");
    let settings: Option<HubSettings> = hub_dir::read_json_at(&hub_dir.settings_path())?;
    assert_eq!(settings.map(|settings| settings.check_updates_at_start), Some(false));
    Ok(())
}

#[test]
fn a_first_install_without_a_slug_needs_exactly_one_asset_for_this_machine() -> TestResult {
    let Some(target) = Target::CURRENT else { return Ok(()) };
    let root = tempfile::tempdir()?;
    let fake = FakeGitHub::start()?;
    let key = test_key(8)?;
    let hub = RemoteHub::new(root.path(), &fake)?;
    let other = Target::ALL.into_iter().find(|other| *other != target).ok_or("another target")?;

    let elsewhere = FakeAsset::new(&asset_of(DATA_EXPLORER, other)?, "x");
    publish(&fake, REPO, "v1.0.0", &key, vec![elsewhere], true);
    let error = hub.run(hub.supervisor.preview(REPO, None)).err().ok_or("no asset")?;
    assert_eq!(error.code(), "hub_no_asset");
    assert!(error.to_string().contains(target.triple()), "{error}");

    let two = vec![
        FakeAsset::new(&asset_of(DATA_EXPLORER, target)?, "x"),
        FakeAsset::new(&asset_of(SIGNAL_LAB, target)?, "y"),
    ];
    publish(&fake, REPO, "v1.0.0", &key, two, true);
    let error = hub.run(hub.supervisor.preview(REPO, None)).err().ok_or("two assets")?;
    assert_eq!(error.code(), "hub_ambiguous_asset");
    let message = error.to_string();
    assert!(message.contains("data-explorer, signal-lab"), "names the slugs: {message}");
    assert!(message.contains("choose the one to install by its slug"), "{message}");
    // A slug the release does not have.
    let error = hub.run(hub.supervisor.preview(REPO, Some("other"))).err().ok_or("no such slug")?;
    assert_eq!(error.code(), "hub_no_asset");
    assert!(error.to_string().contains(&asset_of("other", target)?), "{error}");
    let invalid = hub.run(hub.supervisor.preview(REPO, Some("../x")));
    assert_eq!(refusal(invalid), Some("hub_invalid_slug"));
    assert_eq!(fake.requests().iter().filter(|path| path.starts_with("/assets/")).count(), 0);
    assert_nothing_installed(root.path(), DATA_EXPLORER)?;
    Ok(())
}

/// Serves a release of `repo` with both examples, signed by `key` with its
/// `minisign.pub`; data-explorer and signal-lab get `marker` appended.
fn publish_suite(fake: &FakeGitHub, key: &TestKey, marker: &str) -> Result<(), Failure> {
    let target = Target::CURRENT.ok_or("no target")?;
    let mut signal_lab = fs::read(example(SIGNAL_LAB)?)?;
    signal_lab.extend_from_slice(marker.as_bytes());
    let assets = vec![
        FakeAsset::new(&asset_of(DATA_EXPLORER, target)?, data_explorer_build(marker)?),
        FakeAsset::new(&asset_of(SIGNAL_LAB, target)?, signal_lab),
    ];
    publish(fake, SUITE, TAG, key, assets, true);
    Ok(())
}

/// A repository that publishes both examples in one release.
const SUITE: &str = "lab/suite";

#[test]
fn two_dashboards_of_one_repository_install_side_by_side_and_update_on_their_own() -> TestResult {
    let Some(target) = Target::CURRENT else { return Ok(()) };
    let root = tempfile::tempdir()?;
    let fake = FakeGitHub::start()?;
    let key = test_key(20)?;
    publish_suite(&fake, &key, "")?;
    let hub = RemoteHub::new(root.path(), &fake)?;
    let registry = || HubDir::at(root.path()).read_registry();

    // Without a slug, the hub cannot tell which one: refused before any
    // download, with the slugs to choose from.
    let error = hub.run(hub.supervisor.preview(SUITE, None)).err().ok_or("ambiguous")?;
    assert_eq!(error.code(), "hub_ambiguous_asset");
    assert!(error.to_string().contains("data-explorer, signal-lab"), "{error}");
    assert_eq!(fake.requests().iter().filter(|path| path.starts_with("/assets/")).count(), 0);

    // The first, by its slug: trust on first use, for the repository.
    let signal_lab = hub.run(hub.supervisor.preview(SUITE, Some(SIGNAL_LAB)))?;
    assert_eq!((signal_lab.slug.as_str(), signal_lab.key_status), (SIGNAL_LAB, KeyStatus::Unknown));
    assert_eq!(signal_lab.asset, asset_of(SIGNAL_LAB, target)?);
    assert_eq!(signal_lab.manifest, None);
    // Another dashboard of the repository, previewed meanwhile, waits beside
    // it: one preview per dashboard.
    let explorer = hub.run(hub.supervisor.preview(SUITE, Some(DATA_EXPLORER)))?;
    assert_eq!(explorer.key_status, KeyStatus::Unknown);
    assert_eq!(staged_files(root.path())?, 2);
    let accepted =
        hub.run(hub.supervisor.accept_key(SUITE, Some(SIGNAL_LAB), &signal_lab.key_sha256))?;
    let manifest = accepted.manifest.clone().ok_or("the full preview")?;
    assert_eq!(manifest.slug, SIGNAL_LAB, "the manifest gives the slug asked for");
    // Accepting the key of one preview leaves the other as it was.
    let wrong = hub.run(hub.supervisor.install(SUITE, Some(DATA_EXPLORER), &explorer.sha256));
    assert_eq!(refusal(wrong), Some("hub_key_not_accepted"));
    // The SHA-256 of one dashboard does not install another.
    let crossed = hub.run(hub.supervisor.install(SUITE, Some(SIGNAL_LAB), &explorer.sha256));
    assert_eq!(refusal(crossed), Some("hub_preview_mismatch"));
    hub.run(hub.supervisor.install(SUITE, Some(SIGNAL_LAB), &signal_lab.sha256))?;
    let entry = registry()?.get(SIGNAL_LAB).cloned().ok_or("signal-lab")?;
    assert_eq!(entry.source, Source::Github { repo: SUITE.to_owned() });
    assert_eq!(entry.pinned_key, Some(key.public_key()));

    // The second: the repository's pinned key decides, so it is trusted and
    // run at once. The key-pending preview made before is replaced.
    let explorer = hub.run(hub.supervisor.preview(SUITE, Some(DATA_EXPLORER)))?;
    assert_eq!(explorer.key_status, KeyStatus::Pinned);
    let manifest = explorer.manifest.clone().ok_or("a pinned key: the full preview")?;
    assert_eq!(manifest.slug, DATA_EXPLORER);
    assert_eq!(staged_files(root.path())?, 1);
    // The two-segment install takes the repository's preview of that SHA-256.
    hub.run(hub.supervisor.install(SUITE, None, &explorer.sha256))?;
    let installed = registry()?;
    assert_eq!(installed.dashboards.len(), 2);
    for slug in [SIGNAL_LAB, DATA_EXPLORER] {
        let entry = installed.get(slug).ok_or("both installed")?;
        assert_eq!(entry.source, Source::Github { repo: SUITE.to_owned() }, "{slug}");
        assert_eq!(entry.pinned_key, Some(key.public_key()), "{slug}");
    }
    let again = hub.run(hub.supervisor.preview(SUITE, Some(DATA_EXPLORER)));
    assert_eq!(refusal(again), Some("hub_already_installed"));
    let ambiguous = hub.run(hub.supervisor.preview(SUITE, None));
    assert_eq!(refusal(ambiguous), Some("hub_ambiguous_asset"));

    // Both start, side by side.
    hub.run(hub.supervisor.start(SIGNAL_LAB))?;
    hub.run(hub.supervisor.start(DATA_EXPLORER))?;
    hub.wait_for(SIGNAL_LAB, DashboardState::Running)?;
    hub.wait_for(DATA_EXPLORER, DashboardState::Running)?;
    hub.run(hub.supervisor.stop_all())?;

    // A check reads the repository's release once, for both.
    set_installed_version(root.path(), DATA_EXPLORER, EARLIER_1)?;
    let releases = |fake: &FakeGitHub| {
        let path = format!("/repos/{SUITE}/releases/latest");
        fake.requests().iter().filter(|requested| **requested == path).count()
    };
    let before = releases(&fake);
    let check = hub.run(hub.supervisor.check())?;
    assert_eq!(releases(&fake), before + 1, "one request for the repository");
    let found: Vec<(&str, Option<&str>)> = check
        .dashboards
        .iter()
        .map(|update| (update.slug.as_str(), update.available_version.as_deref()))
        .collect();
    assert_eq!(found, [(SIGNAL_LAB, None), (DATA_EXPLORER, Some(EXAMPLE_VERSION))]);

    // An update of one takes its own asset and leaves the other alone.
    publish_suite(&fake, &key, "v2")?;
    let untouched = registry()?.get(SIGNAL_LAB).cloned().ok_or("signal-lab")?;
    hub.run(hub.supervisor.update(DATA_EXPLORER))?;
    let updated = registry()?;
    let explorer = updated.get(DATA_EXPLORER).ok_or("data-explorer")?;
    assert_eq!(explorer.manifest.slug, DATA_EXPLORER);
    assert_eq!(fs::read(&explorer.path)?, data_explorer_build("v2")?);
    assert!(explorer.previous.is_some());
    assert_eq!(updated.get(SIGNAL_LAB), Some(&untouched), "the other is not updated");
    hub.run(hub.supervisor.rollback(DATA_EXPLORER))?;
    let rolled_back = registry()?;
    assert_eq!(rolled_back.get(SIGNAL_LAB), Some(&untouched), "nor rolled back");
    assert_eq!(
        fs::read(&rolled_back.get(DATA_EXPLORER).ok_or("entry")?.path)?,
        data_explorer_build("")?
    );

    // Uninstalling one keeps the repository's pin with the other.
    hub.run(hub.supervisor.uninstall(DATA_EXPLORER))?;
    let explorer = hub.run(hub.supervisor.preview(SUITE, Some(DATA_EXPLORER)))?;
    assert_eq!(explorer.key_status, KeyStatus::Pinned);
    Ok(())
}

#[test]
fn a_key_change_seen_for_one_dashboard_of_a_repository_holds_for_the_others() -> TestResult {
    let Some(_) = Target::CURRENT else { return Ok(()) };
    let root = tempfile::tempdir()?;
    let fake = FakeGitHub::start()?;
    let (first, second) = (test_key(21)?, test_key(22)?);
    publish_suite(&fake, &first, "")?;
    serve_catalog(&fake, eario_hub::remote::DEFAULT_CATALOG, &[(SUITE, first.public_key())]);
    let hub = RemoteHub::new(root.path(), &fake)?;
    let registry = || HubDir::at(root.path()).read_registry();
    for slug in [SIGNAL_LAB, DATA_EXPLORER] {
        let preview = hub.run(hub.supervisor.preview(SUITE, Some(slug)))?;
        let status = if slug == SIGNAL_LAB { KeyStatus::Catalog } else { KeyStatus::Pinned };
        assert_eq!(preview.key_status, status, "{slug}");
        hub.run(hub.supervisor.install(SUITE, Some(slug), &preview.sha256))?;
        set_installed_version(root.path(), slug, EARLIER_1)?;
    }

    // A stale preview of signal-lab, signed with the first key.
    publish_suite(&fake, &first, "v2")?;
    let stale = hub.run(hub.supervisor.preview_update(SIGNAL_LAB))?;
    assert_eq!(stale.key_status, KeyStatus::Pinned);

    // The second key, for both: changed for each, whatever the catalog says.
    publish_suite(&fake, &second, "v3")?;
    let explorer = hub.run(hub.supervisor.preview_update(DATA_EXPLORER))?;
    assert_eq!((explorer.key_status, explorer.manifest.as_ref()), (KeyStatus::Changed, None));
    // Accepted for data-explorer, it is accepted for the repository.
    hub.run(hub.supervisor.accept_key(SUITE, Some(DATA_EXPLORER), &explorer.key_sha256))?;
    hub.run(hub.supervisor.apply_update(DATA_EXPLORER, &explorer.sha256))?;
    let pinned = registry()?;
    for slug in [SIGNAL_LAB, DATA_EXPLORER] {
        let entry = pinned.get(slug).ok_or("installed")?;
        assert_eq!(
            entry.pinned_key,
            Some(second.public_key()),
            "pinned for the repository: {slug}"
        );
    }
    // The preview signed with the first key no longer installs: the
    // repository moved to the second.
    let refused = hub.run(hub.supervisor.apply_update(SIGNAL_LAB, &stale.sha256));
    let error = refused.err().ok_or("a stale key must be refused")?;
    assert_eq!(error.code(), "hub_key_changed_meanwhile");
    assert_eq!(registry()?.get(SIGNAL_LAB), pinned.get(SIGNAL_LAB), "signal-lab unchanged");
    // Previewed again, signal-lab's update is signed with the pinned key.
    let signal_lab = hub.run(hub.supervisor.preview_update(SIGNAL_LAB))?;
    assert_eq!(signal_lab.key_status, KeyStatus::Pinned);
    hub.run(hub.supervisor.apply_update(SIGNAL_LAB, &signal_lab.sha256))?;

    // A third key, seen for signal-lab and accepted there, is accepted for
    // data-explorer too: its preview is `accepted`, and installs.
    let third = test_key(23)?;
    publish_suite(&fake, &third, "v4")?;
    for slug in [SIGNAL_LAB, DATA_EXPLORER] {
        set_installed_version(root.path(), slug, EARLIER_2)?;
    }
    let one = hub.run(hub.supervisor.preview_update(SIGNAL_LAB))?;
    assert_eq!(one.key_status, KeyStatus::Changed);
    let other = hub.run(hub.supervisor.preview_update(DATA_EXPLORER))?;
    assert_eq!(other.key_status, KeyStatus::Changed, "a change for one is a change for both");
    // Without a slug, accept-key takes the repository's latest preview.
    let accepted = hub.run(hub.supervisor.accept_key(SUITE, None, &other.key_sha256))?;
    assert_eq!((accepted.slug.as_str(), accepted.key_status), (DATA_EXPLORER, KeyStatus::Accepted));
    let one = hub.run(hub.supervisor.preview_update(SIGNAL_LAB))?;
    assert_eq!(one.key_status, KeyStatus::Accepted, "accepted for the repository");
    assert!(one.manifest.is_some(), "so it is run at once");
    hub.run(hub.supervisor.apply_update(SIGNAL_LAB, &one.sha256))?;
    hub.run(hub.supervisor.apply_update(DATA_EXPLORER, &accepted.sha256))?;
    for (slug, entry) in registry()?.dashboards.iter().map(|entry| (&entry.slug, entry)) {
        assert_eq!(entry.pinned_key, Some(third.public_key()), "{slug}");
    }
    Ok(())
}

/// Installs data-explorer from `REPO` signed by `key`, through the
/// supervisor, with the catalog vouching for the key.
fn install_from_github(
    hub: &RemoteHub,
    fake: &FakeGitHub,
    key: &TestKey,
    marker: &str,
) -> TestResult {
    let Some(target) = Target::CURRENT else { return Err("no target".into()) };
    let asset = asset_of(DATA_EXPLORER, target)?;
    let build = data_explorer_build(marker)?;
    publish(fake, REPO, TAG, key, vec![FakeAsset::new(&asset, build)], true);
    serve_catalog(fake, eario_hub::remote::DEFAULT_CATALOG, &[(REPO, key.public_key())]);
    let preview = hub.run(hub.supervisor.preview(REPO, None))?;
    hub.run(hub.supervisor.install(REPO, None, &preview.sha256))?;
    Ok(())
}

#[test]
fn an_update_keeps_one_previous_version_and_a_rollback_restores_it() -> TestResult {
    let Some(target) = Target::CURRENT else { return Ok(()) };
    let root = tempfile::tempdir()?;
    let fake = FakeGitHub::start()?;
    let key = test_key(9)?;
    let hub = RemoteHub::new(root.path(), &fake)?;
    let hub_dir = HubDir::at(root.path());
    let asset = asset_of(DATA_EXPLORER, target)?;
    install_from_github(&hub, &fake, &key, "build 1")?;
    set_installed_version(root.path(), DATA_EXPLORER, EARLIER_1)?;
    let first = hub_dir.read_registry()?.get(DATA_EXPLORER).cloned().ok_or("entry")?;
    assert_eq!(refusal(hub.run(hub.supervisor.rollback(DATA_EXPLORER))), Some("hub_no_previous"));

    // First update: the installed version becomes the previous one.
    let second_build = data_explorer_build("build 2")?;
    publish(&fake, REPO, TAG, &key, vec![FakeAsset::new(&asset, second_build.clone())], true);
    let preview = hub.run(hub.supervisor.preview_update(DATA_EXPLORER))?;
    assert_eq!(preview.key_status, KeyStatus::Pinned);
    hub.run(hub.supervisor.apply_update(DATA_EXPLORER, &preview.sha256))?;
    let second = hub_dir.read_registry()?.get(DATA_EXPLORER).cloned().ok_or("entry")?;
    assert_eq!(fs::read(&second.path)?, second_build);
    assert_eq!(second.manifest.version.as_deref(), Some(EXAMPLE_VERSION), "from the new manifest");
    let previous = second.previous.clone().ok_or("a previous version")?;
    assert_eq!((previous.size, previous.sha256.as_str()), (first.size, first.sha256.as_str()));
    assert_eq!(previous.manifest.version.as_deref(), Some(EARLIER_1));
    let first_name = first.path.file_name().and_then(|name| name.to_str()).ok_or("name")?;
    assert_eq!(previous.file_name, first_name);
    assert_eq!(bin_files(root.path(), DATA_EXPLORER)?.len(), 2);
    assert!(hub.state(DATA_EXPLORER)?.has_previous);

    // Second update: the first version is removed.
    set_installed_version(root.path(), DATA_EXPLORER, EARLIER_2)?;
    let second = hub_dir.read_registry()?.get(DATA_EXPLORER).cloned().ok_or("entry")?;
    let third_build = data_explorer_build("build 3")?;
    publish(&fake, REPO, TAG, &key, vec![FakeAsset::new(&asset, third_build.clone())], true);
    hub.run(hub.supervisor.update(DATA_EXPLORER))?;
    let third = hub_dir.read_registry()?.get(DATA_EXPLORER).cloned().ok_or("entry")?;
    assert_eq!(fs::read(&third.path)?, third_build);
    let previous = third.previous.clone().ok_or("a previous version")?;
    assert_eq!(previous.sha256, second.sha256);
    let mut expected: Vec<String> = [&third.path, &second.path]
        .iter()
        .filter_map(|path| path.file_name().and_then(|name| name.to_str()).map(str::to_owned))
        .collect();
    expected.sort();
    assert_eq!(bin_files(root.path(), DATA_EXPLORER)?, expected, "exactly one previous version");
    assert!(!first.path.exists(), "the older version is removed");
    assert_eq!(staged_files(root.path())?, 0);

    // A rollback restores the previous version and removes the newer copy.
    hub.run(hub.supervisor.rollback(DATA_EXPLORER))?;
    let restored = hub_dir.read_registry()?.get(DATA_EXPLORER).cloned().ok_or("entry")?;
    assert_eq!(restored.path, second.path);
    assert_eq!((restored.size, restored.sha256.as_str()), (second.size, second.sha256.as_str()));
    assert_eq!(restored.manifest.version.as_deref(), Some(EARLIER_2));
    assert_eq!(restored.previous, None);
    assert_eq!(restored.pinned_key, Some(key.public_key()));
    assert!(!third.path.exists(), "the newer copy is removed");
    assert_eq!(bin_files(root.path(), DATA_EXPLORER)?.len(), 1);
    assert_eq!(refusal(hub.run(hub.supervisor.rollback(DATA_EXPLORER))), Some("hub_no_previous"));
    // The restored copy passes the check before each start.
    hub.run(hub.supervisor.start(DATA_EXPLORER))?;
    hub.wait_for(DATA_EXPLORER, DashboardState::Running)?;
    hub.run(hub.supervisor.stop(DATA_EXPLORER))?;

    // A previous version whose copy was changed is not restored.
    publish(&fake, REPO, TAG, &key, vec![FakeAsset::new(&asset, third_build)], true);
    hub.run(hub.supervisor.update(DATA_EXPLORER))?;
    fs::write(&second.path, b"changed")?;
    let error = hub.run(hub.supervisor.rollback(DATA_EXPLORER)).err().ok_or("changed copy")?;
    assert_eq!(error.code(), "hub_previous_unusable");
    // Nor one the registry names by a path.
    let mut registry = hub_dir.read_registry()?;
    let mut entry = registry.get(DATA_EXPLORER).cloned().ok_or("entry")?;
    if let Some(previous) = entry.previous.as_mut() {
        previous.file_name = "../../registry.json".to_owned();
    }
    registry.insert(entry);
    hub_dir.write_registry(&registry)?;
    let error = hub.run(hub.supervisor.rollback(DATA_EXPLORER)).err().ok_or("a path")?;
    assert_eq!(error.code(), "hub_foreign_executable");
    assert!(hub_dir.registry_path().exists());
    Ok(())
}

#[test]
fn an_update_is_refused_while_the_dashboard_runs() -> TestResult {
    let Some(target) = Target::CURRENT else { return Ok(()) };
    let root = tempfile::tempdir()?;
    let fake = FakeGitHub::start()?;
    let key = test_key(10)?;
    let hub = RemoteHub::new(root.path(), &fake)?;
    install_from_github(&hub, &fake, &key, "build 1")?;
    set_installed_version(root.path(), DATA_EXPLORER, EARLIER_1)?;
    let asset = asset_of(DATA_EXPLORER, target)?;
    publish(
        &fake,
        REPO,
        TAG,
        &key,
        vec![FakeAsset::new(&asset, data_explorer_build("build 2")?)],
        true,
    );
    let preview = hub.run(hub.supervisor.preview_update(DATA_EXPLORER))?;

    hub.run(hub.supervisor.start(DATA_EXPLORER))?;
    let starting = hub.run(hub.supervisor.update(DATA_EXPLORER));
    assert!(matches!(refusal(starting), Some("hub_starting" | "hub_running")));
    hub.wait_for(DATA_EXPLORER, DashboardState::Running)?;
    let before = HubDir::at(root.path()).read_registry()?;
    let downloads = asset_downloads(&fake, &asset);
    assert_eq!(refusal(hub.run(hub.supervisor.update(DATA_EXPLORER))), Some("hub_running"));
    assert_eq!(asset_downloads(&fake, &asset), downloads, "refused before the download");
    let apply = hub.run(hub.supervisor.apply_update(DATA_EXPLORER, &preview.sha256));
    assert_eq!(refusal(apply), Some("hub_running"));
    assert_eq!(refusal(hub.run(hub.supervisor.rollback(DATA_EXPLORER))), Some("hub_running"));
    assert_eq!(HubDir::at(root.path()).read_registry()?, before, "nothing was changed");

    hub.run(hub.supervisor.stop(DATA_EXPLORER))?;
    hub.run(hub.supervisor.apply_update(DATA_EXPLORER, &preview.sha256))?;
    assert!(hub.state(DATA_EXPLORER)?.has_previous);
    // While the rolled-back copy runs, it is not rolled back again either.
    hub.run(hub.supervisor.start(DATA_EXPLORER))?;
    hub.wait_for(DATA_EXPLORER, DashboardState::Running)?;
    assert_eq!(refusal(hub.run(hub.supervisor.rollback(DATA_EXPLORER))), Some("hub_running"));
    hub.run(hub.supervisor.stop(DATA_EXPLORER))?;
    hub.run(hub.supervisor.rollback(DATA_EXPLORER))?;
    Ok(())
}

#[test]
fn an_update_to_an_older_or_incomparable_version_is_refused() -> TestResult {
    let Some(target) = Target::CURRENT else { return Ok(()) };
    let root = tempfile::tempdir()?;
    let fake = FakeGitHub::start()?;
    let key = test_key(11)?;
    let hub = RemoteHub::new(root.path(), &fake)?;
    let asset = asset_of(DATA_EXPLORER, target)?;
    install_from_github(&hub, &fake, &key, "build 1")?;
    let older = vec![FakeAsset::new(&asset, data_explorer_build("build 0")?)];

    // Installed a version ahead of the release, which is older: refused by its tag,
    // before any download.
    set_installed_version(root.path(), DATA_EXPLORER, AHEAD)?;
    publish(&fake, REPO, TAG, &key, older.clone(), true);
    let downloads = asset_downloads(&fake, &asset);
    let error = hub.run(hub.supervisor.update(DATA_EXPLORER)).err().ok_or("a downgrade")?;
    assert_eq!(error.code(), "hub_not_newer");
    assert!(error.to_string().contains(AHEAD), "{error}");
    assert_eq!(asset_downloads(&fake, &asset), downloads);
    // The same version is no update either.
    set_installed_version(root.path(), DATA_EXPLORER, EXAMPLE_VERSION)?;
    assert_eq!(refusal(hub.run(hub.supervisor.update(DATA_EXPLORER))), Some("hub_not_newer"));

    // A newer tag on an older executable: its signed manifest decides.
    set_installed_version(root.path(), DATA_EXPLORER, AHEAD)?;
    publish(&fake, REPO, "v900.0.0", &key, older.clone(), true);
    let error = hub.run(hub.supervisor.update(DATA_EXPLORER)).err().ok_or("an older executable")?;
    assert_eq!(error.code(), "hub_not_newer");
    assert_eq!(asset_downloads(&fake, &asset), downloads + 1, "refused after the download");

    // Without SemVer the hub cannot tell an update from a downgrade.
    set_installed_version(root.path(), DATA_EXPLORER, "nightly")?;
    publish(&fake, REPO, "latest", &key, older, true);
    let refused = hub.run(hub.supervisor.update(DATA_EXPLORER));
    assert_eq!(refusal(refused), Some("hub_version_not_comparable"));

    let entry =
        HubDir::at(root.path()).read_registry()?.get(DATA_EXPLORER).cloned().ok_or("entry")?;
    assert_eq!(entry.previous, None, "nothing was installed");
    assert_eq!(bin_files(root.path(), DATA_EXPLORER)?.len(), 1);
    assert_eq!(staged_files(root.path())?, 0);
    Ok(())
}

#[test]
fn a_check_reports_updates_and_what_could_not_be_checked() -> TestResult {
    let Some(target) = Target::CURRENT else { return Ok(()) };
    let root = tempfile::tempdir()?;
    let fake = FakeGitHub::start()?;
    let key = test_key(12)?;
    let hub = RemoteHub::new(root.path(), &fake)?;
    install_from_github(&hub, &fake, &key, "build 1")?;
    install(root.path(), SIGNAL_LAB)?;
    assert_eq!(hub.run(hub.supervisor.last_check())?, None);

    set_installed_version(root.path(), DATA_EXPLORER, EARLIER_1)?;
    let downloads = |fake: &FakeGitHub| {
        fake.requests().iter().filter(|path| path.starts_with("/assets/")).count()
    };
    let before = downloads(&fake);
    let check = hub.run(hub.supervisor.check())?;
    assert_eq!(downloads(&fake), before, "a check downloads nothing");
    assert_eq!(check.dashboards.len(), 1, "only dashboards installed from GitHub: {check:?}");
    let found = &check.dashboards[0];
    assert_eq!((found.slug.as_str(), found.repo.as_str()), (DATA_EXPLORER, REPO));
    assert_eq!(found.latest_version.as_deref(), Some(EXAMPLE_VERSION));
    assert_eq!(found.available_version.as_deref(), Some(EXAMPLE_VERSION));
    assert_eq!(found.error, None);
    assert!(check.checked_at > 0);

    // The list reads the stored result.
    let info = hub.run(hub.supervisor.list())?;
    assert_eq!(info.last_check.as_ref(), Some(&check));
    let listed =
        |slug: &str| info.dashboards.iter().find(|dashboard| dashboard.slug == slug).cloned();
    let explorer = listed(DATA_EXPLORER).ok_or("listed")?;
    assert_eq!(explorer.available_version.as_deref(), Some(EXAMPLE_VERSION));
    assert_eq!(listed(SIGNAL_LAB).ok_or("listed")?.available_version, None);
    assert_eq!(hub.run(hub.supervisor.last_check())?, Some(check.clone()));

    // Once updated, nothing is available any more, without a new check.
    let asset = asset_of(DATA_EXPLORER, target)?;
    publish(
        &fake,
        REPO,
        TAG,
        &key,
        vec![FakeAsset::new(&asset, data_explorer_build("build 2")?)],
        true,
    );
    hub.run(hub.supervisor.update(DATA_EXPLORER))?;
    assert_eq!(hub.state(DATA_EXPLORER)?.available_version, None);

    // Up to date.
    let check = hub.run(hub.supervisor.check())?;
    assert_eq!(check.dashboards[0].available_version, None);
    assert_eq!(check.dashboards[0].latest_version.as_deref(), Some(EXAMPLE_VERSION));

    // Offline: the dashboard could not be checked, and the hub still lists.
    drop(fake);
    let check = hub.run(hub.supervisor.check())?;
    let error = check.dashboards[0].error.clone().ok_or("could not check")?;
    assert_eq!(error.code, "github_offline");
    assert!(error.message.contains(REPO), "{}", error.message);
    assert_eq!(check.dashboards[0].available_version, None);
    let info = hub.run(hub.supervisor.list())?;
    assert_eq!(info.dashboards.len(), 2);
    assert_eq!(info.last_check, Some(check));
    Ok(())
}

#[test]
fn the_hub_checks_for_updates_at_start_unless_turned_off() -> TestResult {
    let Some(_) = Target::CURRENT else { return Ok(()) };
    let root = tempfile::tempdir()?;
    let fake = FakeGitHub::start()?;
    let key = test_key(13)?;
    {
        let hub = RemoteHub::new(root.path(), &fake)?;
        install_from_github(&hub, &fake, &key, "build 1")?;
        set_installed_version(root.path(), DATA_EXPLORER, EARLIER_1)?;
        let settings = hub.run(hub.supervisor.set_check_updates_at_start(false))?;
        assert!(!settings.check_updates_at_start);
    }
    let latest = format!("/repos/{REPO}/releases/latest");
    let checked =
        |fake: &FakeGitHub| fake.requests().iter().filter(|path| **path == latest).count();
    let before = checked(&fake);

    // Off: the hub starts and reads nothing from GitHub.
    let supervisor = Supervisor::with_github(
        HubDir::at(root.path()),
        SupervisorOptions::default(),
        fake_client(&fake)?,
    )?;
    let hub = TestHub::with_supervisor(supervisor)?;
    assert_eq!(hub.dashboard(DATA_EXPLORER)?.available_version, None);
    thread::sleep(Duration::from_secs(2));
    assert_eq!(checked(&fake), before, "no request to GitHub");
    assert!(!HubDir::at(root.path()).updates_path().exists());
    drop(hub);

    // On (the default): checked in the background once the hub serves.
    {
        let remote = RemoteHub::new(root.path(), &fake)?;
        remote.run(remote.supervisor.set_check_updates_at_start(true))?;
    }
    let supervisor = Supervisor::with_github(
        HubDir::at(root.path()),
        SupervisorOptions::default(),
        fake_client(&fake)?,
    )?;
    let hub = TestHub::with_supervisor(supervisor)?;
    let started = Instant::now();
    while hub.dashboard(DATA_EXPLORER)?.available_version.is_none() {
        if started.elapsed() > TIMEOUT {
            return Err("the hub did not check for updates at start".into());
        }
        thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(checked(&fake), before + 1);
    assert_eq!(hub.list()?.last_check.map(|check| check.dashboards.len()), Some(1));
    drop(hub);

    // Offline at start: logged, "could not check", and the hub serves.
    drop(fake);
    let offline = FakeGitHub::start()?;
    let gone = offline.base_url();
    drop(offline);
    let endpoint = Endpoint::loopback(&gone).ok_or("loopback")?;
    let client: Arc<dyn GitHub> =
        Arc::new(GitHubClient::with_options(endpoint, ClientOptions::default())?);
    let supervisor =
        Supervisor::with_github(HubDir::at(root.path()), SupervisorOptions::default(), client)?;
    let hub = TestHub::with_supervisor(supervisor)?;
    let started = Instant::now();
    loop {
        let check = hub.list()?.last_check.ok_or("checked before")?;
        if check.dashboards[0].error.is_some() {
            assert_eq!(
                check.dashboards[0].error.as_ref().map(|error| error.code.as_str()),
                Some("github_offline")
            );
            break;
        }
        if started.elapsed() > TIMEOUT {
            return Err("the hub did not record that it could not check".into());
        }
        thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(hub.dashboard(DATA_EXPLORER)?.state, DashboardState::Stopped);
    Ok(())
}

#[test]
fn catalogs_are_added_and_removed_by_repository() -> TestResult {
    let root = tempfile::tempdir()?;
    let fake = FakeGitHub::start()?;
    let key = test_key(14)?;
    serve_catalog(&fake, "lab/catalog", &[(REPO, key.public_key())]);
    let hub = RemoteHub::new(root.path(), &fake)?;
    let default = eario_hub::remote::DEFAULT_CATALOG;

    assert_eq!(hub.run(hub.supervisor.catalogs())?, [default]);
    // The default catalog is not served: listed, with why.
    let listing = hub.run(hub.supervisor.catalog_listing())?;
    assert_eq!(
        listing[0].error.as_ref().map(|error| error.code.as_str()),
        Some("github_not_found")
    );

    let error = hub.run(hub.supervisor.add_catalog("lab/nothing")).err().ok_or("no catalog")?;
    assert_eq!(error.code(), "github_not_found");
    for bad in ["../x", "https://github.com/lab/catalog", "lab"] {
        assert_eq!(refusal(hub.run(hub.supervisor.add_catalog(bad))), Some("github_invalid_repo"));
    }
    assert_eq!(hub.run(hub.supervisor.add_catalog("lab/catalog"))?, [default, "lab/catalog"]);
    assert_eq!(hub.run(hub.supervisor.add_catalog("Lab/Catalog"))?, [default, "lab/catalog"]);
    let listing = hub.run(hub.supervisor.catalog_listing())?;
    assert_eq!(listing[1].name.as_deref(), Some("Lab"));
    assert_eq!(listing[1].dashboards[0].repo, REPO);

    assert_eq!(hub.run(hub.supervisor.remove_catalog(default))?, ["lab/catalog"]);
    let error = hub.run(hub.supervisor.remove_catalog(default)).err().ok_or("removed")?;
    assert_eq!(error.code(), "hub_unknown_catalog");
    // Stored in the hub directory: another supervisor reads the same list.
    let other = RemoteHub::new(root.path(), &fake)?;
    assert_eq!(other.run(other.supervisor.catalogs())?, ["lab/catalog"]);
    assert_eq!(hub.run(hub.supervisor.remove_catalog("lab/catalog"))?, Vec::<String>::new());
    assert!(hub.run(hub.supervisor.catalog_listing())?.is_empty());
    Ok(())
}

// The routes of installs and updates from GitHub (ADR 0013), over HTTP: thin
// adapters over the supervisor methods tested above, so these tests check
// what the HTTP layer adds: strict path segments, the token, the Origin
// check, and the status of each refusal.

/// Sends one request without a body, with a `Host` header of its own.
fn request_with_host(
    address: &str,
    method: &str,
    path: &str,
    token: &str,
    host: &str,
) -> Result<Answer, Failure> {
    let mut stream = TcpStream::connect(address)?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nx-eario-token: {token}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let status = response.split_whitespace().nth(1).ok_or("answer without a status")?.parse()?;
    let body = response.split_once("\r\n\r\n").map(|(_, body)| body.to_owned()).unwrap_or_default();
    Ok(Answer { status, body })
}

/// Sends one request without a body, with an `Origin` header.
fn request_from_origin(
    address: &str,
    method: &str,
    path: &str,
    token: &str,
    origin: &str,
) -> Result<Answer, Failure> {
    let mut stream = TcpStream::connect(address)?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nOrigin: {origin}\r\nx-eario-token: {token}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let status = response.split_whitespace().nth(1).ok_or("answer without a status")?.parse()?;
    let body = response.split_once("\r\n\r\n").map(|(_, body)| body.to_owned()).unwrap_or_default();
    Ok(Answer { status, body })
}

/// A hub served in this process that reaches the fake GitHub.
fn remote_test_hub(root: &Path, fake: &FakeGitHub) -> Result<TestHub, Failure> {
    let supervisor = Supervisor::with_github(
        HubDir::at(root),
        SupervisorOptions::default(),
        fake_client(fake)?,
    )?;
    TestHub::with_supervisor(supervisor)
}

impl TestHub {
    fn get(&self, path: &str) -> Result<Answer, Failure> {
        request(&self.address, "GET", path, Some(&self.token))
    }

    /// The hub's list from an answer of 200.
    fn info(answer: &Answer) -> Result<HubInfo, Failure> {
        if answer.status != 200 {
            return Err(format!("expected 200, got {}: {}", answer.status, answer.body).into());
        }
        answer.json()
    }
}

/// The JSON of an answer of 200.
fn ok<T: serde::de::DeserializeOwned>(answer: &Answer) -> Result<T, Failure> {
    if answer.status != 200 {
        return Err(format!("expected 200, got {}: {}", answer.status, answer.body).into());
    }
    answer.json()
}

/// The registry entry of `slug` in `root`, which must be there.
fn entry_of(root: &Path, slug: &str) -> Result<hub_dir::RegistryEntry, Failure> {
    let registry = HubDir::at(root).read_registry()?;
    registry.get(slug).cloned().ok_or_else(|| format!("{slug} is not installed").into())
}

/// The SHA-256 the hub identifies `key` by.
fn key_sha256(key: &TestKey) -> Result<String, Failure> {
    Ok(verify::PublicKey::from_base64(REPO, &key.public_key())?.sha256())
}

#[test]
fn remote_routes_take_strict_path_segments_and_nothing_else() -> TestResult {
    let root = tempfile::tempdir()?;
    let fake = FakeGitHub::start()?;
    let hub = remote_test_hub(root.path(), &fake)?;
    let hash = "a".repeat(64);

    // `{owner}/{repo}` is a GitHub repository, nothing else.
    let not_repos = [
        ("..", "data-explorer"),
        ("lab", ".."),
        ("lab", "."),
        ("lab%2Fdata-explorer", "x"),
        ("lab", "data-explorer%2F..%2F.."),
        ("lab", "data%5Cexplorer"),
        ("https:", "github.com"),
        ("lab", "data%20explorer"),
        ("lab", "data-explorer%00"),
        ("lab.x", "data-explorer"),
    ];
    let mut owner_too_long = "a".repeat(40);
    owner_too_long.push_str("/x");
    for (owner, repo) in not_repos {
        let routes = [
            format!("/api/catalogs/{owner}/{repo}/add"),
            format!("/api/catalogs/{owner}/{repo}/remove"),
            format!("/api/sources/{owner}/{repo}/preview"),
            format!("/api/sources/{owner}/{repo}/accept-key/{hash}"),
            format!("/api/sources/{owner}/{repo}/install/{hash}"),
            format!("/api/sources/{owner}/{repo}/dashboards/data-explorer/preview"),
            format!("/api/sources/{owner}/{repo}/dashboards/data-explorer/accept-key/{hash}"),
            format!("/api/sources/{owner}/{repo}/dashboards/data-explorer/install/{hash}"),
        ];
        for path in routes {
            let refusal =
                hub.post(&path)?.refusal(400).map_err(|error| format!("{path}: {error}"))?;
            assert_eq!(refusal.code, "github_invalid_repo", "{path}");
        }
    }
    let path = format!("/api/sources/{owner_too_long}/preview");
    assert_eq!(hub.post(&path)?.refusal(400)?.code, "github_invalid_repo");

    // `{sha256}` and `{key_sha256}`: 64 lower-case hexadecimal digits. A
    // minisign key id (16 digits) is not a key's SHA-256.
    let not_hashes = [
        "..".to_owned(),
        "%2E%2E%2F%2E%2E%2Fbin".to_owned(),
        "A".repeat(64),
        "a".repeat(63),
        "a".repeat(65),
        "g".repeat(64),
        "0123456789abcdef".to_owned(),
        format!("{}%20", "a".repeat(63)),
    ];
    for not_hash in &not_hashes {
        let routes = [
            format!("/api/sources/lab/data-explorer/accept-key/{not_hash}"),
            format!("/api/sources/lab/data-explorer/install/{not_hash}"),
            format!("/api/sources/lab/suite/dashboards/signal-lab/accept-key/{not_hash}"),
            format!("/api/sources/lab/suite/dashboards/signal-lab/install/{not_hash}"),
            format!("/api/dashboards/data-explorer/update/{not_hash}"),
        ];
        for path in routes {
            let refusal =
                hub.post(&path)?.refusal(400).map_err(|error| format!("{path}: {error}"))?;
            assert_eq!(refusal.code, "hub_invalid_sha256", "{path}");
        }
    }

    // `{slug}`: a slug.
    for slug in ["..", "%2E%2E", "Data%20Explorer", "data-explorer.exe", "%2Fbin%2Fsh"] {
        let routes = [
            format!("/api/dashboards/{slug}/update-preview"),
            format!("/api/dashboards/{slug}/update"),
            format!("/api/dashboards/{slug}/update/{hash}"),
            format!("/api/dashboards/{slug}/rollback"),
            format!("/api/sources/lab/suite/dashboards/{slug}/preview"),
            format!("/api/sources/lab/suite/dashboards/{slug}/accept-key/{hash}"),
            format!("/api/sources/lab/suite/dashboards/{slug}/install/{hash}"),
        ];
        for path in routes {
            let refusal =
                hub.post(&path)?.refusal(400).map_err(|error| format!("{path}: {error}"))?;
            assert_eq!(refusal.code, "hub_invalid_slug", "{path}");
        }
    }

    // `on` or `off`, in lower case.
    for switch in ["yes", "ON", "Off", "1", "true", "on%20"] {
        let path = format!("/api/settings/check-at-start/{switch}");
        let refusal = hub.post(&path)?.refusal(400).map_err(|error| format!("{path}: {error}"))?;
        assert_eq!(refusal.code, "hub_invalid_switch", "{path}");
    }
    // A percent-encoded letter decodes to the letter: still only `on`.
    let settings: HubSettings = ok(&hub.post("/api/settings/check-at-start/%6Fff")?)?;
    assert!(!settings.check_updates_at_start);

    // A path in place of a segment matches no route.
    for path in [
        "/api/sources/lab/data-explorer/x/preview",
        "/api/sources/lab/preview",
        "/api/catalogs/lab/catalog/x/add",
        "/api/sources/lab/data-explorer/install/a/b",
        "/api/sources/lab/suite/dashboards/preview",
        "/api/sources/lab/suite/dashboards/a/b/preview",
        "/api/sources/lab/suite/signal-lab/preview",
    ] {
        assert_eq!(hub.post(path)?.refusal(404)?.code, "unknown_endpoint", "{path}");
    }

    // A query or a body is never read: the segments alone decide.
    let path = format!(
        "/api/sources/lab/data-explorer/install/{hash}?repo=evil/x&sha256=0&path=C%3A%5Cx.exe"
    );
    assert_eq!(hub.post(&path)?.refusal(404)?.code, "hub_no_preview");
    let body = r#"{"repo":"evil/x","url":"https://evil.example/x.exe","path":"C:\\x.exe","args":["--evil"]}"#;
    let path = format!("/api/sources/lab/data-explorer/install/{hash}");
    let answer = request_with_body(&hub.address, "POST", &path, Some(&hub.token), body)?;
    assert_eq!(answer.refusal(404)?.code, "hub_no_preview");
    let path = "/api/settings/check-at-start/on?switch=off";
    let settings: HubSettings = ok(&hub.post(path)?)?;
    assert!(settings.check_updates_at_start);

    // Nothing above reached GitHub, the registry or the catalogs.
    assert!(fake.requests().is_empty(), "requests to GitHub: {:?}", fake.requests());
    assert!(!HubDir::at(root.path()).catalogs_path().exists());
    assert!(HubDir::at(root.path()).read_registry()?.dashboards.is_empty());

    // Every new route sits behind the session token and the Host and Origin
    // checks.
    let routes = [
        ("GET", "/api/catalogs".to_owned()),
        ("POST", "/api/catalogs/lab/catalog/add".to_owned()),
        ("POST", "/api/catalogs/lab/catalog/remove".to_owned()),
        ("POST", "/api/sources/lab/data-explorer/preview".to_owned()),
        ("POST", format!("/api/sources/lab/data-explorer/accept-key/{hash}")),
        ("POST", format!("/api/sources/lab/data-explorer/install/{hash}")),
        ("POST", "/api/sources/lab/suite/dashboards/signal-lab/preview".to_owned()),
        ("POST", format!("/api/sources/lab/suite/dashboards/signal-lab/accept-key/{hash}")),
        ("POST", format!("/api/sources/lab/suite/dashboards/signal-lab/install/{hash}")),
        ("POST", "/api/check".to_owned()),
        ("POST", "/api/dashboards/data-explorer/update-preview".to_owned()),
        ("POST", format!("/api/dashboards/data-explorer/update/{hash}")),
        ("POST", "/api/dashboards/data-explorer/update".to_owned()),
        ("POST", "/api/dashboards/data-explorer/rollback".to_owned()),
        ("GET", "/api/settings".to_owned()),
        ("POST", "/api/settings/check-at-start/off".to_owned()),
        ("POST", "/api/self-update".to_owned()),
    ];
    for (method, path) in &routes {
        let answer = request(&hub.address, method, path, None)?;
        assert_eq!(answer.refusal(401)?.code, "missing_token", "{method} {path}");
        let answer = request(&hub.address, method, path, Some("wrong"))?;
        assert_eq!(answer.refusal(401)?.code, "missing_token", "{method} {path}");
        for origin in ["https://evil.example", "http://localhost:1", "null"] {
            let answer = request_from_origin(&hub.address, method, path, &hub.token, origin)?;
            let refusal = answer.refusal(403).map_err(|error| format!("{path}: {error}"))?;
            assert_eq!(refusal.code, "forbidden_origin", "{method} {path} from {origin}");
        }
        // A foreign Host header (DNS rebinding) is refused too.
        for host in ["evil.example", "evil.example:80", "127.0.0.2:1"] {
            let answer = request_with_host(&hub.address, method, path, &hub.token, host)?;
            let refusal = answer.refusal(403).map_err(|error| format!("{path}: {error}"))?;
            assert_eq!(refusal.code, "forbidden_host", "{method} {path} with Host {host}");
        }
    }
    assert!(fake.requests().is_empty(), "requests to GitHub: {:?}", fake.requests());
    let settings: HubSettings = ok(&hub.get("/api/settings")?)?;
    assert!(settings.check_updates_at_start, "no refused request changed the setting");

    // A dashboard may be called `install` or `accept-key`: `dashboards`
    // keeps its slug apart from those routes, so this is a preview of it.
    let answer = hub.post("/api/sources/lab/suite/dashboards/install/preview")?;
    assert_eq!(answer.refusal(502)?.code, "github_no_release");
    let asked = "/repos/lab/suite/releases/latest".to_owned();
    assert!(fake.requests().contains(&asked), "requests to GitHub: {:?}", fake.requests());
    Ok(())
}

#[test]
fn an_install_over_http_needs_the_preview_its_hash_and_an_accepted_key() -> TestResult {
    let Some(target) = Target::CURRENT else { return Ok(()) };
    let root = tempfile::tempdir()?;
    let fake = FakeGitHub::start()?;
    let key = test_key(20)?;
    let asset = asset_of(DATA_EXPLORER, target)?;
    let executable = data_explorer_build("")?;
    // No catalog lists the repository: its minisign.pub is trusted on first
    // use.
    publish(&fake, REPO, TAG, &key, vec![FakeAsset::new(&asset, executable.clone())], true);
    let hub = remote_test_hub(root.path(), &fake)?;
    let install = |sha256: &str| hub.post(&format!("/api/sources/{REPO}/install/{sha256}"));

    // No preview: nothing to install, whatever the hash.
    assert_eq!(install(&"a".repeat(64))?.refusal(404)?.code, "hub_no_preview");
    assert_eq!(asset_downloads(&fake, &asset), 0);

    let preview: InstallPreview = ok(&hub.post(&format!("/api/sources/{REPO}/preview"))?)?;
    assert_eq!(preview.key_status, KeyStatus::Unknown);
    assert_eq!(preview.manifest, None, "not run before its key is trusted");
    assert_eq!(preview.key_sha256, key_sha256(&key)?);
    assert_eq!(preview.public_key, key.public_key());

    // Another hash, or another repository, is not this preview.
    assert_eq!(install(&"0".repeat(64))?.refusal(409)?.code, "hub_preview_mismatch");
    let other = hub.post(&format!("/api/sources/lab/other/install/{}", preview.sha256))?;
    assert_eq!(other.refusal(404)?.code, "hub_no_preview");
    // The right hash, but the key is not accepted.
    let refusal = install(&preview.sha256)?.refusal(409)?;
    assert_eq!(refusal.code, "hub_key_not_accepted");
    assert!(refusal.message.contains(&preview.key_sha256), "{}", refusal.message);
    assert_nothing_in_bin(root.path())?;

    // Only the key's SHA-256 accepts it.
    let accept = |sha256: &str| hub.post(&format!("/api/sources/{REPO}/accept-key/{sha256}"));
    assert_eq!(accept(&"ab".repeat(32))?.refusal(409)?.code, "hub_key_mismatch");
    assert_eq!(accept(&preview.sha256)?.refusal(409)?.code, "hub_key_mismatch");
    let accepted: InstallPreview = ok(&accept(&preview.key_sha256)?)?;
    assert_eq!(accepted.key_status, KeyStatus::Accepted);
    assert_eq!(accepted.sha256, preview.sha256);
    let manifest = accepted.manifest.ok_or("the full preview once the key is accepted")?;
    assert_eq!((manifest.slug.as_str(), manifest.name.as_str()), (DATA_EXPLORER, "Data Explorer"));
    assert_nothing_in_bin(root.path())?;

    let info = TestHub::info(&install(&preview.sha256)?)?;
    let installed = info
        .dashboards
        .iter()
        .find(|dashboard| dashboard.slug == DATA_EXPLORER)
        .ok_or("installed and listed")?;
    assert_eq!(installed.source, Source::Github { repo: REPO.to_owned() });
    assert_eq!(installed.state, DashboardState::Stopped);
    let entry = entry_of(root.path(), DATA_EXPLORER)?;
    assert_eq!(entry.pinned_key, Some(key.public_key()), "the accepted key is pinned");
    assert_eq!(fs::read(&entry.path)?, executable);

    // The preview was used up, and the repository is installed now.
    assert_eq!(install(&preview.sha256)?.refusal(404)?.code, "hub_no_preview");
    let again = hub.post(&format!("/api/sources/{REPO}/preview"))?;
    assert_eq!(again.refusal(409)?.code, "hub_already_installed");
    assert_eq!(staged_files(root.path())?, 0);

    // It starts like a local install.
    assert_eq!(hub.action(DATA_EXPLORER, "start")?.status, 200);
    hub.wait_for(DATA_EXPLORER, DashboardState::Running)?;
    assert_eq!(hub.action(DATA_EXPLORER, "stop")?.status, 200);
    Ok(())
}

#[test]
fn a_dashboard_of_a_repository_that_publishes_several_installs_over_http_by_its_slug() -> TestResult
{
    let Some(target) = Target::CURRENT else { return Ok(()) };
    let root = tempfile::tempdir()?;
    let fake = FakeGitHub::start()?;
    let key = test_key(24)?;
    publish_suite(&fake, &key, "")?;
    let hub = remote_test_hub(root.path(), &fake)?;
    let route = |slug: &str, rest: &str| format!("/api/sources/{SUITE}/dashboards/{slug}/{rest}");

    let ambiguous = hub.post(&format!("/api/sources/{SUITE}/preview"))?.refusal(502)?;
    assert_eq!(ambiguous.code, "hub_ambiguous_asset");
    assert!(ambiguous.message.contains("data-explorer, signal-lab"), "{}", ambiguous.message);
    let missing = hub.post(&route("other", "preview"))?.refusal(502)?;
    assert_eq!(missing.code, "hub_no_asset");
    let none = hub.post(&route(SIGNAL_LAB, &format!("install/{}", "a".repeat(64))))?;
    assert_eq!(none.refusal(404)?.code, "hub_no_preview");

    let preview: InstallPreview = ok(&hub.post(&route(SIGNAL_LAB, "preview"))?)?;
    assert_eq!((preview.slug.as_str(), preview.key_status), (SIGNAL_LAB, KeyStatus::Unknown));
    assert_eq!(preview.asset, asset_of(SIGNAL_LAB, target)?);
    // Another dashboard of the repository has no preview yet.
    let other = hub.post(&route(DATA_EXPLORER, &format!("accept-key/{}", preview.key_sha256)))?;
    assert_eq!(other.refusal(404)?.code, "hub_no_preview");
    let accepted: InstallPreview =
        ok(&hub.post(&route(SIGNAL_LAB, &format!("accept-key/{}", preview.key_sha256)))?)?;
    assert_eq!(accepted.key_status, KeyStatus::Accepted);
    let manifest = accepted.manifest.ok_or("the full preview")?;
    assert_eq!(manifest.slug, SIGNAL_LAB);
    let info =
        TestHub::info(&hub.post(&route(SIGNAL_LAB, &format!("install/{}", preview.sha256)))?)?;
    let listed: Vec<&str> =
        info.dashboards.iter().map(|dashboard| dashboard.slug.as_str()).collect();
    assert_eq!(listed, [SIGNAL_LAB]);

    // The second dashboard: the repository's pinned key.
    let preview: InstallPreview = ok(&hub.post(&route(DATA_EXPLORER, "preview"))?)?;
    assert_eq!(preview.key_status, KeyStatus::Pinned);
    let info =
        TestHub::info(&hub.post(&route(DATA_EXPLORER, &format!("install/{}", preview.sha256)))?)?;
    for slug in [SIGNAL_LAB, DATA_EXPLORER] {
        let dashboard = info.dashboards.iter().find(|dashboard| dashboard.slug == slug);
        let source = dashboard.map(|dashboard| dashboard.source.clone());
        assert_eq!(source, Some(Source::Github { repo: SUITE.to_owned() }), "{slug}");
    }
    let again = hub.post(&route(DATA_EXPLORER, "preview"))?.refusal(409)?;
    assert_eq!(again.code, "hub_already_installed");
    Ok(())
}

/// No dashboard has a directory in `bin/`.
fn assert_nothing_in_bin(root: &Path) -> TestResult {
    let bin = root.join("bin");
    let count = match fs::read_dir(&bin) {
        Ok(entries) => entries.count(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(error) => return Err(error.into()),
    };
    assert_eq!(count, 0, "something was written to {}", bin.display());
    Ok(())
}

#[test]
fn updates_checks_and_rollbacks_run_over_http() -> TestResult {
    let Some(target) = Target::CURRENT else { return Ok(()) };
    let root = tempfile::tempdir()?;
    let fake = FakeGitHub::start()?;
    let (key, new_key) = (test_key(21)?, test_key(22)?);
    let asset = asset_of(DATA_EXPLORER, target)?;
    let release = |key: &TestKey, marker: &str| -> Result<Vec<u8>, Failure> {
        let build = data_explorer_build(marker)?;
        publish(&fake, REPO, TAG, key, vec![FakeAsset::new(&asset, build.clone())], true);
        Ok(build)
    };
    release(&key, "build 1")?;
    serve_catalog(&fake, eario_hub::remote::DEFAULT_CATALOG, &[(REPO, key.public_key())]);
    let hub = remote_test_hub(root.path(), &fake)?;
    let dashboard = |info: &HubInfo| -> Result<HubDashboard, Failure> {
        let found = info.dashboards.iter().find(|dashboard| dashboard.slug == DATA_EXPLORER);
        found.cloned().ok_or_else(|| "data-explorer is not listed".into())
    };

    // Installed from a catalog key: trusted at once.
    let preview: InstallPreview = ok(&hub.post(&format!("/api/sources/{REPO}/preview"))?)?;
    assert_eq!(preview.key_status, KeyStatus::Catalog);
    TestHub::info(&hub.post(&format!("/api/sources/{REPO}/install/{}", preview.sha256))?)?;
    set_installed_version(root.path(), DATA_EXPLORER, EARLIER_1)?;
    let first = entry_of(root.path(), DATA_EXPLORER)?;
    assert_eq!(hub.action(DATA_EXPLORER, "rollback")?.refusal(409)?.code, "hub_no_previous");
    let missing = hub.post("/api/dashboards/nope/update")?;
    assert_eq!(missing.refusal(404)?.code, "hub_not_installed");

    // Check now: an update is available.
    let second_build = release(&key, "build 2")?;
    let info = TestHub::info(&hub.post("/api/check")?)?;
    let check = info.last_check.clone().ok_or("the check just made")?;
    assert_eq!(check.dashboards.len(), 1);
    assert_eq!(check.dashboards[0].available_version.as_deref(), Some(EXAMPLE_VERSION));
    assert_eq!(dashboard(&info)?.available_version.as_deref(), Some(EXAMPLE_VERSION));

    // Previewed, then installed by its hash.
    let preview: InstallPreview = ok(&hub.action(DATA_EXPLORER, "update-preview")?)?;
    assert_eq!(preview.key_status, KeyStatus::Pinned);
    let wrong = hub.action(DATA_EXPLORER, &format!("update/{}", "0".repeat(64)))?;
    assert_eq!(wrong.refusal(409)?.code, "hub_preview_mismatch");
    let info = TestHub::info(&hub.action(DATA_EXPLORER, &format!("update/{}", preview.sha256))?)?;
    assert!(dashboard(&info)?.has_previous);
    assert_eq!(dashboard(&info)?.available_version, None);
    let second = entry_of(root.path(), DATA_EXPLORER)?;
    assert_eq!(fs::read(&second.path)?, second_build);
    assert_eq!(second.sha256, preview.sha256);
    assert_eq!(
        second.previous.as_ref().map(|previous| previous.sha256.as_str()),
        Some(first.sha256.as_str())
    );

    // Not newer: refused.
    let refusal = hub.action(DATA_EXPLORER, "update")?.refusal(409)?;
    assert_eq!(refusal.code, "hub_not_newer");

    // Refused while it runs.
    set_installed_version(root.path(), DATA_EXPLORER, EARLIER_2)?;
    let third_build = release(&key, "build 3")?;
    assert_eq!(hub.action(DATA_EXPLORER, "start")?.status, 200);
    hub.wait_for(DATA_EXPLORER, DashboardState::Running)?;
    assert_eq!(hub.action(DATA_EXPLORER, "update")?.refusal(409)?.code, "hub_running");
    assert_eq!(hub.action(DATA_EXPLORER, "rollback")?.refusal(409)?.code, "hub_running");
    assert_eq!(hub.action(DATA_EXPLORER, "stop")?.status, 200);

    // In one step, with the pinned key.
    let info = TestHub::info(&hub.action(DATA_EXPLORER, "update")?)?;
    assert!(dashboard(&info)?.has_previous);
    let third = entry_of(root.path(), DATA_EXPLORER)?;
    assert_eq!(fs::read(&third.path)?, third_build);

    // Rolled back once: the previous version, and no older one.
    let info = TestHub::info(&hub.action(DATA_EXPLORER, "rollback")?)?;
    assert!(!dashboard(&info)?.has_previous);
    let restored = entry_of(root.path(), DATA_EXPLORER)?;
    assert_eq!(
        (restored.path.as_path(), restored.sha256.as_str()),
        (second.path.as_path(), second.sha256.as_str())
    );
    assert!(!third.path.exists(), "the newer copy is removed");
    assert_eq!(hub.action(DATA_EXPLORER, "rollback")?.refusal(409)?.code, "hub_no_previous");

    // A new key: refused in one step, then accepted by its SHA-256 and
    // installed by the preview's hash.
    let fourth_build = release(&new_key, "build 4")?;
    let refusal = hub.action(DATA_EXPLORER, "update")?.refusal(409)?;
    assert_eq!(refusal.code, "hub_key_changed");
    let changed: InstallPreview = ok(&hub.action(DATA_EXPLORER, "update-preview")?)?;
    assert_eq!(changed.key_status, KeyStatus::Changed);
    assert_eq!(changed.manifest, None, "not run before the new key is accepted");
    let path = format!("/api/dashboards/{DATA_EXPLORER}/update/{}", changed.sha256);
    assert_eq!(hub.post(&path)?.refusal(409)?.code, "hub_key_changed");
    assert_eq!(entry_of(root.path(), DATA_EXPLORER)?.pinned_key, Some(key.public_key()));
    let accepted: InstallPreview =
        ok(&hub.post(&format!("/api/sources/{REPO}/accept-key/{}", key_sha256(&new_key)?))?)?;
    assert_eq!(accepted.key_status, KeyStatus::Accepted);
    assert!(accepted.manifest.is_some());
    TestHub::info(&hub.post(&path)?)?;
    let fourth = entry_of(root.path(), DATA_EXPLORER)?;
    assert_eq!(fs::read(&fourth.path)?, fourth_build);
    assert_eq!(fourth.pinned_key, Some(new_key.public_key()), "the new key is pinned");
    assert_eq!(staged_files(root.path())?, 0);
    Ok(())
}

#[test]
fn catalogs_and_the_check_at_start_setting_change_over_http() -> TestResult {
    let root = tempfile::tempdir()?;
    let fake = FakeGitHub::start()?;
    let key = test_key(23)?;
    serve_catalog(&fake, "lab/catalog", &[(REPO, key.public_key())]);
    let hub = remote_test_hub(root.path(), &fake)?;
    let default = eario_hub::remote::DEFAULT_CATALOG;

    // The default catalog, not served here: listed with why.
    let listing: Vec<HubCatalog> = ok(&hub.get("/api/catalogs")?)?;
    assert_eq!(listing.len(), 1);
    assert_eq!(listing[0].repo, default);
    assert_eq!(
        listing[0].error.as_ref().map(|error| error.code.as_str()),
        Some("github_not_found")
    );

    // A repository without a catalog is not added: GitHub said no.
    let refusal = hub.post("/api/catalogs/lab/nothing/add")?.refusal(502)?;
    assert_eq!(refusal.code, "github_not_found");
    let listing: Vec<HubCatalog> = ok(&hub.post("/api/catalogs/lab/catalog/add")?)?;
    let repos: Vec<&str> = listing.iter().map(|catalog| catalog.repo.as_str()).collect();
    assert_eq!(repos, [default, "lab/catalog"]);
    assert_eq!(listing[1].name.as_deref(), Some("Lab"));
    assert_eq!(listing[1].dashboards[0].repo, REPO);
    assert_eq!(listing[1].dashboards[0].installed_slug, None);

    let listing: Vec<HubCatalog> = ok(&hub.post(&format!("/api/catalogs/{default}/remove"))?)?;
    let repos: Vec<&str> = listing.iter().map(|catalog| catalog.repo.as_str()).collect();
    assert_eq!(repos, ["lab/catalog"]);
    let again = hub.post(&format!("/api/catalogs/{default}/remove"))?;
    assert_eq!(again.refusal(404)?.code, "hub_unknown_catalog");
    let remote = RemoteHub::new(root.path(), &fake)?;
    assert_eq!(remote.run(remote.supervisor.catalogs())?, ["lab/catalog"], "stored");

    // The setting, stored in the hub directory.
    let settings: HubSettings = ok(&hub.get("/api/settings")?)?;
    assert!(settings.check_updates_at_start, "on by default");
    let settings: HubSettings = ok(&hub.post("/api/settings/check-at-start/off")?)?;
    assert!(!settings.check_updates_at_start);
    assert!(!ok::<HubSettings>(&hub.get("/api/settings")?)?.check_updates_at_start);
    assert!(!remote.run(remote.supervisor.settings())?.check_updates_at_start, "stored");
    let settings: HubSettings = ok(&hub.post("/api/settings/check-at-start/on")?)?;
    assert!(settings.check_updates_at_start);

    // A check with nothing installed from GitHub reads no dashboard's
    // repository; it checks the hub itself, or says why it does not.
    let dashboard_requests =
        |fake: &FakeGitHub| fake.requests().iter().filter(|path| !path.contains(HUB_REPO)).count();
    let before = dashboard_requests(&fake);
    let info = TestHub::info(&hub.post("/api/check")?)?;
    let check = info.last_check.ok_or("checked")?;
    assert_eq!(check.dashboards.len(), 0);
    assert_eq!(check.hub.map(|hub| hub.repo).as_deref(), Some(HUB_REPO));
    assert_eq!(dashboard_requests(&fake), before);
    Ok(())
}

/// Runs the `eario-hub` command line on `root`, against the fake GitHub
/// at `api`. Returns its exit status, standard output and standard error.
fn hub_cli(
    root: &Path,
    api: &str,
    arguments: &[&str],
) -> Result<(ExitStatus, String, String), Failure> {
    let output = hub_command(root).env("EARIO_HUB_GITHUB_API", api).args(arguments).output()?;
    let printed = String::from_utf8_lossy(&output.stdout).into_owned();
    let errors = String::from_utf8_lossy(&output.stderr).into_owned();
    Ok((output.status, printed, errors))
}

#[test]
fn the_command_line_installs_updates_and_rolls_back() -> TestResult {
    let Some(target) = Target::CURRENT else { return Ok(()) };
    // Release builds ignore the fake GitHub's address and would reach GitHub.
    if !cfg!(debug_assertions) {
        return Ok(());
    }
    let root = tempfile::tempdir()?;
    let fake = FakeGitHub::start()?;
    let api = fake.base_url();
    let key = test_key(15)?;
    let asset = asset_of(DATA_EXPLORER, target)?;
    publish(
        &fake,
        REPO,
        TAG,
        &key,
        vec![FakeAsset::new(&asset, data_explorer_build("build 1")?)],
        true,
    );
    serve_catalog(&fake, "lab/catalog", &[(REPO, key.public_key())]);
    // A hub runs meanwhile: it sees every change at its next request.
    let supervisor = Supervisor::with_github(
        HubDir::at(root.path()),
        SupervisorOptions::default(),
        fake_client(&fake)?,
    )?;
    let hub = TestHub::with_supervisor(supervisor)?;

    let (status, _, errors) = hub_cli(root.path(), &api, &["frobnicate"])?;
    assert_eq!(status.code(), Some(2), "{errors}");
    assert!(errors.contains("Usage: eario-hub"), "{errors}");

    // No terminal: a confirmation cannot be asked.
    let (status, _, errors) = hub_cli(root.path(), &api, &["catalog", "add", "lab/catalog"])?;
    assert!(!status.success());
    assert!(errors.contains("hub_confirmation_required"), "{errors}");
    let (status, printed, errors) =
        hub_cli(root.path(), &api, &["catalog", "add", "lab/catalog", "--yes"])?;
    assert!(status.success(), "{errors}");
    assert!(printed.contains("lab/catalog"), "{printed}");
    let (status, printed, errors) = hub_cli(root.path(), &api, &["catalog", "list"])?;
    assert!(status.success(), "{errors}");
    assert!(printed.contains(REPO) && printed.contains("could not be read"), "{printed}");

    // The catalog vouches for the key; the install still needs a yes.
    let (status, printed, errors) = hub_cli(root.path(), &api, &["install", REPO])?;
    assert!(!status.success());
    assert!(errors.contains("hub_confirmation_required"), "{errors}");
    assert!(printed.contains("Key SHA-256:") && printed.contains(&key.public_key()), "{printed}");
    assert!(printed.contains("catalog"), "{printed}");
    let (status, printed, errors) = hub_cli(root.path(), &api, &["install", REPO, "--yes"])?;
    assert!(status.success(), "{errors}");
    assert!(printed.contains("Installed Data Explorer"), "{printed}");
    assert_eq!(hub.dashboard(DATA_EXPLORER)?.source, Source::Github { repo: REPO.to_owned() });

    let (status, printed, errors) = hub_cli(root.path(), &api, &["check"])?;
    assert!(status.success(), "{errors}");
    assert!(printed.contains("up to date"), "{printed}");
    set_installed_version(root.path(), DATA_EXPLORER, EARLIER_1)?;
    let newer = data_explorer_build("build 2")?;
    publish(&fake, REPO, TAG, &key, vec![FakeAsset::new(&asset, newer.clone())], true);
    let (status, printed, errors) = hub_cli(root.path(), &api, &["check"])?;
    assert!(status.success(), "{errors}");
    assert!(printed.contains(&format!("version {EXAMPLE_VERSION} available")), "{printed}");
    assert_eq!(hub.dashboard(DATA_EXPLORER)?.available_version.as_deref(), Some(EXAMPLE_VERSION));

    // Not while it runs from the hub.
    assert_eq!(hub.action(DATA_EXPLORER, "start")?.status, 200);
    hub.wait_for(DATA_EXPLORER, DashboardState::Running)?;
    let (status, _, errors) = hub_cli(root.path(), &api, &["update", DATA_EXPLORER, "--yes"])?;
    assert!(!status.success());
    assert!(errors.contains("hub_running"), "{errors}");
    assert_eq!(hub.action(DATA_EXPLORER, "stop")?.status, 200);

    let (status, printed, errors) =
        hub_cli(root.path(), &api, &["update", DATA_EXPLORER, "--yes"])?;
    assert!(status.success(), "{errors}");
    assert!(printed.contains("Updated Data Explorer"), "{printed}");
    let updated =
        HubDir::at(root.path()).read_registry()?.get(DATA_EXPLORER).cloned().ok_or("entry")?;
    assert_eq!(fs::read(&updated.path)?, newer);
    assert!(hub.dashboard(DATA_EXPLORER)?.has_previous);

    let (status, printed, errors) =
        hub_cli(root.path(), &api, &["rollback", DATA_EXPLORER, "--yes"])?;
    assert!(status.success(), "{errors}");
    assert!(printed.contains("Restored Data Explorer"), "{printed}");
    assert!(!hub.dashboard(DATA_EXPLORER)?.has_previous);
    assert!(!updated.path.exists());

    let (status, printed, _) = hub_cli(root.path(), &api, &["check-at-start", "off"])?;
    assert!(status.success());
    assert!(printed.contains("does not check"), "{printed}");
    let (status, printed, errors) =
        hub_cli(root.path(), &api, &["catalog", "remove", "lab/catalog", "--yes"])?;
    assert!(status.success(), "{errors}");
    assert!(printed.contains("Catalogs: rtonneau/eario-catalog"), "{printed}");
    Ok(())
}

#[test]
fn the_command_line_trusts_a_new_key_only_when_told_its_sha256() -> TestResult {
    let Some(target) = Target::CURRENT else { return Ok(()) };
    if !cfg!(debug_assertions) {
        return Ok(());
    }
    let root = tempfile::tempdir()?;
    let fake = FakeGitHub::start()?;
    let api = fake.base_url();
    let key = test_key(16)?;
    let asset = asset_of(DATA_EXPLORER, target)?;
    publish(&fake, REPO, TAG, &key, vec![FakeAsset::new(&asset, data_explorer_build("")?)], true);
    let key_sha256 = verify::PublicKey::from_base64(REPO, &key.public_key())?.sha256();

    // --yes does not trust a key, and the executable is not run.
    let (status, printed, errors) = hub_cli(root.path(), &api, &["install", REPO, "--yes"])?;
    assert!(!status.success());
    assert!(printed.contains("UNKNOWN") && printed.contains(&key_sha256), "{printed}");
    assert!(printed.contains("read from the executable once its key is trusted"), "{printed}");
    assert!(!printed.contains("Data Explorer"), "no manifest was read: {printed}");
    assert!(errors.contains("hub_key_not_accepted") && errors.contains("--accept-key"), "{errors}");
    let wrong = "ab".repeat(32);
    let (status, _, errors) =
        hub_cli(root.path(), &api, &["install", REPO, "--yes", "--accept-key", &wrong])?;
    assert!(!status.success());
    assert!(errors.contains("hub_key_mismatch"), "{errors}");
    assert_nothing_installed(root.path(), DATA_EXPLORER)?;

    let arguments = ["install", REPO, "--yes", "--accept-key", &key_sha256];
    let (status, printed, errors) = hub_cli(root.path(), &api, &arguments)?;
    assert!(status.success(), "{errors}");
    // The key first, then the full preview, read once the key is accepted.
    let accepted = printed.find("Key accepted.").ok_or_else(|| printed.clone())?;
    let full = printed.find(&format!("Data Explorer (`data-explorer`), version {EXAMPLE_VERSION}"));
    assert!(full.is_some_and(|full| full > accepted), "{printed}");
    assert!(printed.contains("Installed Data Explorer"), "{printed}");
    let entry =
        HubDir::at(root.path()).read_registry()?.get(DATA_EXPLORER).cloned().ok_or("entry")?;
    assert_eq!(entry.pinned_key, Some(key.public_key()));
    Ok(())
}

// The hub's own update (ADR 0013). A fake GitHub serves releases of
// `rtonneau/eario` signed with a test key, which stands in for the Eario
// release key compiled into the hub. The new hub is this build of
// `eario-hub` with a few bytes appended (it still runs), so its `--version`
// prints this build's version, and the hub under test pretends to run an
// older one. The executable replaced is a copy in a temporary directory,
// never this test's own.

/// The version of this build of the hub, which its `--version` prints.
const THIS_HUB: &str = env!("CARGO_PKG_VERSION");
/// The version the hub under test pretends to run, older than [`THIS_HUB`].
const OLDER_HUB: &str = "0.0.1";

/// A copy of this build of the hub, at `<directory>/eario-hub[.exe]`.
fn hub_copy(directory: &Path) -> Result<PathBuf, Failure> {
    let path = directory.join(format!("{HUB_SLUG}{}", std::env::consts::EXE_SUFFIX));
    fs::copy(HUB_EXECUTABLE, &path)?;
    Ok(path)
}

/// This build of the hub with `marker` appended: a new version that runs.
fn hub_build(marker: &str) -> Result<Vec<u8>, Failure> {
    let mut bytes = fs::read(HUB_EXECUTABLE)?;
    bytes.extend_from_slice(marker.as_bytes());
    Ok(bytes)
}

/// `path` with `suffix` added to its file name.
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(suffix);
    path.with_file_name(name)
}

/// The name of the file at `path`.
fn file_name(path: &Path) -> String {
    path.file_name().unwrap_or_default().to_string_lossy().into_owned()
}

/// The names of the files in `directory`, sorted.
fn file_names(directory: &Path) -> Result<Vec<String>, Failure> {
    let mut names = Vec::new();
    for entry in fs::read_dir(directory)? {
        names.push(entry?.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    Ok(names)
}

/// A supervisor of `root` whose self-update trusts `key` (`None`: the hub
/// holds no release key), replaces `executable` and runs `version`.
fn self_updating(
    root: &Path,
    fake: &FakeGitHub,
    key: Option<&TestKey>,
    executable: &Path,
    version: &str,
) -> Result<Supervisor, Failure> {
    let key = match key {
        Some(key) => Some(verify::PublicKey::from_base64(HUB_REPO, &key.public_key())?),
        None => None,
    };
    let config = SelfUpdateConfig::for_tests(key, executable.to_owned(), version);
    let options = SupervisorOptions::default();
    Ok(Supervisor::with_self_update(HubDir::at(root), options, fake_client(fake)?, config)?)
}

/// Serves `content` as the hub's asset for `target` in the latest release
/// `tag` of the hub, its checksum list signed by `key`, without
/// `minisign.pub`.
fn publish_hub(
    fake: &FakeGitHub,
    target: Target,
    tag: &str,
    key: &TestKey,
    content: Vec<u8>,
) -> TestResult {
    let asset = FakeAsset::new(&asset_of(HUB_SLUG, target)?, content);
    publish(fake, HUB_REPO, tag, key, vec![asset], false);
    Ok(())
}

/// Starts `executable`, a hub, on `root`, against the fake GitHub at `api`,
/// and waits until it answers at the address it records in `hub.json`.
fn hub_process(
    executable: &Path,
    root: &Path,
    api: &str,
) -> Result<(HubProcess, RunFile), Failure> {
    let mut command = Command::new(executable);
    command
        .env("EARIO_HUB_DIR", root)
        .env("EARIO_NO_BROWSER", "1")
        .env("EARIO_HUB_GITHUB_API", api)
        .env_remove("EARIO_PORT")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut process = HubProcess(command.spawn()?);
    let hub_file = HubDir::at(root).hub_file_path();
    let started = Instant::now();
    loop {
        if let Some(recorded) = hub_dir::read_run_file_at(&hub_file)?
            && hub_dir::probe_blocking(&recorded.address, &recorded.token)?
        {
            return Ok((process, recorded));
        }
        if let Some(status) = process.0.try_wait()? {
            return Err(format!("the hub exited with {status}").into());
        }
        if started.elapsed() > TIMEOUT {
            return Err("the hub never recorded itself in hub.json".into());
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn self_update_replaces_the_running_hub_and_restarts_it() -> TestResult {
    let Some(target) = Target::CURRENT else { return Ok(()) };
    let root = tempfile::tempdir()?;
    let installed = tempfile::tempdir()?;
    let fake = FakeGitHub::start()?;
    let key = test_key(30)?;
    let executable = hub_copy(installed.path())?;
    let before = fs::read(&executable)?;
    let old = with_suffix(&executable, ".old");
    let name = file_name(&executable);

    // The executable to replace is running, as the hub of another directory:
    // Windows renames a running executable, but never deletes it.
    let elsewhere = tempfile::tempdir()?;
    let (mut old_hub, recorded) = hub_process(&executable, elsewhere.path(), &fake.base_url())?;

    // A dashboard runs too, which the self-update leaves alone.
    install(root.path(), DATA_EXPLORER)?;
    let new_build = hub_build("new hub")?;
    publish_hub(&fake, target, &format!("v{THIS_HUB}"), &key, new_build.clone())?;
    let supervisor = self_updating(root.path(), &fake, Some(&key), &executable, OLDER_HUB)?;
    let mut hub = TestHub::with_supervisor(supervisor)?;
    assert_eq!(hub.action(DATA_EXPLORER, "start")?.status, 200);
    hub.wait_for(DATA_EXPLORER, DashboardState::Running)?;
    let dashboard = run_file(root.path(), DATA_EXPLORER)?;

    // The check finds the newer hub by its tag, and downloads nothing.
    let info = TestHub::info(&hub.post("/api/check")?)?;
    assert_eq!(info.eario_version, OLDER_HUB);
    assert_eq!(info.available_version.as_deref(), Some(THIS_HUB));
    let found = info.last_check.and_then(|check| check.hub).ok_or("the hub checked itself")?;
    let expected = HubUpdate {
        repo: HUB_REPO.to_owned(),
        latest_version: Some(THIS_HUB.to_owned()),
        available_version: Some(THIS_HUB.to_owned()),
        error: None,
    };
    assert_eq!(found, expected);
    let asset = asset_of(HUB_SLUG, target)?;
    assert_eq!(asset_downloads(&fake, &asset), 0);

    let updated: HubSelfUpdate = ok(&hub.post("/api/self-update")?)?;
    let expected =
        HubSelfUpdate { previous_version: OLDER_HUB.to_owned(), version: THIS_HUB.to_owned() };
    assert_eq!(updated, expected);
    assert_eq!(asset_downloads(&fake, &asset), 1);

    // Having answered, the server stops by itself, and names the executable
    // to start.
    let server = hub.running.take().ok_or("the hub served")?;
    hub.runtime.block_on(async { tokio::time::timeout(TIMEOUT, server.wait()).await })??;
    assert_eq!(hub.supervisor.restart_executable(), Some(executable.clone()));
    assert_eq!(fs::read(&executable)?, new_build, "the new version is in place");
    assert_eq!(fs::read(&old)?, before, "the replaced executable is kept beside it");
    assert_eq!(file_names(installed.path())?, [name.clone(), format!("{name}.old")]);
    assert_eq!(staged_files(root.path())?, 0, "the download is gone from staging");
    let again = hub.runtime.block_on(hub.supervisor.self_update());
    assert_eq!(refusal(again), Some("self_update_pending"));

    // The dashboard runs on, and so does the hub running from the replaced
    // executable.
    assert!(hub_dir::probe_blocking(&dashboard.address, &dashboard.token)?);
    assert!(hub_dir::probe_blocking(&recorded.address, &recorded.token)?);

    // Once that hub has quit, the next start removes the replaced
    // executable, and re-attaches to the dashboard.
    let quit = request(&recorded.address, "POST", "/api/shutdown", Some(&recorded.token))?;
    assert_eq!(quit.status, 204);
    exit_status(&mut old_hub.0)?;
    let supervisor = self_updating(root.path(), &fake, Some(&key), &executable, THIS_HUB)?;
    let next = TestHub::with_supervisor(supervisor)?;
    let started = Instant::now();
    while old.exists() {
        if started.elapsed() > TIMEOUT {
            return Err("the replaced executable was not removed at the next start".into());
        }
        thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(file_names(installed.path())?, [name]);
    assert_eq!(fs::read(&executable)?, new_build);
    assert_eq!(next.dashboard(DATA_EXPLORER)?.state, DashboardState::Running);
    Ok(())
}

#[test]
fn self_update_refuses_what_it_cannot_verify_and_changes_nothing() -> TestResult {
    let Some(target) = Target::CURRENT else { return Ok(()) };
    let root = tempfile::tempdir()?;
    let installed = tempfile::tempdir()?;
    let fake = FakeGitHub::start()?;
    let key = test_key(31)?;
    let other = test_key(32)?;
    let executable = hub_copy(installed.path())?;
    let before = fs::read(&executable)?;
    let name = file_name(&executable);
    let asset = asset_of(HUB_SLUG, target)?;
    let new_build = hub_build("new hub")?;
    let tag = format!("v{THIS_HUB}");
    let unchanged = |case: &str| -> TestResult {
        assert_eq!(fs::read(&executable)?, before, "{case}: the hub was not replaced");
        assert_eq!(file_names(installed.path())?, [name.as_str()], "{case}: nothing beside it");
        assert_eq!(staged_files(root.path())?, 0, "{case}: nothing left in staging");
        Ok(())
    };

    // A hub without a release key: refused before any request, and its
    // check says why.
    publish_hub(&fake, target, &tag, &key, new_build.clone())?;
    let keyless = RemoteHub::with_supervisor(self_updating(
        root.path(),
        &fake,
        None,
        &executable,
        OLDER_HUB,
    )?)?;
    let not_configured = Some("self_update_not_configured");
    assert_eq!(refusal(keyless.run(keyless.supervisor.self_update())), not_configured);
    assert_eq!(refusal(keyless.run(keyless.supervisor.check_hub())), not_configured);
    let check = keyless.run(keyless.supervisor.check())?;
    let found = check.hub.ok_or("the hub is in the check")?;
    assert_eq!(found.error.map(|error| error.code).as_deref(), not_configured);
    assert_eq!(found.available_version, None);
    assert_eq!(keyless.run(keyless.supervisor.list())?.available_version, None);
    assert!(fake.requests().is_empty(), "requests to GitHub: {:?}", fake.requests());
    unchanged("no key")?;

    let hub = RemoteHub::with_supervisor(self_updating(
        root.path(),
        &fake,
        Some(&key),
        &executable,
        OLDER_HUB,
    )?)?;
    let self_update = || refusal(hub.run(hub.supervisor.self_update()));

    // A tag that is not newer: refused before the download.
    publish_hub(&fake, target, &format!("v{OLDER_HUB}"), &key, new_build.clone())?;
    assert_eq!(self_update(), Some("self_update_not_newer"));
    unchanged("an older tag")?;

    // No signature: refused before the download.
    let signed = [FakeAsset::new(&asset, new_build.clone())];
    let sums = FakeAsset::new(verify::SUMS_FILE, fake_github::sums_file(&signed));
    fake.release(HUB_REPO, &tag, &[signed[0].clone(), sums]);
    assert_eq!(self_update(), Some("release_missing_signature"));
    unchanged("no signature")?;

    // Signed with another key, which the release carries as its
    // `minisign.pub`: the hub trusts only the key compiled into it.
    publish(&fake, HUB_REPO, &tag, &other, signed.to_vec(), true);
    assert_eq!(self_update(), Some("release_wrong_key"));
    unchanged("another key")?;

    // A checksum list changed after it was signed.
    let mut files = key.release_files(&signed).to_vec();
    let changed = [FakeAsset::new(&asset, b"another build".to_vec())];
    files[1] = FakeAsset::new(verify::SUMS_FILE, fake_github::sums_file(&changed));
    files.push(signed[0].clone());
    fake.release(HUB_REPO, &tag, &files);
    assert_eq!(self_update(), Some("release_bad_signature"));
    unchanged("a changed checksum list")?;
    assert_eq!(asset_downloads(&fake, &asset), 0, "nothing downloaded so far");

    // An asset that is not the one signed.
    let mut files = key.release_files(&signed).to_vec();
    let mut tampered = new_build.clone();
    if let Some(last) = tampered.last_mut() {
        *last ^= 0xff;
    }
    files.push(FakeAsset::new(&asset, tampered));
    fake.release(HUB_REPO, &tag, &files);
    assert_eq!(self_update(), Some("release_hash_mismatch"));
    unchanged("a tampered asset")?;

    // A signed file that does not run.
    publish_hub(&fake, target, &tag, &key, b"not an executable".to_vec())?;
    assert_eq!(self_update(), Some("self_update_probe_failed"));
    unchanged("not an executable")?;

    // No asset for this machine.
    let elsewhere = FakeAsset::new("eario-hub-riscv64gc-unknown-linux-gnu", new_build.clone());
    publish(&fake, HUB_REPO, &tag, &key, vec![elsewhere], false);
    assert_eq!(self_update(), Some("hub_no_asset"));
    unchanged("no asset")?;

    // A newer tag on an executable that is not newer (an older signed
    // release served again): the version the executable prints decides.
    let current = self_updating(root.path(), &fake, Some(&key), &executable, THIS_HUB)?;
    let current = RemoteHub::with_supervisor(current)?;
    publish_hub(&fake, target, "v99.0.0", &key, new_build.clone())?;
    let replayed = current.run(current.supervisor.self_update());
    assert_eq!(refusal(replayed), Some("self_update_not_newer"));
    unchanged("an executable that is not newer")?;
    assert_eq!(current.supervisor.restart_executable(), None);

    // Over HTTP a refusal answers 409, and the hub serves on.
    let http =
        TestHub::with_supervisor(self_updating(root.path(), &fake, None, &executable, OLDER_HUB)?)?;
    let refused = http.post("/api/self-update")?.refusal(409)?;
    assert_eq!(refused.code, "self_update_not_configured");
    assert!(refused.message.contains("eario-release.pub"), "{}", refused.message);
    assert!(http.list()?.dashboards.is_empty(), "the hub still answers");
    assert_eq!(http.supervisor.restart_executable(), None);
    unchanged("over HTTP")?;
    Ok(())
}

#[test]
fn a_dropped_self_update_still_restarts_the_hub_and_runs_alone() -> TestResult {
    let Some(target) = Target::CURRENT else { return Ok(()) };
    let root = tempfile::tempdir()?;
    let installed = tempfile::tempdir()?;
    let fake = FakeGitHub::start()?;
    let key = test_key(33)?;
    let executable = hub_copy(installed.path())?;
    let before = fs::read(&executable)?;
    let new_build = hub_build("new hub")?;
    publish_hub(&fake, target, &format!("v{THIS_HUB}"), &key, new_build.clone())?;
    let hub = RemoteHub::with_supervisor(self_updating(
        root.path(),
        &fake,
        Some(&key),
        &executable,
        OLDER_HUB,
    )?)?;

    // The request is dropped once the update has started, as when its
    // client goes away or Quit cuts it short: its first poll starts the
    // work, then the future is gone.
    let pending = hub.run(async {
        let mut update = std::pin::pin!(hub.supervisor.self_update());
        std::future::poll_fn(|context| {
            std::task::Poll::Ready(update.as_mut().poll(context).is_pending())
        })
        .await
    });
    assert!(pending, "the update takes longer than one poll");

    // Meanwhile, a stopping hub waits for that update to end and finds the
    // restart it asked for by itself; a second update waits too, then finds
    // the hub restarting.
    let (restart, again) = hub.run(async {
        tokio::join!(hub.supervisor.restart_after_self_update(), hub.supervisor.self_update())
    });
    assert_eq!(restart, Some(executable.clone()));
    assert_eq!(refusal(again), Some("self_update_pending"));
    assert_eq!(hub.supervisor.restart_executable(), Some(executable.clone()));
    let asset = asset_of(HUB_SLUG, target)?;
    assert_eq!(asset_downloads(&fake, &asset), 1, "one update ran");
    assert_eq!(fs::read(&executable)?, new_build, "the new version is in place");
    assert_eq!(fs::read(with_suffix(&executable, ".old"))?, before);
    assert_eq!(staged_files(root.path())?, 0, "the download is gone from staging");
    Ok(())
}

#[test]
fn a_self_update_that_cannot_place_the_new_hub_leaves_it_as_it_was() -> TestResult {
    let Some(target) = Target::CURRENT else { return Ok(()) };
    let root = tempfile::tempdir()?;
    let installed = tempfile::tempdir()?;
    let fake = FakeGitHub::start()?;
    let key = test_key(34)?;
    let executable = hub_copy(installed.path())?;
    let before = fs::read(&executable)?;
    let old = with_suffix(&executable, ".old");
    let name = file_name(&executable);
    let asset = asset_of(HUB_SLUG, target)?;
    let new_build = hub_build("new hub")?;
    publish_hub(&fake, target, &format!("v{THIS_HUB}"), &key, new_build.clone())?;
    let hub = RemoteHub::with_supervisor(self_updating(
        root.path(),
        &fake,
        Some(&key),
        &executable,
        OLDER_HUB,
    )?)?;
    // Verified, run, copied beside the hub, then refused by the swap.
    let unchanged = |case: &str, result: Result<HubSelfUpdate, eario_hub::HubError>| {
        let error = result.err().ok_or_else(|| format!("{case}: not refused"))?;
        assert_eq!(error.code(), "self_update_failed", "{case}: {error}");
        assert!(error.to_string().contains("left as it was"), "{case}: {error}");
        assert_eq!(fs::read(&executable)?, before, "{case}: the hub was not replaced");
        assert_eq!(staged_files(root.path())?, 0, "{case}: nothing left in staging");
        assert_eq!(hub.supervisor.restart_executable(), None, "{case}: no restart");
        TestResult::Ok(())
    };

    // `<name>.old` is a folder with a file in it, which the swap cannot
    // remove to keep the running executable there.
    fs::create_dir(&old)?;
    fs::write(old.join("kept"), b"")?;
    unchanged("an .old that cannot be removed", hub.run(hub.supervisor.self_update()))?;
    assert_eq!(asset_downloads(&fake, &asset), 1, "downloaded and run before the swap");
    assert_eq!(file_names(installed.path())?, [name.clone(), format!("{name}.old")]);
    assert_eq!(file_names(&old)?, ["kept"], "the folder is left alone");
    fs::remove_dir_all(&old)?;

    // Windows: the executable is held without sharing its deletion, so it
    // cannot be renamed aside.
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let held = fs::OpenOptions::new().read(true).share_mode(0).open(&executable)?;
        let result = hub.run(hub.supervisor.self_update());
        drop(held);
        unchanged("a held executable", result)?;
        assert_eq!(asset_downloads(&fake, &asset), 2);
        assert_eq!(file_names(installed.path())?, [name.as_str()], "nothing beside it");
    }

    // A failed swap holds nothing back: once the obstacle is gone, the
    // update succeeds and asks for the restart.
    let updated = hub.run(hub.supervisor.self_update())?;
    assert_eq!(updated.version, THIS_HUB);
    assert_eq!(fs::read(&executable)?, new_build);
    assert_eq!(hub.supervisor.restart_executable(), Some(executable.clone()));
    Ok(())
}

#[test]
fn a_hub_without_the_release_key_offers_no_update_of_itself() -> TestResult {
    let Some(target) = Target::CURRENT else { return Ok(()) };
    let root = tempfile::tempdir()?;
    let installed = tempfile::tempdir()?;
    let fake = FakeGitHub::start()?;
    let key = test_key(35)?;
    let executable = hub_copy(installed.path())?;
    publish_hub(&fake, target, &format!("v{THIS_HUB}"), &key, hub_build("new hub")?)?;

    // A build with the key checks, and stores what it found in the hub
    // directory.
    let configured = RemoteHub::with_supervisor(self_updating(
        root.path(),
        &fake,
        Some(&key),
        &executable,
        OLDER_HUB,
    )?)?;
    configured.run(configured.supervisor.check())?;
    let info = configured.run(configured.supervisor.list())?;
    assert_eq!(info.available_version.as_deref(), Some(THIS_HUB));

    // A build without it, on the same directory, cannot verify that
    // release, so it offers none.
    let keyless = self_updating(root.path(), &fake, None, &executable, OLDER_HUB)?;
    let keyless = RemoteHub::with_supervisor(keyless)?;
    let info = keyless.run(keyless.supervisor.list())?;
    assert_eq!(info.available_version, None);
    let stored = info.last_check.and_then(|check| check.hub).and_then(|hub| hub.available_version);
    assert_eq!(stored.as_deref(), Some(THIS_HUB), "the other build's check is still stored");
    Ok(())
}

#[test]
fn self_update_from_the_command_line_waits_for_the_hub_to_quit() -> TestResult {
    // Release builds ignore the fake GitHub's address and would reach GitHub.
    if !cfg!(debug_assertions) {
        return Ok(());
    }
    let root = tempfile::tempdir()?;
    let fake = FakeGitHub::start()?;
    let api = fake.base_url();

    let (status, printed, errors) = hub_cli(root.path(), &api, &["--version"])?;
    assert!(status.success(), "{errors}");
    assert_eq!(printed.trim(), format!("{HUB_SLUG} {THIS_HUB}"));

    // While a hub runs, its executable is the one to replace: refused.
    let (mut running, recorded) = hub_process(Path::new(HUB_EXECUTABLE), root.path(), &api)?;
    let (status, _, errors) = hub_cli(root.path(), &api, &["self-update", "--yes"])?;
    assert_eq!(status.code(), Some(1), "{errors}");
    assert!(errors.contains("self_update_hub_running"), "{errors}");
    assert!(errors.contains(&recorded.address), "{errors}");
    let quit = request(&recorded.address, "POST", "/api/shutdown", Some(&recorded.token))?;
    assert_eq!(quit.status, 204);
    exit_status(&mut running.0)?;

    // Once it has quit: this build holds no release key, so the command
    // says so without a request (with a key, it reads the fake GitHub,
    // which has no release of the hub).
    let configured = SelfUpdateConfig::compiled().is_configured();
    let (status, _, errors) = hub_cli(root.path(), &api, &["self-update", "--yes"])?;
    assert_eq!(status.code(), Some(1), "{errors}");
    let expected = if configured { "github_no_release" } else { "self_update_not_configured" };
    assert!(errors.contains(expected), "{errors}");
    if !configured {
        let hub_requests = fake.requests().iter().filter(|path| path.contains(HUB_REPO)).count();
        assert_eq!(hub_requests, 0, "{:?}", fake.requests());
    }
    Ok(())
}
