//! The supervisor: what the hub does, whoever asks.
//!
//! A [`Supervisor`] lists the installed dashboards with their state, and
//! starts, opens, stops and uninstalls them, always by slug. The HTTP API is a
//! thin adapter over it.
//!
//! Nothing on disk is trusted. Every operation reads the registry again, so an
//! install made meanwhile with `--eario-install` is seen at once, and a run
//! file counts only while its dashboard answers with the token it records.
//! Before each start the hub's copy of the executable is checked against the
//! size, SHA-256 and launch protocol version recorded at install time.
//!
//! The supervisor remembers only what the disk cannot tell: the starts in
//! progress, the last failed start of each dashboard, and the processes this
//! hub started, which it can kill when they do not stop. Dropping it leaves
//! those processes running.

use std::collections::{BTreeSet, HashMap};
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use eario_core::{
    CHILD_ARGUMENT, DashboardState, HUB_LOG_LINES, HubDashboard, HubInfo, HubOpen,
    LAUNCH_PROTOCOL_VERSION, RunFile, is_slug,
};
use eario_runtime::hub_dir::{self, HUB_DIR_VARIABLE, HubDir, RegistryEntry};
use eario_runtime::{HubDirError, ServeError};
use tokio::task::JoinSet;

use crate::github::GitHubError;

/// Longest a started dashboard has to answer with its run file's token.
pub const START_TIMEOUT: Duration = Duration::from_secs(30);

/// Longest a dashboard has to stop once asked. A dashboard waits up to five
/// seconds for its open connections, so this leaves it time to do so.
pub const STOP_TIMEOUT: Duration = Duration::from_secs(10);

/// Interval between two looks at a starting or stopping dashboard.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Most of a log file read to find its last lines.
const LOG_TAIL_BYTES: u64 = 64 * 1024;

/// Timeouts of a [`Supervisor`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupervisorOptions {
    /// See [`START_TIMEOUT`].
    pub start_timeout: Duration,
    /// See [`STOP_TIMEOUT`].
    pub stop_timeout: Duration,
}

impl Default for SupervisorOptions {
    fn default() -> Self {
        Self { start_timeout: START_TIMEOUT, stop_timeout: STOP_TIMEOUT }
    }
}

