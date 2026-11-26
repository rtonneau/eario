//! The local HTTP server.
//!
//! [`start`] binds the loopback interface and serves an [`App`]; it does not
//! open a browser or install signal handlers, so it can be embedded in tests
//! or, later, a desktop shell. [`start_routes`] does the same for any other
//! set of API routes, such as the hub's, behind the same protection. [`run`]
//! is the blocking entry point of an ordinary dashboard executable, and
//! [`run_child`] that of a dashboard started by a hub, which logs with
//! [`init_file_logging`].

use std::net::{Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use axum::Router;
use eario_core::{LAUNCH_PROTOCOL_VERSION, RunFile};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::app::App;
use crate::error::ServeError;
use crate::http::{self, Security};
use crate::hub_dir;
use crate::log_file::{LOG_FILE_LIMIT, RotatingFile};

/// Time given to open connections to finish once shutdown is requested.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

/// Settings of the local server.
pub struct ServerConfig {
    /// Port to listen on. 0 asks the system for a free port.
    pub port: u16,
    /// Open the dashboard in the default browser once the server is up.
    pub open_browser: bool,
    /// Largest accepted file upload, in bytes.
    pub max_upload_bytes: usize,
    /// Additional origin allowed to call the API, for a frontend development
    /// server. Leave `None` in applications.
    pub dev_origin: Option<String>,
    /// Routes served beside the API, behind the same host, origin and token
    /// checks. Used to mount the MCP endpoint.
    pub extra_routes: Option<Router>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: 0,
            open_browser: true,
            max_upload_bytes: 256 * 1024 * 1024,
            dev_origin: None,
            extra_routes: None,
        }
    }
}

impl ServerConfig {
    /// Applies the environment overrides meant for development and automation:
    ///
    /// - `EARIO_PORT`: port to listen on.
    /// - `EARIO_NO_BROWSER`: when set to anything but `0`, do not open a browser.
    /// - `EARIO_DEV_ORIGIN`: additional allowed origin; debug builds only.
    pub fn with_env_overrides(mut self) -> Self {
        if let Ok(port) = std::env::var("EARIO_PORT") {
            match port.parse() {
                Ok(port) => self.port = port,
                Err(_) => tracing::warn!(value = port, "ignoring EARIO_PORT: not a port number"),
            }
        }
        if std::env::var("EARIO_NO_BROWSER").is_ok_and(|value| value != "0") {
            self.open_browser = false;
        }
        if cfg!(debug_assertions)
            && let Ok(origin) = std::env::var("EARIO_DEV_ORIGIN")
        {
            self.dev_origin = Some(origin);
        }
        self
    }
}

/// A server that is accepting connections.
pub struct RunningServer {
    address: SocketAddr,
    token: String,
    shutdown: watch::Sender<bool>,
    task: JoinHandle<std::io::Result<()>>,
}

impl RunningServer {
    /// Socket address the server listens on, for example `127.0.0.1:49152`.
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// Address of the server, for example `http://127.0.0.1:49152`.
    pub fn base_url(&self) -> String {
        format!("http://{}", self.address)
    }

    /// Address to open in a browser. The session token travels in the URL
    /// fragment, which browsers never send to a server.
    pub fn url(&self) -> String {
        format!("{}/#token={}", self.base_url(), self.token)
    }

    /// The session token API clients must present.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Asks the server to stop. Returns immediately.
    pub fn shutdown(&self) {
        // Sending only fails when the server has already stopped.
        let _ = self.shutdown.send(true);
    }

    /// A handle that stops the server from elsewhere, for example from a
    /// Ctrl+C handler, while [`RunningServer::wait`] has taken the server.
    pub fn shutdown_handle(&self) -> ShutdownHandle {
        ShutdownHandle(self.shutdown.clone())
    }

    /// Waits until shutdown is requested, by [`RunningServer::shutdown`] or
    /// by the user interface, then until open connections are finished.
    pub async fn wait(mut self) -> Result<(), ServeError> {
        let mut requested = self.shutdown.subscribe();
        tokio::select! {
            finished = &mut self.task => return task_result(finished),
            _ = requested.wait_for(|requested| *requested) => {}
        }
        match tokio::time::timeout(SHUTDOWN_GRACE, &mut self.task).await {
            Ok(finished) => task_result(finished),
            Err(_) => {
                tracing::warn!("connections still open after the grace period, stopping anyway");
                self.task.abort();
                Ok(())
            }
        }
    }
}

/// Stops a [`RunningServer`]; see [`RunningServer::shutdown_handle`].
#[derive(Clone)]
pub struct ShutdownHandle(watch::Sender<bool>);

impl ShutdownHandle {
    /// Asks the server to stop. Returns immediately.
    pub fn shutdown(&self) {
        // Sending only fails when the server has already stopped.
        let _ = self.0.send(true);
    }
}

fn task_result(
    finished: Result<std::io::Result<()>, tokio::task::JoinError>,
) -> Result<(), ServeError> {
    match finished {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(ServeError::Serve(error)),
        Err(error) => Err(ServeError::Task(error.to_string())),
    }
}

/// Generates the session token: 32 random bytes, in hexadecimal.
fn new_token() -> Result<String, ServeError> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|error| ServeError::Token(error.to_string()))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// Starts serving `app` on the loopback interface.
pub async fn start(app: App, config: ServerConfig) -> Result<RunningServer, ServeError> {
    let api_routes = http::dashboard_api(app, config.max_upload_bytes);
    start_routes(api_routes, config).await
}

