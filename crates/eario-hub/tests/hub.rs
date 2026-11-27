//! The hub on the real example executables: each test installs them into a
//! temporary hub directory with `--eario-install --yes`, then drives a hub
//! through its HTTP API.
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
use std::thread;
use std::time::{Duration, Instant};

use eario_core::{
    ApiError, AppResponse, DashboardState, HubDashboard, HubInfo, HubOpen, LAUNCH_PROTOCOL_VERSION,
    RunFile,
};
use eario_hub::{RunningHub, Supervisor, SupervisorOptions};
use eario_runtime::ServerConfig;
use eario_runtime::hub_dir::{self, HubDir};

type TestResult = Result<(), Box<dyn std::error::Error>>;
type Failure = Box<dyn std::error::Error>;

const HUB_EXECUTABLE: &str = env!("CARGO_BIN_EXE_eario-hub");
const DATA_EXPLORER: &str = "data-explorer";
const SIGNAL_LAB: &str = "signal-lab";
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
        let runtime =
            tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()?;
        let supervisor = Supervisor::new(HubDir::at(root), options)?;
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
        assert_eq!(dashboard.version.as_deref(), Some("0.0.0"));
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