/// Why a hub operation failed. Messages name the dashboard and, where it
/// helps, the file, and say what to do.
#[derive(Debug, thiserror::Error)]
pub enum HubError {
    /// The request named something that is not a slug.
    #[error(
        "`{0}` is not a dashboard identifier; identifiers have lower-case letters, digits and dashes only"
    )]
    InvalidSlug(String),
    /// No dashboard of this slug is in the registry.
    #[error(
        "no dashboard `{slug}` is installed in {registry}; install it by running its executable with --eario-install"
    )]
    NotInstalled {
        /// The slug asked for.
        slug: String,
        /// The registry that was read.
        registry: PathBuf,
    },
    /// A start was asked for a dashboard that answers already.
    #[error("`{0}` is already running; open it, or stop it first")]
    AlreadyRunning(String),
    /// The dashboard must be stopped before this operation.
    #[error("`{0}` is running; stop it first")]
    Running(String),
    /// A start of this dashboard is in progress.
    #[error("`{0}` is starting; wait until it runs or its start fails")]
    Starting(String),
    /// The dashboard does not answer, so it cannot be opened.
    #[error("`{0}` is not running; start it first")]
    NotRunning(String),
    /// The executable speaks a launch protocol this hub does not.
    #[error(
        "`{slug}` was built for launch protocol {found} and this hub speaks {supported}; install a version of the dashboard built with the same version of Eario as the hub"
    )]
    UnsupportedProtocol {
        /// The dashboard.
        slug: String,
        /// Version in its manifest.
        found: u32,
        /// Version of this hub.
        supported: u32,
    },
    /// The hub's copy differs from what was installed.
    #[error(
        "the hub's copy of `{slug}` at {path} was changed after it was installed (its size or SHA-256 differs); reinstall it by running the dashboard's executable with --eario-install"
    )]
    Modified {
        /// The dashboard.
        slug: String,
        /// The copy.
        path: PathBuf,
    },
    /// The hub's copy is gone.
    #[error(
        "the hub's copy of `{slug}` is missing at {path}; reinstall it by running the dashboard's executable with --eario-install"
    )]
    MissingExecutable {
        /// The dashboard.
        slug: String,
        /// Where the copy should be.
        path: PathBuf,
    },
    /// The registry names a file outside the hub's own copy directory.
    #[error(
        "the registry entry of `{slug}` names {path}, outside the hub's directory {expected}; reinstall it by running the dashboard's executable with --eario-install"
    )]
    ForeignExecutable {
        /// The dashboard.
        slug: String,
        /// Path in the registry.
        path: PathBuf,
        /// The directory the copy must be in.
        expected: PathBuf,
    },
    /// The process could not be created.
    #[error("could not start `{slug}` from {path}: {source}")]
    Spawn {
        /// The dashboard.
        slug: String,
        /// The executable.
        path: PathBuf,
        /// The underlying failure.
        source: io::Error,
    },
    /// The dashboard was asked to stop and still answers.
    #[error("`{slug}` did not stop within {seconds} s; quit it from its own window, or try again")]
    StopTimeout {
        /// The dashboard.
        slug: String,
        /// The time it was given.
        seconds: u64,
    },
    /// The hub's copy could not be removed.
    #[error("could not remove {path}: {source}; close any program using it and try again")]
    Remove {
        /// The directory of the copy.
        path: PathBuf,
        /// The underlying failure.
        source: io::Error,
    },
    /// A file of the hub directory could not be read or written.
    #[error(transparent)]
    HubDir(#[from] HubDirError),
    /// The hub's server could not start or stopped with an error.
    #[error(transparent)]
    Serve(#[from] ServeError),
    /// A request to GitHub failed.
    #[error(transparent)]
    GitHub(#[from] GitHubError),
    /// The hub's asynchronous runtime could not be created.
    #[error("could not start the hub's runtime: {0}")]
    Runtime(io::Error),
    /// A background task panicked or was cancelled.
    #[error("a hub operation failed unexpectedly: {0}")]
    Task(String),
}

impl HubError {
    /// Stable, machine-readable code of the error.
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidSlug(_) => "hub_invalid_slug",
            Self::NotInstalled { .. } => "hub_not_installed",
            Self::AlreadyRunning(_) => "hub_already_running",
            Self::Running(_) => "hub_running",
            Self::Starting(_) => "hub_starting",
            Self::NotRunning(_) => "hub_not_running",
            Self::UnsupportedProtocol { .. } => "hub_unsupported_protocol",
            Self::Modified { .. } => "hub_modified_executable",
            Self::MissingExecutable { .. } => "hub_missing_executable",
            Self::ForeignExecutable { .. } => "hub_foreign_executable",
            Self::Spawn { .. } => "hub_spawn_failed",
            Self::StopTimeout { .. } => "hub_stop_timeout",
            Self::Remove { .. } => "hub_remove_failed",
            Self::HubDir(error) => error.code(),
            Self::Serve(error) => error.code(),
            Self::GitHub(error) => error.code(),
            Self::Runtime(_) | Self::Task(_) => "internal_error",
        }
    }
}

/// The hub's services over one hub directory. Cloning it is cheap and
/// shares the state.
#[derive(Clone)]
pub struct Supervisor {
    shared: Arc<Shared>,
}

struct Shared {
    hub: HubDir,
    options: SupervisorOptions,
    /// Serializes the operations that check and then act on the registry or
    /// the processes: start and uninstall. Never held by a stop or a list.
    operations: tokio::sync::Mutex<()>,
    processes: Mutex<Processes>,
}

/// What only this hub process knows.
#[derive(Default)]
struct Processes {
    /// Processes this hub started, by slug, until they are seen to exit.
    children: HashMap<String, Child>,
    /// Starts in progress, by slug, with the number of each start, so that
    /// the task following a start never acts on a later one.
    starting: HashMap<String, u64>,
    /// Slugs whose last start failed, with the lines to show.
    failed: HashMap<String, Vec<String>>,
    /// Number of the last start.
    last_start: u64,
}

impl Processes {
    /// Forgets the processes that have exited, except those starting: their
    /// start watches them.
    fn reap(&mut self) {
        let starting = &self.starting;
        // A process whose state cannot be read is kept and checked next time.
        self.children.retain(|slug, child| {
            starting.contains_key(slug) || !matches!(child.try_wait(), Ok(Some(_)))
        });
    }

    /// Whether start number `start` of `slug` is still in progress.
    fn is_starting(&self, slug: &str, start: u64) -> bool {
        self.starting.get(slug) == Some(&start)
    }
}

/// How a start ended.
enum StartOutcome {
    Running,
    Exited(ExitStatus),
    TimedOut,
}