/// Starts serving `api_routes` on the loopback interface: the entry point
/// for a server that is not a dashboard, such as a hub. [`start`] is this
/// function with the routes of an [`App`].
///
/// The routes are mounted under `/api`, so a route `/dashboards` answers at
/// `/api/dashboards`; they include `/app`, which tells the frontend what to
/// show (see [`eario_core::AppResponse`]). Give the router its state before
/// passing it (build it with `with_state`). The server adds `POST
/// /api/shutdown`, which stops it as it does for a dashboard, so the routes
/// must not define `/shutdown` (axum panics on the duplicate route when the
/// server is assembled); any other unknown `/api` path answers 404.
/// The routes sit behind the same Host and Origin check and session token as
/// a dashboard's, and the embedded frontend is served beside them.
/// `config.max_upload_bytes` only applies to [`start`]; set a body limit on
/// the routes that need one.
///
/// Read [`RunningServer::address`] and [`RunningServer::token`] to publish how
/// to reach the server.
pub async fn start_routes(
    api_routes: Router,
    config: ServerConfig,
) -> Result<RunningServer, ServeError> {
    let requested = SocketAddr::from((Ipv4Addr::LOCALHOST, config.port));
    let bind_error = |source| ServeError::Bind { port: config.port, source };
    let listener = TcpListener::bind(requested).await.map_err(bind_error)?;
    let address = listener.local_addr().map_err(bind_error)?;

    let token = new_token()?;
    let security = Arc::new(Security::for_port(token.clone(), address.port(), config.dev_origin));
    let (shutdown, mut stop) = watch::channel(false);
    let router = http::router(api_routes, security, shutdown.clone(), config.extra_routes);

    let task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                // An error means every sender is gone, which also ends the server.
                let _ = stop.wait_for(|requested| *requested).await;
            })
            .await
    });
    tracing::info!(%address, "server listening");
    Ok(RunningServer { address, token, shutdown, task })
}

/// Serves `app` until the user quits: the blocking entry point of a dashboard
/// executable.
///
/// Prints the address, opens the browser when configured to, and returns
/// after the user chooses *Quit* in the interface or presses Ctrl+C.
pub fn run(app: App, config: ServerConfig) -> Result<(), ServeError> {
    init_logging();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(ServeError::Runtime)?;
    runtime.block_on(async move {
        let open_browser = config.open_browser;
        let title = app.spec().title.clone();
        let server = start(app, config).await?;
        let url = server.url();
        println!("{title} is running at {url}");
        println!("Press Ctrl+C or choose Quit in the application to stop.");

        if open_browser && let Err(error) = webbrowser::open(&url) {
            tracing::warn!(%error, "could not open a browser; open the address above manually");
        }

        let on_interrupt = server.shutdown_handle();
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                tracing::info!("interrupt received, shutting down");
                on_interrupt.shutdown();
            }
        });

        server.wait().await?;
        println!("{title} stopped.");
        Ok(())
    })
}

/// Serves `app` for a hub: the blocking entry point of a dashboard started
/// with `--eario-child <run file>`.
///
/// Unlike [`run`], it prints nothing, opens no browser whatever `config`
/// says, and installs no Ctrl+C handler. Once listening it writes the run file
/// at `run_file` atomically; it returns after `POST /api/shutdown` (*Quit* in
/// the interface, or the hub), having removed the run file. Call
/// [`init_file_logging`] first, so its log goes to the hub's log file.
pub fn run_child(app: App, config: ServerConfig, run_file: &Path) -> Result<(), ServeError> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(ServeError::Runtime)?;
    runtime.block_on(async move {
        let server = start(app, ServerConfig { open_browser: false, ..config }).await?;
        let record = RunFile {
            protocol: LAUNCH_PROTOCOL_VERSION,
            pid: std::process::id(),
            address: server.address().to_string(),
            token: server.token().to_owned(),
            started_at: unix_seconds(),
        };
        if let Err(error) = hub_dir::write_run_file_at(run_file, &record) {
            server.shutdown();
            // The failure to report is the run file's; the server is only
            // being stopped because nobody can reach it without that file.
            let _ = server.wait().await;
            return Err(error.into());
        }
        tracing::info!(run_file = %run_file.display(), address = %record.address, "run file written");

        let served = server.wait().await;
        let removed = hub_dir::remove_run_file_at(run_file);
        tracing::info!(run_file = %run_file.display(), "stopped");
        served?;
        Ok(removed?)
    })
}

/// Seconds since the Unix epoch; 0 when the clock is set before it, which
/// only affects the start time the hub displays.
fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

/// Installs a log subscriber writing to `path`, for a dashboard started by a
/// hub, unless the application installed its own.
///
/// The file is appended to and rotated past 1 MiB: the full file is renamed
/// with `.1` appended, replacing the previous one. `EARIO_LOG` sets the
/// filter; the default is `info`, so the file shows when the server started
/// and stopped.
pub fn init_file_logging(path: &Path) -> Result<(), ServeError> {
    let file = RotatingFile::open(path, LOG_FILE_LIMIT)
        .map_err(|source| ServeError::LogFile { path: path.to_owned(), source })?;
    let filter = tracing_subscriber::EnvFilter::try_from_env("EARIO_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    // Fails when a subscriber is already installed, which is fine.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_writer(Mutex::new(file))
        .try_init();
    Ok(())
}

/// Installs a log subscriber writing to standard error, unless the
/// application installed its own. `EARIO_LOG` sets the filter, for example
/// `EARIO_LOG=debug`.
fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_env("EARIO_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));
    // Fails when a subscriber is already installed, which is fine.
    let _ =
        tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::io::stderr).try_init();
}
