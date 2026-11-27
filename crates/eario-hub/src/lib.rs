//! The Eario hub: one place to see, start, open and stop the dashboards a
//! user installed (ADR 0011), and to install and update them from GitHub
//! releases (ADR 0013).
//!
//! A dashboard enters the hub through `--eario-install`, run by the user from
//! its own executable, which copies it into the hub directory and records it
//! in the registry; or from the signed release of a GitHub repository, once
//! the user confirmed what the hub verified. The hub then starts that copy
//! with `--eario-child` when asked, by slug.
//!
//! - [`supervisor`]: the hub's services. A [`Supervisor`] lists, starts,
//!   opens, stops and uninstalls dashboards; reads catalogs; previews and
//!   installs releases; checks for updates, updates and rolls back. The HTTP
//!   API and the command line are thin adapters over it.
//! - [`remote`]: the blocking work of installing and updating from GitHub:
//!   staging, verification, the manifest of a downloaded executable, the
//!   one previous version kept, the version rule.
//! - [`self_update`]: the hub replaces its own executable with a newer
//!   release, verified with the Eario release key compiled into it, and
//!   restarts.
//! - [`start`] serves the hub on the loopback interface, behind the same
//!   Host/Origin check and session token as a dashboard, with the generic
//!   frontend, and checks for updates in the background.
//! - [`run`] is the `eario-hub` executable without arguments: one hub per
//!   user, recorded in `hub.json`, in a terminal that shows its address.
//!   [`cli`] is the executable with a command, such as `install` or
//!   `update`.
//! - [`verify`] checks a GitHub release before anything from it is installed
//!   (ADR 0013): the minisign signature of its checksum list, the key pinned
//!   for its repository, and the name of the asset for this machine.
//! - [`github`] reads releases and catalogs from GitHub over HTTPS, the
//!   hub's only network access; `fake_github` (feature `fake-github`)
//!   stands in for GitHub in tests.
//!
//! Quitting the hub leaves the dashboards running. The next hub re-attaches
//! to those whose run file answers with its token, and deletes the others.

pub mod cli;
#[cfg(feature = "fake-github")]
pub mod fake_github;
pub mod github;
mod http;
pub mod remote;
pub mod self_update;
pub mod supervisor;
pub mod verify;

use std::net::SocketAddr;
use std::time::SystemTime;

use eario_core::{LAUNCH_PROTOCOL_VERSION, RunFile};
use eario_runtime::hub_dir::{self, HubDir};
use eario_runtime::{HubDirError, RunningServer, ServerConfig, ShutdownHandle};

pub use supervisor::{HubError, START_TIMEOUT, STOP_TIMEOUT, Supervisor, SupervisorOptions};

/// A hub that is accepting connections.
pub struct RunningHub {
    server: RunningServer,
    supervisor: Supervisor,
}

impl RunningHub {
    /// Socket address the hub listens on, for example `127.0.0.1:49152`.
    pub fn address(&self) -> SocketAddr {
        self.server.address()
    }

    /// The session token API clients must present.
    pub fn token(&self) -> &str {
        self.server.token()
    }

    /// Address to open in a browser, with the session token in the fragment.
    pub fn url(&self) -> String {
        self.server.url()
    }

    /// The services behind the API.
    pub fn supervisor(&self) -> &Supervisor {
        &self.supervisor
    }

    /// A handle that quits the hub from elsewhere, for example a Ctrl+C
    /// handler, while [`RunningHub::wait`] has taken it.
    pub fn shutdown_handle(&self) -> ShutdownHandle {
        self.server.shutdown_handle()
    }

    /// Waits until the hub is asked to quit (`POST /api/shutdown` or the
    /// shutdown handle), then until open connections are finished. The
    /// dashboards keep running.
    pub async fn wait(self) -> Result<(), HubError> {
        Ok(self.server.wait().await?)
    }
}