impl Supervisor {
    /// A supervisor of the hub directory `hub`. Touches nothing on disk.
    pub fn new(hub: HubDir, options: SupervisorOptions) -> Result<Self, HubError> {
        // Children run in another working directory and receive paths in the
        // hub directory, so those paths must be absolute.
        let root = std::path::absolute(hub.root())
            .map_err(|source| HubDirError::Io { path: hub.root().to_owned(), source })?;
        let shared = Shared {
            hub: HubDir::at(root),
            options,
            operations: tokio::sync::Mutex::new(()),
            processes: Mutex::new(Processes::default()),
        };
        Ok(Self { shared: Arc::new(shared) })
    }

    /// The hub directory, with an absolute root.
    pub fn hub_dir(&self) -> &HubDir {
        &self.shared.hub
    }

    fn processes(&self) -> MutexGuard<'_, Processes> {
        // Every change under this lock is one insert or one remove, so the
        // maps are consistent even if a holder panicked.
        self.shared.processes.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// What a hub does when it starts: keeps the run files whose dashboard
    /// answers with its token, which this hub can then open and stop, and
    /// deletes the others, left by a dashboard that crashed. Returns the
    /// slugs that answer.
    pub async fn reattach(&self) -> Result<Vec<String>, HubError> {
        let hub = self.hub_dir().clone();
        let (runs, unreadable) = blocking(move || {
            let mut runs = Vec::new();
            let mut unreadable = Vec::new();
            for slug in hub.run_file_slugs()? {
                match hub.read_run_file(&slug) {
                    Ok(Some(run)) => runs.push((slug, run)),
                    // Removed since the directory was listed.
                    Ok(None) => {}
                    // A run file is always written whole: this one is no
                    // dashboard's.
                    Err(HubDirError::Invalid { .. }) => unreadable.push(slug),
                    // Possibly a write in progress: kept, and checked again at
                    // the next start.
                    Err(error) => {
                        tracing::warn!(dashboard = %slug, %error, "could not read a run file");
                    }
                }
            }
            Ok((runs, unreadable))
        })
        .await?;

        let slugs: Vec<String> = runs.iter().map(|(slug, _)| slug.clone()).collect();
        let live = live_runs(runs).await;
        let mut stale = unreadable;
        stale.extend(slugs.into_iter().filter(|slug| !live.contains_key(slug)));

        let hub = self.hub_dir().clone();
        blocking(move || {
            for slug in &stale {
                match hub.remove_run_file(slug) {
                    Ok(()) => tracing::info!(dashboard = %slug, "deleted a stale run file"),
                    // Not fatal: the next start of that dashboard removes it.
                    Err(error) => {
                        tracing::warn!(dashboard = %slug, %error, "could not delete a stale run file");
                    }
                }
            }
            Ok(())
        })
        .await?;

        let mut attached: Vec<String> = live.into_keys().collect();
        attached.sort();
        for slug in &attached {
            tracing::info!(dashboard = %slug, "re-attached to a running dashboard");
        }
        Ok(attached)
    }

    /// The installed dashboards, in registry order, with their state.
    ///
    /// Run files of installed dashboards that no longer answer are deleted,
    /// unless a start of that dashboard is in progress, so the next list
    /// does not wait on them again.
    pub async fn list(&self) -> Result<HubInfo, HubError> {
        let hub = self.hub_dir().clone();
        let (registry, runs) = blocking(move || {
            let registry = hub.read_registry()?;
            let runs: Vec<(String, RunFile)> = registry
                .dashboards
                .iter()
                // A run file that cannot be read now counts as absent this
                // time; the next list reads it again.
                .filter_map(|entry| match hub.read_run_file(&entry.slug) {
                    Ok(Some(run)) => Some((entry.slug.clone(), run)),
                    _ => None,
                })
                .collect();
            Ok((registry, runs))
        })
        .await?;
        let probed: Vec<(String, String)> =
            runs.iter().map(|(slug, run)| (slug.clone(), run.token.clone())).collect();
        let live = live_runs(runs).await;
        let stale: Vec<(String, String)> = {
            let processes = self.processes();
            probed
                .into_iter()
                .filter(|(slug, _)| {
                    !live.contains_key(slug) && !processes.starting.contains_key(slug)
                })
                .collect()
        };
        if !stale.is_empty() {
            self.remove_run_files(stale).await;
        }

        let mut processes = self.processes();
        processes.reap();
        let dashboards = registry
            .dashboards
            .into_iter()
            .map(|entry| {
                let slug = entry.slug;
                let (state, log_lines) = if processes.starting.contains_key(&slug) {
                    (DashboardState::Starting, Vec::new())
                } else if live.contains_key(&slug) {
                    processes.failed.remove(&slug);
                    (DashboardState::Running, Vec::new())
                } else if let Some(lines) = processes.failed.get(&slug) {
                    (DashboardState::Failed, lines.clone())
                } else {
                    (DashboardState::Stopped, Vec::new())
                };
                HubDashboard {
                    slug,
                    name: entry.manifest.name,
                    version: entry.manifest.version,
                    description: entry.manifest.description,
                    state,
                    log_lines,
                    source: entry.source,
                    // Filled by the update check, which a later ticket adds.
                    available_version: None,
                    has_previous: entry.previous.is_some(),
                }
            })
            .collect();
        Ok(HubInfo { eario_version: env!("CARGO_PKG_VERSION").to_owned(), dashboards })
    }

    /// Starts an installed dashboard and returns once its process is created.
    ///
    /// Refused when the dashboard answers already or is starting, when its
    /// launch protocol is not this hub's, and when the hub's copy is missing,
    /// outside `bin/<slug>/` or differs in size or SHA-256 from the registry.
    /// The process gets `--eario-child <run file>` and nothing else, no
    /// console window (Windows) or its own process group (Unix), no standard
    /// input or output, and the copy's directory as working directory.
    ///
    /// The dashboard is then `starting` until its run file's token answers,
    /// when it is `running`. It is `failed`, with the last lines of its log,
    /// if it exits first or does not answer within the start timeout, in
    /// which case it is killed.
    pub async fn start(&self, slug: &str) -> Result<(), HubError> {
        check_slug(slug)?;
        let _operation = self.shared.operations.lock().await;
        let entry = self.entry(slug).await?;
        if self.processes().starting.contains_key(slug) {
            return Err(HubError::Starting(slug.to_owned()));
        }
        // A second process would replace the run file of the first.
        if self.live_run(slug).await?.is_some() {
            return Err(HubError::AlreadyRunning(slug.to_owned()));
        }
        if entry.manifest.protocol != LAUNCH_PROTOCOL_VERSION {
            return Err(HubError::UnsupportedProtocol {
                slug: slug.to_owned(),
                found: entry.manifest.protocol,
                supported: LAUNCH_PROTOCOL_VERSION,
            });
        }
        // A process of this hub that no longer answers is replaced.
        let previous = self.processes().children.remove(slug);
        if let Some(previous) = previous {
            kill(slug, previous).await;
        }

        let supervisor = self.clone();
        let (child, log_offset) = blocking(move || supervisor.verify_and_spawn(&entry)).await?;
        tracing::info!(dashboard = %slug, pid = child.id(), "started, waiting for it to answer");
        let start = {
            let mut processes = self.processes();
            processes.last_start += 1;
            let start = processes.last_start;
            processes.failed.remove(slug);
            processes.starting.insert(slug.to_owned(), start);
            processes.children.insert(slug.to_owned(), child);
            start
        };
        let supervisor = self.clone();
        let slug = slug.to_owned();
        tokio::spawn(async move { supervisor.watch_start(slug, start, log_offset).await });
        Ok(())
    }

    /// Checks the hub's copy of `entry` and starts it. Returns the process
    /// and the length of its log file before the start. Blocking.
    fn verify_and_spawn(&self, entry: &RegistryEntry) -> Result<(Child, u64), HubError> {
        let hub = self.hub_dir();
        let slug = &entry.slug;
        let directory = hub.bin_dir(slug)?;
        // Only the hub's own copy is ever started, whatever the registry says.
        if entry.path.parent() != Some(directory.as_path()) {
            return Err(HubError::ForeignExecutable {
                slug: slug.clone(),
                path: entry.path.clone(),
                expected: directory,
            });
        }
        let (size, sha256) = match hub_dir::file_digest(&entry.path) {
            Ok(digest) => digest,
            Err(HubDirError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                return Err(HubError::MissingExecutable {
                    slug: slug.clone(),
                    path: entry.path.clone(),
                });
            }
            Err(error) => return Err(error.into()),
        };
        if size != entry.size || sha256 != entry.sha256 {
            return Err(HubError::Modified { slug: slug.clone(), path: entry.path.clone() });
        }

        // A run file left by a dashboard that no longer answers would be
        // taken for this start's.
        hub.remove_run_file(slug)?;
        let run_file = hub.run_file_path(slug)?;
        // Only the lines written by this start are reported if it fails.
        let log_offset = fs::metadata(hub.log_path(slug)?).map_or(0, |metadata| metadata.len());
        let child = spawn(&entry.path, &directory, &run_file, hub.root()).map_err(|source| {
            HubError::Spawn { slug: slug.clone(), path: entry.path.clone(), source }
        })?;
        Ok((child, log_offset))
    }