/// Starts a hub: re-attaches to the dashboards of its directory that are
/// still running, deletes the stale run files, removes the staging areas
/// left by processes that ended (not that of a command still running), then
/// serves the hub's API and the frontend on the loopback interface.
///
/// Once serving, it checks GitHub for updates of the dashboards installed
/// from it and of itself, in the background, unless the setting
/// `check_updates_at_start` is off; that check never delays or stops the
/// hub. Also in the background, it removes the executable a self-update
/// replaced (`<executable>.old`), if one is left.
///
/// When a self-update has replaced the hub's executable, the server stops
/// as if asked to quit, and [`RunningHub::wait`] returns; the new
/// executable is then [`Supervisor::restart_executable`].
///
/// It does not write `hub.json`, open a browser or handle Ctrl+C; [`run`]
/// does.
pub async fn start(supervisor: Supervisor, config: ServerConfig) -> Result<RunningHub, HubError> {
    supervisor.reattach().await?;
    supervisor.clean_staging().await;
    let server = eario_runtime::start_routes(http::routes(supervisor.clone()), config).await?;
    let shutdown = server.shutdown_handle();
    let mut restart = supervisor.restart_requests();
    tokio::spawn(async move {
        // An error means the supervisor is gone: nothing will restart.
        if restart.wait_for(Option::is_some).await.is_ok() {
            tracing::info!("the hub updated itself; stopping its server to restart");
            shutdown.shutdown();
        }
    });
    supervisor.check_in_background();
    supervisor.remove_old_executable();
    Ok(RunningHub { server, supervisor })
}

/// The `eario-hub` executable, in a terminal.
///
/// Uses the hub directory of this user (`EARIO_HUB_DIR` overrides it). If
/// `hub.json` names a hub that answers with its token, opens the browser on
/// it and returns. Otherwise starts a hub, records it in `hub.json`, prints
/// its address, opens the browser (unless `EARIO_NO_BROWSER` is set), and
/// returns when the user chooses Quit in the hub or presses Ctrl+C. The
/// dashboards it started keep running.
///
/// After a self-update it returns too, once its server has stopped and
/// `hub.json` is removed, having started the new executable without
/// arguments, standard streams or console window, in a process group of its
/// own: the new hub then finds none running and starts. If the new
/// executable cannot be started, `self-update.log` in the hub directory says
/// so, since this hub may have no terminal.
pub fn run() -> Result<(), HubError> {
    init_logging();
    let hub = HubDir::resolve()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(HubError::Runtime)?;
    runtime.block_on(serve(hub, ServerConfig::default().with_env_overrides()))
}

async fn serve(hub: HubDir, config: ServerConfig) -> Result<(), HubError> {
    let open_browser = config.open_browser;
    if let Some(existing) = running_hub(&hub).await {
        let url = format!("http://{}/#token={}", existing.address, existing.token);
        println!("The Eario hub is already running at {url}");
        if open_browser {
            open_in_browser(&url);
        }
        return Ok(());
    }

    let supervisor = Supervisor::new(hub, SupervisorOptions::default())?;
    let hub = supervisor.hub_dir().clone();
    let running = start(supervisor, config).await?;
    let record = RunFile {
        protocol: LAUNCH_PROTOCOL_VERSION,
        pid: std::process::id(),
        address: running.address().to_string(),
        token: running.token().to_owned(),
        started_at: unix_seconds(),
    };
    let hub_file = hub.hub_file_path();
    if let Err(error) = write_hub_file(&hub, &record).await {
        // The hub works without it; only a second start would not find it.
        tracing::warn!(%error, "could not record the hub in hub.json");
    }

    let url = running.url();
    println!("The Eario hub is running at {url}");
    println!("Press Ctrl+C or choose Quit in the hub to stop it. Dashboards keep running.");
    if open_browser {
        open_in_browser(&url);
    }
    let on_interrupt = running.shutdown_handle();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            tracing::info!("interrupt received, quitting the hub");
            on_interrupt.shutdown();
        }
    });

    let supervisor = running.supervisor().clone();
    let served = running.wait().await;
    if let Err(error) = remove_hub_file(&hub, &record.token).await {
        tracing::warn!(%error, file = %hub_file.display(), "could not remove hub.json");
    }
    // Only now, with the server stopped and `hub.json` removed, so the new
    // hub finds none running; and once a self-update still in progress has
    // ended, so that one whose request Quit cut short restarts the hub too.
    match supervisor.restart_after_self_update().await {
        Some(executable) => {
            // Blocking: it tries again while the system holds the new file.
            let restarted = tokio::task::spawn_blocking(move || restart(&hub, &executable));
            if let Err(error) = restarted.await {
                tracing::error!(%error, "could not start the new hub");
            }
        }
        None => println!("The Eario hub stopped. Dashboards it started keep running."),
    }
    served
}