    /// Follows start number `start` of `slug` until the dashboard answers,
    /// exits or times out. Gives up when a stop cancels the start.
    async fn watch_start(self, slug: String, start: u64, log_offset: u64) {
        let started = Instant::now();
        let outcome = loop {
            tokio::time::sleep(POLL_INTERVAL).await;
            let exited = {
                let mut processes = self.processes();
                if !processes.is_starting(&slug, start) {
                    return;
                }
                match processes.children.get_mut(&slug).map(Child::try_wait) {
                    Some(Ok(Some(status))) => Some(status),
                    _ => None,
                }
            };
            if let Some(status) = exited {
                break StartOutcome::Exited(status);
            }
            if started.elapsed() >= self.shared.options.start_timeout {
                break StartOutcome::TimedOut;
            }
            // An error reading the run file is a write in progress: next time.
            if let Ok(Some(_)) = self.live_run(&slug).await {
                break StartOutcome::Running;
            }
        };

        let reason = match outcome {
            StartOutcome::Running => {
                let mut processes = self.processes();
                if processes.is_starting(&slug, start) {
                    processes.starting.remove(&slug);
                    tracing::info!(dashboard = %slug, "running");
                }
                return;
            }
            StartOutcome::Exited(status) => {
                let mut processes = self.processes();
                if !processes.is_starting(&slug, start) {
                    return;
                }
                processes.children.remove(&slug);
                format!("eario-hub: the dashboard exited ({status}) before answering.")
            }
            StartOutcome::TimedOut => {
                let child = {
                    let mut processes = self.processes();
                    if !processes.is_starting(&slug, start) {
                        return;
                    }
                    processes.children.remove(&slug)
                };
                if let Some(child) = child {
                    kill(&slug, child).await;
                }
                self.remove_stale_run_file(&slug).await;
                format!(
                    "eario-hub: the dashboard did not answer within {} s and was stopped.",
                    self.shared.options.start_timeout.as_secs()
                )
            }
        };
        tracing::warn!(dashboard = %slug, %reason, "start failed");

        let log = self.hub_dir().log_path(&slug);
        let mut lines = blocking(move || {
            Ok(match log {
                Ok(log) => log_tail(&log, log_offset, HUB_LOG_LINES).unwrap_or_else(|error| {
                    vec![format!("eario-hub: could not read the log {}: {error}", log.display())]
                }),
                Err(error) => vec![format!("eario-hub: {error}")],
            })
        })
        .await
        .unwrap_or_else(|error| vec![format!("eario-hub: {error}")]);
        lines.push(reason);

        let mut processes = self.processes();
        // A stop may have cancelled the start meanwhile.
        if processes.is_starting(&slug, start) {
            processes.starting.remove(&slug);
            processes.failed.insert(slug, lines);
        }
    }

    /// Stops an installed dashboard and returns once it no longer runs.
    ///
    /// Asks it with `POST /api/shutdown`. A process this hub started that has
    /// not exited after the stop timeout is killed; any other dashboard that
    /// still answers then is reported. A start in progress is cancelled: its
    /// process is killed. Stopping a dashboard that does not run does
    /// nothing.
    pub async fn stop(&self, slug: &str) -> Result<(), HubError> {
        check_slug(slug)?;
        self.entry(slug).await?;
        self.stop_one(slug).await
    }

    /// Stops every dashboard of this hub directory that runs or is starting,
    /// installed or not, all at once. Each is stopped as by
    /// [`Supervisor::stop`]; the first failure is returned once all were
    /// tried.
    pub async fn stop_all(&self) -> Result<(), HubError> {
        let hub = self.hub_dir().clone();
        let mut slugs: BTreeSet<String> =
            blocking(move || Ok(hub.run_file_slugs()?)).await?.into_iter().collect();
        {
            let processes = self.processes();
            slugs.extend(processes.starting.keys().cloned());
            slugs.extend(processes.children.keys().cloned());
        }

        let mut stops = JoinSet::new();
        for slug in slugs {
            let supervisor = self.clone();
            stops.spawn(async move {
                let result = supervisor.stop_one(&slug).await;
                (slug, result)
            });
        }
        let mut first_error = None;
        while let Some(joined) = stops.join_next().await {
            let error = match joined {
                Ok((_, Ok(()))) => continue,
                Ok((slug, Err(error))) => {
                    tracing::warn!(dashboard = %slug, %error, "could not stop");
                    error
                }
                Err(error) => HubError::Task(error.to_string()),
            };
            first_error.get_or_insert(error);
        }
        first_error.map_or(Ok(()), Err)
    }

    async fn stop_one(&self, slug: &str) -> Result<(), HubError> {
        let (cancelled, owned) = {
            let mut processes = self.processes();
            let cancelled = processes.starting.remove(slug).is_some();
            (cancelled, processes.children.remove(slug))
        };
        if cancelled {
            // It has not answered yet, so it holds nothing to save.
            if let Some(child) = owned {
                kill(slug, child).await;
            }
            self.remove_stale_run_file(slug).await;
            tracing::info!(dashboard = %slug, "start cancelled");
            return Ok(());
        }

        let run = self.live_run(slug).await?;
        if let Some(run) = &run {
            tracing::info!(dashboard = %slug, "asking it to stop");
            if !hub_dir::request_shutdown(&run.address, &run.token).await {
                tracing::warn!(dashboard = %slug, "the dashboard did not accept the stop request");
            }
        }
        let timeout = self.shared.options.stop_timeout;
        match (owned, run) {
            (Some(child), run) => {
                // A process of this hub that does not answer is killed at once.
                let patience = if run.is_some() { timeout } else { Duration::ZERO };
                if let Some(child) = wait_for_exit(child, patience).await {
                    tracing::warn!(dashboard = %slug, "still running after the stop request, killing it");
                    kill(slug, child).await;
                }
                self.remove_stale_run_file(slug).await;
                Ok(())
            }
            (None, Some(run)) => self.wait_until_stopped(slug, &run).await,
            (None, None) => Ok(()),
        }
    }

    /// Waits until a dashboard this hub did not start has removed its run
    /// file, which it does last when it stops.
    async fn wait_until_stopped(&self, slug: &str, run: &RunFile) -> Result<(), HubError> {
        let started = Instant::now();
        let timeout = self.shared.options.stop_timeout;
        loop {
            tokio::time::sleep(POLL_INTERVAL).await;
            let hub = self.hub_dir().clone();
            let name = slug.to_owned();
            let current = blocking(move || Ok(hub.read_run_file(&name)?)).await;
            match current {
                Ok(None) => return Ok(()),
                Ok(Some(current)) if current.token != run.token => return Ok(()),
                _ => {}
            }
            if started.elapsed() >= timeout {
                break;
            }
        }
        // Gone without removing its file: a crash while stopping.
        if !hub_dir::probe(&run.address, &run.token).await {
            self.remove_stale_run_file(slug).await;
            return Ok(());
        }
        Err(HubError::StopTimeout { slug: slug.to_owned(), seconds: timeout.as_secs() })
    }

    /// The address to open a running dashboard at, with its token.
    pub async fn open(&self, slug: &str) -> Result<HubOpen, HubError> {
        check_slug(slug)?;
        self.entry(slug).await?;
        if self.processes().starting.contains_key(slug) {
            return Err(HubError::Starting(slug.to_owned()));
        }
        match self.live_run(slug).await? {
            Some(run) => {
                Ok(HubOpen { url: format!("http://{}/#token={}", run.address, run.token) })
            }
            None => Err(HubError::NotRunning(slug.to_owned())),
        }
    }

    /// Removes an installed dashboard that does not run: the hub's copy in
    /// `bin/<slug>/`, then its registry entry. Its log is kept.
    pub async fn uninstall(&self, slug: &str) -> Result<(), HubError> {
        check_slug(slug)?;
        let _operation = self.shared.operations.lock().await;
        self.entry(slug).await?;
        if self.processes().starting.contains_key(slug) {
            return Err(HubError::Starting(slug.to_owned()));
        }
        if self.live_run(slug).await?.is_some() {
            return Err(HubError::Running(slug.to_owned()));
        }
        // A process of this hub that no longer answers would keep running
        // from the copy about to be removed.
        let owned = self.processes().children.remove(slug);
        if let Some(child) = owned {
            kill(slug, child).await;
        }
        let hub = self.hub_dir().clone();
        let name = slug.to_owned();
        blocking(move || remove_installed(&hub, &name)).await?;
        self.processes().failed.remove(slug);
        tracing::info!(dashboard = %slug, "uninstalled");
        Ok(())
    }

    /// The registry entry of `slug`, read again from disk.
    async fn entry(&self, slug: &str) -> Result<RegistryEntry, HubError> {
        let hub = self.hub_dir().clone();
        let slug = slug.to_owned();
        blocking(move || {
            let registry = hub.read_registry()?;
            registry
                .get(&slug)
                .cloned()
                .ok_or_else(|| HubError::NotInstalled { slug, registry: hub.registry_path() })
        })
        .await
    }