/// Starts the new hub a self-update installed at `executable`, detached, and
/// says so; the new hub has no terminal and opens the browser itself. A
/// failure is also recorded in `self-update.log` of `hub`
/// ([`self_update::restart`]). Blocking.
fn restart(hub: &HubDir, executable: &std::path::Path) {
    match self_update::restart(hub, executable) {
        // Not waited for: it outlives this process, which exits now.
        Ok(child) => {
            tracing::info!(pid = child.id(), "the new hub started");
            println!(
                "The Eario hub updated itself and restarts without this terminal. Dashboards keep running. If no browser opens, run {} again.",
                executable.display()
            );
        }
        Err(error) => {
            tracing::error!(%error, executable = %executable.display(), "could not start the new hub");
            println!(
                "The Eario hub updated itself but could not start the new version ({error}); start {} again. This is recorded in {}.",
                executable.display(),
                hub.self_update_log_path().display()
            );
        }
    }
}

/// The hub recorded in `hub.json` when it answers with its token.
///
/// Asks `GET /api/alive`, not `/api/app`: listing probes every dashboard and
/// reads the registry, so a stale run file (two seconds to a closed port on
/// Windows) or a broken registry would hide a running hub, and this start
/// would replace it in `hub.json`.
pub(crate) async fn running_hub(hub: &HubDir) -> Option<RunFile> {
    let path = hub.hub_file_path();
    let read = tokio::task::spawn_blocking(move || hub_dir::read_run_file_at(&path)).await;
    let recorded = match read {
        Ok(Ok(recorded)) => recorded?,
        // A file that cannot be read names no hub that can be reached.
        Ok(Err(error)) => {
            tracing::warn!(%error, "ignoring hub.json");
            return None;
        }
        Err(error) => {
            tracing::warn!(%error, "could not read hub.json");
            return None;
        }
    };
    let status =
        hub_dir::request_status(&recorded.address, "GET", http::ALIVE_PATH, &recorded.token);
    (status.await == Some(204)).then_some(recorded)
}

async fn write_hub_file(hub: &HubDir, record: &RunFile) -> Result<(), HubError> {
    let path = hub.hub_file_path();
    let record = record.clone();
    blocking(move || hub_dir::write_run_file_at(&path, &record)).await
}

/// Removes `hub.json` if it is still this hub's: a hub started meanwhile may
/// have replaced it.
async fn remove_hub_file(hub: &HubDir, token: &str) -> Result<(), HubError> {
    let path = hub.hub_file_path();
    let token = token.to_owned();
    blocking(move || match hub_dir::read_run_file_at(&path) {
        Ok(Some(recorded)) if recorded.token == token => hub_dir::remove_run_file_at(&path),
        Ok(_) => Ok(()),
        Err(error) => Err(error),
    })
    .await
}

async fn blocking<F>(call: F) -> Result<(), HubError>
where
    F: FnOnce() -> Result<(), HubDirError> + Send + 'static,
{
    match tokio::task::spawn_blocking(call).await {
        Ok(result) => Ok(result?),
        Err(error) => Err(HubError::Task(error.to_string())),
    }
}

fn open_in_browser(url: &str) {
    if let Err(error) = webbrowser::open(url) {
        tracing::warn!(%error, "could not open a browser; open the address above manually");
    }
}

/// Seconds since the Unix epoch; 0 when the clock is set before it.
pub(crate) fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

/// Installs a log subscriber writing to standard error, unless one is
/// installed. `EARIO_LOG` sets the filter, for example `EARIO_LOG=info`.
pub(crate) fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_env("EARIO_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));
    // Fails when a subscriber is already installed, which is fine.
    let _ =
        tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::io::stderr).try_init();
}