    /// The run file of `slug` when its dashboard answers with its token.
    async fn live_run(&self, slug: &str) -> Result<Option<RunFile>, HubError> {
        let hub = self.hub_dir().clone();
        let slug = slug.to_owned();
        let run = blocking(move || match hub.read_run_file(&slug) {
            Ok(run) => Ok(run),
            // A run file is always written whole: this one is no dashboard's.
            Err(HubDirError::Invalid { .. }) => Ok(None),
            Err(error) => Err(error.into()),
        })
        .await?;
        Ok(match run {
            Some(run) if hub_dir::probe(&run.address, &run.token).await => Some(run),
            _ => None,
        })
    }

    /// Removes run files found not to answer, each only if it still holds the
    /// token that was probed: a dashboard started meanwhile keeps its own.
    async fn remove_run_files(&self, stale: Vec<(String, String)>) {
        let hub = self.hub_dir().clone();
        let removed = blocking(move || {
            for (slug, token) in &stale {
                let removed = hub
                    .run_file_path(slug)
                    .and_then(|path| hub_dir::remove_own_run_file_at(&path, token));
                match removed {
                    Ok(()) => tracing::info!(dashboard = %slug, "deleted a stale run file"),
                    // Not fatal: the next list or start tries again.
                    Err(error) => {
                        tracing::warn!(dashboard = %slug, %error, "could not delete a stale run file");
                    }
                }
            }
            Ok(())
        })
        .await;
        if let Err(error) = removed {
            tracing::warn!(%error, "could not delete stale run files");
        }
    }

    /// Removes the run file of `slug` unless its dashboard answers.
    async fn remove_stale_run_file(&self, slug: &str) {
        let Ok(None) = self.live_run(slug).await else { return };
        let hub = self.hub_dir().clone();
        let name = slug.to_owned();
        if let Err(error) = blocking(move || Ok(hub.remove_run_file(&name)?)).await {
            // Not fatal: the next start of that dashboard removes it.
            tracing::warn!(dashboard = %slug, %error, "could not delete a stale run file");
        }
    }
}

/// Runs blocking work (file system, hashing, processes) off the asynchronous
/// runtime.
async fn blocking<T, F>(call: F) -> Result<T, HubError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, HubError> + Send + 'static,
{
    tokio::task::spawn_blocking(call).await.map_err(|error| HubError::Task(error.to_string()))?
}

fn check_slug(slug: &str) -> Result<(), HubError> {
    if is_slug(slug) { Ok(()) } else { Err(HubError::InvalidSlug(slug.to_owned())) }
}

/// The run files that answer with their token, by slug, probed together.
async fn live_runs(runs: Vec<(String, RunFile)>) -> HashMap<String, RunFile> {
    let mut probes = JoinSet::new();
    for (slug, run) in runs {
        probes.spawn(async move {
            let live = hub_dir::probe(&run.address, &run.token).await;
            (slug, run, live)
        });
    }
    let mut live = HashMap::new();
    while let Some(joined) = probes.join_next().await {
        match joined {
            Ok((slug, run, true)) => {
                live.insert(slug, run);
            }
            Ok(_) => {}
            // Counted as not answering, which the next look corrects.
            Err(error) => tracing::warn!(%error, "a liveness probe failed"),
        }
    }
    live
}

/// Starts `executable` in child mode, as the launch contract defines it.
fn spawn(executable: &Path, directory: &Path, run_file: &Path, hub: &Path) -> io::Result<Child> {
    let mut command = Command::new(executable);
    command
        .arg(CHILD_ARGUMENT)
        .arg(run_file)
        .current_dir(directory)
        // The dashboard finds its log file through the hub directory.
        .env(HUB_DIR_VARIABLE, hub)
        // Each dashboard asks the system for a free port.
        .env_remove("EARIO_PORT")
        // A dashboard that fails before its log starts writes to standard
        // error; nobody would read it.
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    detach(&mut command);
    command.spawn()
}

/// No console window, and a process group of its own, so that a Ctrl+C in
/// the hub's console does not reach the dashboard.
#[cfg(windows)]
fn detach(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
}

/// A process group of its own, so that a Ctrl+C in the hub's terminal does
/// not reach the dashboard.
#[cfg(unix)]
fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(any(windows, unix)))]
fn detach(_command: &mut Command) {}

/// Waits up to `patience` for `child` to exit; returns it if it has not.
async fn wait_for_exit(mut child: Child, patience: Duration) -> Option<Child> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return None,
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(%error, "could not check whether a dashboard exited");
                return Some(child);
            }
        }
        if started.elapsed() >= patience {
            return Some(child);
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// Kills a process this hub started and waits for it to be gone.
async fn kill(slug: &str, mut child: Child) {
    let killed = blocking(move || {
        // Fails when the process has exited already, which is the goal.
        if let Err(error) = child.kill() {
            tracing::debug!(%error, "kill failed, the process had exited");
        }
        child.wait().map_err(|error| HubError::Task(error.to_string()))
    })
    .await;
    match killed {
        Ok(status) => tracing::info!(dashboard = %slug, %status, "process killed"),
        Err(error) => tracing::warn!(dashboard = %slug, %error, "could not kill the process"),
    }
}

/// Removes the hub's copy of `slug`, then its registry entry, as
/// `--eario-uninstall` does. Only the hub's own directory for this slug is
/// removed, never a path read from the registry; it goes first, so if it
/// cannot be removed the entry stays and the user can try again.
fn remove_installed(hub: &HubDir, slug: &str) -> Result<(), HubError> {
    let directory = hub.bin_dir(slug)?;
    match fs::remove_dir_all(&directory) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(source) => return Err(HubError::Remove { path: directory, source }),
    }
    // Read again: an install may have changed the registry meanwhile.
    let mut registry = hub.read_registry()?;
    registry.remove(slug);
    hub.write_registry(&registry)?;
    hub.remove_run_file(slug)?;
    Ok(())
}

/// The last `count` lines of the file at `path` written after byte `offset`.
/// When the file is shorter than `offset` it was rotated meanwhile, and its
/// whole content is new. A missing file has no lines.
fn log_tail(path: &Path, offset: u64, count: usize) -> io::Result<Vec<String>> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let length = file.metadata()?.len();
    let offset = if offset > length { 0 } else { offset };
    let start = offset.max(length.saturating_sub(LOG_TAIL_BYTES));
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let mut lines: Vec<String> =
        String::from_utf8_lossy(&bytes).lines().map(str::to_owned).collect();
    if start > offset && !lines.is_empty() {
        // Starts in the middle of a line.
        lines.remove(0);
    }
    let first = lines.len().saturating_sub(count);
    Ok(lines.split_off(first))
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn log_tail_keeps_the_last_lines_written_since_the_offset() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("a.log");
        assert!(log_tail(&path, 0, 3)?.is_empty(), "a missing log has no lines");

        fs::write(&path, "old 1\nold 2\n")?;
        let offset = fs::metadata(&path)?.len();
        let mut text = fs::read_to_string(&path)?;
        for line in 1..=5 {
            text.push_str(&format!("new {line}\n"));
        }
        fs::write(&path, &text)?;
        assert_eq!(log_tail(&path, offset, 3)?, ["new 3", "new 4", "new 5"]);
        assert_eq!(log_tail(&path, offset, 10)?.len(), 5, "older runs are left out");
        assert_eq!(log_tail(&path, 0, 10)?.len(), 7);

        // Rotated since: the file is shorter than the offset.
        fs::write(&path, "fresh\n")?;
        assert_eq!(log_tail(&path, offset + 100, 3)?, ["fresh"]);
        Ok(())
    }

    #[test]
    fn log_tail_reads_only_the_end_of_a_long_log() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("long.log");
        let text: String = (0..20_000).map(|line| format!("line {line}\n")).collect();
        fs::write(&path, &text)?;
        assert!(text.len() as u64 > LOG_TAIL_BYTES);
        assert_eq!(log_tail(&path, 0, 2)?, ["line 19998", "line 19999"]);
        Ok(())
    }

    #[test]
    fn only_slugs_are_accepted() {
        for name in ["..", "../bin", "a/b", "a\\b", "Data Explorer", "a.b", "", "-a"] {
            let error = check_slug(name).err().map(|error| error.code());
            assert_eq!(error, Some("hub_invalid_slug"), "{name:?}");
        }
        assert!(check_slug("data-explorer").is_ok());
    }

    #[tokio::test]
    async fn operations_on_an_unknown_dashboard_name_the_registry() -> TestResult {
        let directory = tempfile::tempdir()?;
        let supervisor = Supervisor::new(HubDir::at(directory.path()), Default::default())?;
        assert!(supervisor.list().await?.dashboards.is_empty());
        let error = supervisor.start("nope").await.err().ok_or("unknown dashboard")?;
        assert_eq!(error.code(), "hub_not_installed");
        assert!(error.to_string().contains("registry.json"), "{error}");
        assert!(error.to_string().contains("--eario-install"), "{error}");
        for error in [
            supervisor.stop("nope").await.err(),
            supervisor.open("nope").await.err(),
            supervisor.uninstall("nope").await.err(),
        ] {
            assert_eq!(error.map(|error| error.code()), Some("hub_not_installed"));
        }
        supervisor.stop_all().await?;
        Ok(())
    }
}
