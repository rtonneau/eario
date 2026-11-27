//! The supervisor: what the hub does, whoever asks.
//!
//! A [`Supervisor`] lists the installed dashboards with their state, and
//! starts, opens, stops and uninstalls them, always by slug. It also reads
//! catalogs, previews and installs GitHub releases, checks for updates,
//! updates and rolls back (ADR 0013; the blocking work is in
//! [`crate::remote`]). The HTTP API and the command line are thin adapters
//! over it.
//!
//! Nothing on disk is trusted. Every operation reads the registry again, so an
//! install made meanwhile with `--eario-install` is seen at once, and a run
//! file counts only while its dashboard answers with the token it records.
//! Every change that reads a file of the hub directory and writes it back
//! holds the hub directory's lock ([`HubDir::lock`]), as `eario-hub` given a
//! command and `--eario-install` do, so no process undoes another's change.
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
    CHILD_ARGUMENT, DashboardState, HUB_LOG_LINES, HubCatalog, HubDashboard, HubInfo, HubOpen,
    HubSettings, InstallPreview, LAUNCH_PROTOCOL_VERSION, RunFile, Source, UpdateCheck, is_repo,
    is_slug,
};
use eario_runtime::hub_dir::{self, HUB_DIR_VARIABLE, HubDir, RegistryEntry};
use eario_runtime::{HubDirError, ServeError};
use tokio::task::JoinSet;

use crate::github::{GitHub, GitHubClient, GitHubError};
use crate::remote::{
    self, MANIFEST_TIMEOUT, MAX_STAGED, Purpose, StageRequest, Staged, StagingArea, same_repo,
};
use crate::verify::{PublicKey, Target, VerifyError, is_sha256};

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
    /// See [`MANIFEST_TIMEOUT`].
    pub manifest_timeout: Duration,
}

impl Default for SupervisorOptions {
    fn default() -> Self {
        Self {
            start_timeout: START_TIMEOUT,
            stop_timeout: STOP_TIMEOUT,
            manifest_timeout: MANIFEST_TIMEOUT,
        }
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
    /// A release, or a key for it, was refused by its verification.
    #[error(transparent)]
    Verify(#[from] VerifyError),
    /// Releases are not published for the platform the hub runs on.
    #[error(
        "this hub runs on a platform GitHub releases of dashboards are not published for ({os} {arch}); install dashboards built for it with --eario-install",
        os = std::env::consts::OS,
        arch = std::env::consts::ARCH
    )]
    UnsupportedPlatform,
    /// The release has no asset for this machine.
    #[error(
        "the latest release of `{repo}` has no asset `{asset}` for this computer; ask its author to publish one built for this platform"
    )]
    NoAsset {
        /// The repository.
        repo: String,
        /// The asset looked for, or the pattern of its name.
        asset: String,
    },
    /// The release has several assets a first install could take.
    #[error(
        "the latest release of `{repo}` has several assets ending in `{ending}` ({assets}), so the hub cannot tell which dashboard to install; a repository installed from the hub publishes one dashboard"
    )]
    AmbiguousAsset {
        /// The repository.
        repo: String,
        /// The ending of the asset names for this machine.
        ending: String,
        /// Their names.
        assets: String,
    },
    /// The verified executable did not print its manifest.
    #[error(
        "`{asset}` of `{repo}` was verified, but did not give its manifest: {detail}; nothing was installed"
    )]
    ManifestFailed {
        /// The repository.
        repo: String,
        /// The asset.
        asset: String,
        /// What happened.
        detail: String,
    },
    /// The verified executable is another dashboard than its name says.
    #[error(
        "`{asset}` of `{repo}` says it is the dashboard `{found}`, not `{expected}` as its name says; nothing was installed"
    )]
    ManifestMismatch {
        /// The repository.
        repo: String,
        /// The asset.
        asset: String,
        /// The slug of its name.
        expected: String,
        /// The slug of its manifest.
        found: String,
    },
    /// A dashboard of that slug is installed from somewhere else.
    #[error(
        "a dashboard `{slug}` is already installed from {installed_from}; uninstall it before installing `{repo}`"
    )]
    SlugTaken {
        /// The slug.
        slug: String,
        /// The repository asked for.
        repo: String,
        /// Where the installed one comes from.
        installed_from: String,
    },
    /// The repository is installed already.
    #[error("`{repo}` is already installed, as `{slug}`; update it instead")]
    AlreadyInstalled {
        /// The repository.
        repo: String,
        /// The slug it is installed as.
        slug: String,
    },
    /// Nothing was previewed for the repository.
    #[error("no verified release of `{repo}` is waiting to be installed; preview it first")]
    NoPreview {
        /// The repository.
        repo: String,
    },
    /// The verified file of a preview is no longer in the staging directory.
    #[error(
        "the verified release of `{repo}` is no longer in the hub's staging directory ({path}), so nothing was accepted or installed; preview it again"
    )]
    PreviewGone {
        /// The repository.
        repo: String,
        /// Where the file was.
        path: PathBuf,
    },
    /// The confirmation names another file than the preview.
    #[error(
        "the release of `{repo}` waiting to be installed has another SHA-256 than {sha256}; preview it again and confirm what the preview shows"
    )]
    PreviewMismatch {
        /// The repository.
        repo: String,
        /// The SHA-256 given.
        sha256: String,
    },
    /// A request gave something that is not a SHA-256.
    #[error("`{0}` is not a SHA-256: give the 64 lower-case hexadecimal digits the preview shows")]
    InvalidSha256(String),
    /// The key is unknown and the user has not accepted it.
    #[error(
        "no catalog lists `{repo}`, so the key its release is signed with is not trusted yet, and the hub has not run the downloaded executable; compare that key with the one its author publishes, then accept it by its SHA-256 {key_sha256}"
    )]
    KeyNotAccepted {
        /// The repository.
        repo: String,
        /// SHA-256 of the key.
        key_sha256: String,
    },
    /// The key changed and the user has not accepted the new one.
    #[error(
        "the release of `{repo}` is signed with another key than the one trusted for it: its author may have changed keys, or someone else published this release; accept the new key (SHA-256 {key_sha256}) only once its author confirms the change"
    )]
    KeyChanged {
        /// The repository.
        repo: String,
        /// SHA-256 of the new key.
        key_sha256: String,
    },
    /// The key accepted is not the previewed one.
    #[error(
        "the key the release of `{repo}` is signed with has SHA-256 {expected}, not {given}; nothing was accepted"
    )]
    KeyMismatch {
        /// The repository.
        repo: String,
        /// SHA-256 of the previewed key.
        expected: String,
        /// SHA-256 given.
        given: String,
    },
    /// A local install is not updated from GitHub.
    #[error(
        "`{slug}` was installed from an executable on this computer, so it is not updated from GitHub; run the newer executable with --eario-install"
    )]
    NotFromGitHub {
        /// The dashboard.
        slug: String,
    },
    /// The release is not newer than what is installed.
    #[error(
        "the latest release of `{slug}` is version {found}, not newer than the installed version {installed}; nothing to update (only a rollback installs an older version)"
    )]
    NotNewer {
        /// The dashboard.
        slug: String,
        /// Installed version.
        installed: String,
        /// Version of the release.
        found: String,
    },
    /// The versions cannot be compared.
    #[error(
        "`{slug}` cannot be updated in place: its installed version ({installed}) or the release's ({found}) is not a SemVer version such as 1.2.0, so the hub cannot tell an update from a downgrade; uninstall it, then install it again"
    )]
    VersionNotComparable {
        /// The dashboard.
        slug: String,
        /// Installed version.
        installed: String,
        /// Version of the release.
        found: String,
    },
    /// No update left a previous version.
    #[error(
        "`{slug}` has no previous version to roll back to; only an update from GitHub keeps one"
    )]
    NoPrevious {
        /// The dashboard.
        slug: String,
    },
    /// The previous version's copy cannot be restored.
    #[error(
        "the previous version of `{slug}` kept at {path} is missing or was changed, so it cannot be restored"
    )]
    PreviousUnusable {
        /// The dashboard.
        slug: String,
        /// The copy.
        path: PathBuf,
    },
    /// The dashboard changed while an update was prepared.
    #[error("`{slug}` was changed in the hub while the update was prepared; try again")]
    ChangedMeanwhile {
        /// The dashboard.
        slug: String,
    },
    /// The repository is not a catalog of the hub.
    #[error("`{repo}` is not one of the hub's catalogs")]
    UnknownCatalog {
        /// The repository.
        repo: String,
    },
    /// A catalog is written in a format this hub does not read.
    #[error(
        "`catalog.json` of `{repo}` has format version {found}, but this hub reads up to {supported}; update the hub"
    )]
    CatalogVersion {
        /// The catalog's repository.
        repo: String,
        /// Version found.
        found: u32,
        /// Newest version this hub reads.
        supported: u32,
    },
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
            Self::Verify(error) => error.code(),
            Self::UnsupportedPlatform => "hub_unsupported_platform",
            Self::NoAsset { .. } => "hub_no_asset",
            Self::AmbiguousAsset { .. } => "hub_ambiguous_asset",
            Self::ManifestFailed { .. } => "hub_manifest_failed",
            Self::ManifestMismatch { .. } => "hub_manifest_mismatch",
            Self::SlugTaken { .. } => "hub_slug_taken",
            Self::AlreadyInstalled { .. } => "hub_already_installed",
            Self::NoPreview { .. } => "hub_no_preview",
            Self::PreviewGone { .. } => "hub_preview_gone",
            Self::PreviewMismatch { .. } => "hub_preview_mismatch",
            Self::InvalidSha256(_) => "hub_invalid_sha256",
            Self::KeyNotAccepted { .. } => "hub_key_not_accepted",
            Self::KeyChanged { .. } => "hub_key_changed",
            Self::KeyMismatch { .. } => "hub_key_mismatch",
            Self::NotFromGitHub { .. } => "hub_not_from_github",
            Self::NotNewer { .. } => "hub_not_newer",
            Self::VersionNotComparable { .. } => "hub_version_not_comparable",
            Self::NoPrevious { .. } => "hub_no_previous",
            Self::PreviousUnusable { .. } => "hub_previous_unusable",
            Self::ChangedMeanwhile { .. } => "hub_changed_meanwhile",
            Self::UnknownCatalog { .. } => "hub_unknown_catalog",
            Self::CatalogVersion { .. } => "hub_catalog_unknown_version",
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
    /// the processes: start, uninstall, install, update and rollback. Never
    /// held by a stop, a list or a download.
    operations: tokio::sync::Mutex<()>,
    processes: Mutex<Processes>,
    github: GitHubAccess,
    remote: Mutex<RemoteState>,
    /// This supervisor's staging area, created at the first preview.
    staging: Mutex<Option<Arc<StagingArea>>>,
}

/// How the supervisor reaches GitHub.
enum GitHubAccess {
    /// Through this client: a fake GitHub in tests.
    Given(Arc<dyn GitHub>),
    /// Through a [`GitHubClient`] of this process's endpoint, created at
    /// first use, so a hub that never contacts GitHub never reads the
    /// system's certificates.
    FromEnvironment(Mutex<Option<Arc<dyn GitHub>>>),
}

/// What only this hub process knows about installs from GitHub.
#[derive(Default)]
struct RemoteState {
    /// Verified releases waiting to be confirmed, oldest first, at most one
    /// per repository and [`MAX_STAGED`] in all.
    staged: Vec<Staged>,
    /// Keys the user accepted, by repository in lower case, until an install
    /// pins them.
    accepted: HashMap<String, PublicKey>,
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
    ///
    /// It reaches GitHub with a [`GitHubClient`] of this process's endpoint
    /// (see [`crate::github::Endpoint::from_env`]), created when first
    /// needed.
    pub fn new(hub: HubDir, options: SupervisorOptions) -> Result<Self, HubError> {
        Self::with_access(hub, options, GitHubAccess::FromEnvironment(Mutex::new(None)))
    }

    /// A supervisor of the hub directory `hub` that reaches GitHub through
    /// `github`, for example a client of a fake GitHub. Touches nothing on
    /// disk.
    pub fn with_github(
        hub: HubDir,
        options: SupervisorOptions,
        github: Arc<dyn GitHub>,
    ) -> Result<Self, HubError> {
        Self::with_access(hub, options, GitHubAccess::Given(github))
    }

    fn with_access(
        hub: HubDir,
        options: SupervisorOptions,
        github: GitHubAccess,
    ) -> Result<Self, HubError> {
        // Children run in another working directory and receive paths in the
        // hub directory, so those paths must be absolute.
        let root = std::path::absolute(hub.root())
            .map_err(|source| HubDirError::Io { path: hub.root().to_owned(), source })?;
        let shared = Shared {
            hub: HubDir::at(root),
            options,
            operations: tokio::sync::Mutex::new(()),
            processes: Mutex::new(Processes::default()),
            github,
            remote: Mutex::new(RemoteState::default()),
            staging: Mutex::new(None),
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

    fn remote_state(&self) -> MutexGuard<'_, RemoteState> {
        // As for `processes`: one insert, one remove or one field set at a
        // time.
        self.shared.remote.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The GitHub client. Blocking: the first call may read the system's
    /// certificates.
    fn github(&self) -> Result<Arc<dyn GitHub>, HubError> {
        match &self.shared.github {
            GitHubAccess::Given(github) => Ok(Arc::clone(github)),
            GitHubAccess::FromEnvironment(slot) => {
                // Holds a client or nothing, whatever a holder did.
                let mut slot = slot.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(github) = slot.as_ref() {
                    return Ok(Arc::clone(github));
                }
                let github: Arc<dyn GitHub> = Arc::new(GitHubClient::new()?);
                *slot = Some(Arc::clone(&github));
                Ok(github)
            }
        }
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
        let (registry, runs, last_check) = blocking(move || {
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
            // A check that cannot be read only hides the available updates.
            let last_check = remote::read_last_check(&hub).unwrap_or_else(|error| {
                tracing::warn!(%error, "could not read the last update check");
                None
            });
            Ok((registry, runs, last_check))
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
                let available_version = remote::available_version(last_check.as_ref(), &entry);
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
                    available_version,
                    has_previous: entry.previous.is_some(),
                }
            })
            .collect();
        Ok(HubInfo { eario_version: env!("CARGO_PKG_VERSION").to_owned(), dashboards, last_check })
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
        self.make_idle(slug).await?;
        let hub = self.hub_dir().clone();
        let name = slug.to_owned();
        blocking(move || remove_installed(&hub, &name)).await?;
        self.processes().failed.remove(slug);
        tracing::info!(dashboard = %slug, "uninstalled");
        Ok(())
    }

    /// Refuses when `slug` is starting or answers, so its copy can be
    /// replaced or removed. A process of this hub that no longer answers is
    /// killed: it would keep running from that copy.
    async fn make_idle(&self, slug: &str) -> Result<(), HubError> {
        if self.processes().starting.contains_key(slug) {
            return Err(HubError::Starting(slug.to_owned()));
        }
        if self.live_run(slug).await?.is_some() {
            return Err(HubError::Running(slug.to_owned()));
        }
        let owned = self.processes().children.remove(slug);
        if let Some(child) = owned {
            kill(slug, child).await;
        }
        Ok(())
    }

    /// The registry entry of `slug`, as installed now.
    pub async fn installed(&self, slug: &str) -> Result<RegistryEntry, HubError> {
        check_slug(slug)?;
        self.entry(slug).await
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

/// Catalogs, installs from GitHub, update checks, updates and rollbacks
/// (ADR 0013). Every network, process and file access runs off the
/// asynchronous runtime.
impl Supervisor {
    /// The repositories of the catalogs the hub reads, in order.
    pub async fn catalogs(&self) -> Result<Vec<String>, HubError> {
        let hub = self.hub_dir().clone();
        blocking(move || remote::read_catalogs(&hub)).await
    }

    /// Adds the catalog of `repo` at the end of the list, after reading it
    /// once from GitHub: a repository without a valid `catalog.json` is not
    /// added. Adding a catalog that is listed already changes nothing.
    /// Returns the list.
    pub async fn add_catalog(&self, repo: &str) -> Result<Vec<String>, HubError> {
        check_repo(repo)?;
        let supervisor = self.clone();
        let repo = repo.to_owned();
        blocking(move || {
            let hub = supervisor.hub_dir();
            if remote::read_catalogs(hub)?.iter().any(|listed| same_repo(listed, &repo)) {
                return remote::read_catalogs(hub);
            }
            remote::fetch_catalog(supervisor.github()?.as_ref(), &repo)?;
            let _lock = hub.lock()?;
            let mut catalogs = remote::read_catalogs(hub)?;
            if !catalogs.iter().any(|listed| same_repo(listed, &repo)) {
                catalogs.push(repo.clone());
                remote::write_catalogs(hub, &catalogs)?;
                tracing::info!(catalog = %repo, "catalog added");
            }
            Ok(catalogs)
        })
        .await
    }

    /// Removes the catalog of `repo`; the default one stays removed. Returns
    /// the list.
    pub async fn remove_catalog(&self, repo: &str) -> Result<Vec<String>, HubError> {
        check_repo(repo)?;
        let supervisor = self.clone();
        let repo = repo.to_owned();
        blocking(move || {
            let hub = supervisor.hub_dir();
            let _lock = hub.lock()?;
            let mut catalogs = remote::read_catalogs(hub)?;
            let count = catalogs.len();
            catalogs.retain(|listed| !same_repo(listed, &repo));
            if catalogs.len() == count {
                return Err(HubError::UnknownCatalog { repo });
            }
            remote::write_catalogs(hub, &catalogs)?;
            tracing::info!(catalog = %repo, "catalog removed");
            Ok(catalogs)
        })
        .await
    }

    /// Every catalog, read from GitHub, with its dashboards and the slug
    /// installed from each. A catalog that cannot be read is listed with the
    /// reason.
    pub async fn catalog_listing(&self) -> Result<Vec<HubCatalog>, HubError> {
        let supervisor = self.clone();
        blocking(move || {
            let hub = supervisor.hub_dir();
            let catalogs = remote::read_catalogs(hub)?;
            if catalogs.is_empty() {
                return Ok(Vec::new());
            }
            let registry = hub.read_registry()?;
            Ok(match supervisor.github() {
                Ok(github) => catalogs
                    .iter()
                    .map(|repo| remote::list_catalog(github.as_ref(), repo, &registry))
                    .collect(),
                Err(error) => {
                    catalogs.iter().map(|repo| remote::unreadable_catalog(repo, &error)).collect()
                }
            })
        })
        .await
    }

    /// Previews a first install from `repo`: downloads the asset of its
    /// latest release for this machine and verifies it, as
    /// [`crate::remote`] describes, without writing outside `staging/`.
    /// The preview waits for [`Supervisor::install`] with its SHA-256.
    ///
    /// Refused when the repository, or a dashboard of the same slug, is
    /// installed already. The key status is `catalog` when a catalog lists
    /// the repository with that key, `unknown` when none lists it, `changed`
    /// when the catalog lists another key, and `accepted` once the user
    /// accepted it ([`Supervisor::accept_key`]).
    ///
    /// With a key that is `unknown` or `changed`, the downloaded executable
    /// is not run: the preview has no manifest, and only
    /// [`Supervisor::accept_key`] completes it.
    pub async fn preview(&self, repo: &str) -> Result<InstallPreview, HubError> {
        check_repo(repo)?;
        let supervisor = self.clone();
        let repo = repo.to_owned();
        blocking(move || {
            let hub = supervisor.hub_dir();
            let registry = hub.read_registry()?;
            // Before any request: an installed repository is updated.
            if let Some(entry) = remote::installed_from(&registry, &repo) {
                return Err(HubError::AlreadyInstalled { repo, slug: entry.slug.clone() });
            }
            let github = supervisor.github()?;
            let catalogs = remote::read_catalogs(hub)?;
            let catalog_key = remote::catalog_key(github.as_ref(), &catalogs, &repo)?;
            let request = StageRequest {
                accepted: supervisor.accepted_key(&repo),
                repo,
                purpose: Purpose::Install { registry, catalog_key },
                target: Target::CURRENT,
                manifest_timeout: supervisor.shared.options.manifest_timeout,
            };
            let area = supervisor.staging_area()?;
            let staged = remote::stage(&area, github.as_ref(), request)?;
            Ok(supervisor.keep_staged(staged))
        })
        .await
    }

    /// Accepts the key of the release previewed for `repo`, which must have
    /// SHA-256 `key_sha256` (see [`InstallPreview::key_sha256`]): trust on
    /// first use, or a key change the user confirms. The key is pinned when
    /// the release is installed; until then it is trusted for this
    /// repository by this hub only.
    ///
    /// Only now is the verified executable run: its staged file is hashed
    /// again, then run for its manifest, which is checked as for a trusted
    /// key. Returns the full preview, `accepted`, that
    /// [`Supervisor::install`] or [`Supervisor::apply_update`] takes. When
    /// the file is gone (`hub_preview_gone`) or fails a check, the preview is
    /// dropped and nothing is accepted. A preview whose key the hub trusts
    /// already is returned as it is.
    pub async fn accept_key(
        &self,
        repo: &str,
        key_sha256: &str,
    ) -> Result<InstallPreview, HubError> {
        check_repo(repo)?;
        check_sha256(key_sha256)?;
        let supervisor = self.clone();
        let (repo, key_sha256) = (repo.to_owned(), key_sha256.to_owned());
        blocking(move || {
            let staged = {
                let mut state = supervisor.remote_state();
                let index = state
                    .staged
                    .iter()
                    .position(|staged| same_repo(&staged.preview.repo, &repo))
                    .ok_or_else(|| HubError::NoPreview { repo: repo.clone() })?;
                let preview = &state.staged[index].preview;
                if preview.key_sha256 != key_sha256 {
                    return Err(HubError::KeyMismatch {
                        repo,
                        expected: preview.key_sha256.clone(),
                        given: key_sha256,
                    });
                }
                if remote::is_trusted(preview.key_status) {
                    return Ok(preview.clone());
                }
                // Taken out while it runs, which can last the manifest
                // timeout, so the state is not locked meanwhile.
                state.staged.remove(index)
            };
            let key = staged.key.clone();
            // A release that fails is dropped here, its file with it.
            let staged = remote::accept_staged(staged, supervisor.shared.options.manifest_timeout)?;
            supervisor.remote_state().accepted.insert(repo.to_ascii_lowercase(), key);
            tracing::info!(repo, key_sha256, "key accepted, release checked");
            Ok(supervisor.keep_staged(staged))
        })
        .await
    }

    /// Installs the release previewed for `repo`, which must have SHA-256
    /// `sha256` (see [`InstallPreview::sha256`]), and returns its slug.
    ///
    /// Refused when nothing was previewed, the SHA-256 differs, the key is
    /// `unknown` or `changed` and was not accepted (so the preview has no
    /// manifest), or the repository or the slug was installed since. The
    /// file is moved into `bin/<slug>/` and recorded with its key pinned,
    /// under the hub directory's lock; the preview is used up either way,
    /// unless it was refused for its SHA-256 or its key.
    pub async fn install(&self, repo: &str, sha256: &str) -> Result<String, HubError> {
        check_repo(repo)?;
        check_sha256(sha256)?;
        let _operation = self.shared.operations.lock().await;
        let supervisor = self.clone();
        let (repo, sha256) = (repo.to_owned(), sha256.to_owned());
        let entry = blocking(move || {
            let staged = supervisor.take_staged(&repo, &sha256)?;
            let installed = remote::install_staged(supervisor.hub_dir(), &staged);
            // Removes what is left of the staged release, here, off the
            // asynchronous runtime.
            drop(staged);
            let entry = installed?;
            supervisor.forget_accepted(&repo);
            tracing::info!(dashboard = %entry.slug, repo, sha256, "installed from GitHub");
            Ok(entry)
        })
        .await?;
        self.processes().failed.remove(&entry.slug);
        Ok(entry.slug)
    }

    /// Checks GitHub for a newer release of each dashboard installed from
    /// it, stores the result in the hub directory, where
    /// [`Supervisor::list`] reads it, and returns it. Only the latest
    /// releases are read; nothing is downloaded. A repository that cannot be
    /// read is reported in the result (it "could not be checked"), not as an
    /// error.
    pub async fn check(&self) -> Result<UpdateCheck, HubError> {
        let supervisor = self.clone();
        blocking(move || {
            let hub = supervisor.hub_dir();
            let registry = hub.read_registry()?;
            let installed: Vec<(&RegistryEntry, &str)> = registry
                .dashboards
                .iter()
                .filter_map(|entry| match &entry.source {
                    Source::Github { repo } => Some((entry, repo.as_str())),
                    Source::Local => None,
                })
                .collect();
            let dashboards = if installed.is_empty() {
                Vec::new()
            } else {
                match supervisor.github() {
                    Ok(github) => installed
                        .iter()
                        .map(|(entry, repo)| remote::check_entry(github.as_ref(), entry, repo))
                        .collect(),
                    Err(error) => installed
                        .iter()
                        .map(|(entry, repo)| remote::could_not_check(entry, repo, &error))
                        .collect(),
                }
            };
            let check = UpdateCheck { checked_at: crate::unix_seconds(), dashboards };
            remote::write_last_check(hub, &check)?;
            Ok(check)
        })
        .await
    }

    /// What the last update check found; `None` before the first one.
    pub async fn last_check(&self) -> Result<Option<UpdateCheck>, HubError> {
        let hub = self.hub_dir().clone();
        blocking(move || remote::read_last_check(&hub)).await
    }

    /// Previews an update of `slug` to the latest release of the repository
    /// it was installed from, as [`Supervisor::preview`] does, with the key
    /// pinned at install. Refused before any download when `slug` is
    /// running or starting, was installed locally, or the release's tag is
    /// not a newer version; after the download, when the executable's
    /// version is not newer than the installed one. The preview waits for
    /// [`Supervisor::apply_update`] with its SHA-256.
    pub async fn preview_update(&self, slug: &str) -> Result<InstallPreview, HubError> {
        check_slug(slug)?;
        let entry = self.entry(slug).await?;
        let Source::Github { repo } = entry.source.clone() else {
            return Err(HubError::NotFromGitHub { slug: slug.to_owned() });
        };
        if self.processes().starting.contains_key(slug) {
            return Err(HubError::Starting(slug.to_owned()));
        }
        if self.live_run(slug).await?.is_some() {
            return Err(HubError::Running(slug.to_owned()));
        }
        let supervisor = self.clone();
        blocking(move || {
            let github = supervisor.github()?;
            let request = StageRequest {
                accepted: supervisor.accepted_key(&repo),
                repo,
                purpose: Purpose::Update { entry: Box::new(entry) },
                target: Target::CURRENT,
                manifest_timeout: supervisor.shared.options.manifest_timeout,
            };
            let area = supervisor.staging_area()?;
            let staged = remote::stage(&area, github.as_ref(), request)?;
            Ok(supervisor.keep_staged(staged))
        })
        .await
    }

    /// Installs the update of `slug` previewed by
    /// [`Supervisor::preview_update`], which must have SHA-256 `sha256`.
    ///
    /// Refused while `slug` runs or starts, when the key changed and was not
    /// accepted, and when the dashboard changed since the preview. The
    /// version it replaces is kept as the previous version, for
    /// [`Supervisor::rollback`]; the older previous version is removed.
    pub async fn apply_update(&self, slug: &str, sha256: &str) -> Result<(), HubError> {
        check_slug(slug)?;
        check_sha256(sha256)?;
        let _operation = self.shared.operations.lock().await;
        let entry = self.entry(slug).await?;
        let Source::Github { repo } = entry.source.clone() else {
            return Err(HubError::NotFromGitHub { slug: slug.to_owned() });
        };
        self.make_idle(slug).await?;
        let supervisor = self.clone();
        let sha256 = sha256.to_owned();
        let entry = blocking(move || {
            let staged = supervisor.take_staged(&repo, &sha256)?;
            let updated = remote::update_staged(supervisor.hub_dir(), &staged);
            drop(staged);
            let entry = updated?;
            supervisor.forget_accepted(&repo);
            Ok(entry)
        })
        .await?;
        self.processes().failed.remove(slug);
        tracing::info!(
            dashboard = %slug,
            version = entry.manifest.version.as_deref().unwrap_or("unknown"),
            "updated from GitHub"
        );
        Ok(())
    }

    /// Updates `slug` to the latest release of its repository in one step:
    /// [`Supervisor::preview_update`], then [`Supervisor::apply_update`] of
    /// that preview. A key change is refused (`hub_key_changed`) until the
    /// user accepts the new key; the preview stays for
    /// [`Supervisor::accept_key`].
    pub async fn update(&self, slug: &str) -> Result<(), HubError> {
        let preview = self.preview_update(slug).await?;
        self.apply_update(slug, &preview.sha256).await
    }

    /// Restores the version the last update of `slug` replaced, after
    /// checking its copy, and removes the newer copy. Refused while `slug`
    /// runs or starts, and when there is no previous version. The restored
    /// version has none: an update installs the newer release again.
    pub async fn rollback(&self, slug: &str) -> Result<(), HubError> {
        check_slug(slug)?;
        let _operation = self.shared.operations.lock().await;
        self.entry(slug).await?;
        self.make_idle(slug).await?;
        let hub = self.hub_dir().clone();
        let name = slug.to_owned();
        let entry = blocking(move || remote::rollback_entry(&hub, &name)).await?;
        self.processes().failed.remove(slug);
        tracing::info!(
            dashboard = %slug,
            version = entry.manifest.version.as_deref().unwrap_or("unknown"),
            "rolled back"
        );
        Ok(())
    }

    /// The hub's settings.
    pub async fn settings(&self) -> Result<HubSettings, HubError> {
        let hub = self.hub_dir().clone();
        blocking(move || remote::read_settings(&hub)).await
    }

    /// Turns the update check at the hub's start on or off. Returns the
    /// settings.
    pub async fn set_check_updates_at_start(&self, on: bool) -> Result<HubSettings, HubError> {
        let supervisor = self.clone();
        blocking(move || {
            let hub = supervisor.hub_dir();
            let _lock = hub.lock()?;
            let mut settings = remote::read_settings(hub)?;
            settings.check_updates_at_start = on;
            remote::write_settings(hub, &settings)?;
            tracing::info!(on, "update check at start set");
            Ok(settings)
        })
        .await
    }

    /// Removes the staging areas left by processes that ended, as a hub does
    /// when it starts; see [`remote::clean_staging`]. A failure is logged.
    pub(crate) async fn clean_staging(&self) {
        let hub = self.hub_dir().clone();
        if let Err(error) = blocking(move || remote::clean_staging(&hub)).await {
            tracing::warn!(%error, "could not clean the staging directory");
        }
    }

    /// Runs [`Supervisor::check`] in the background, unless the setting
    /// `check_updates_at_start` is off or cannot be read. A failure is
    /// logged and never stops the hub.
    pub(crate) fn check_in_background(&self) {
        let supervisor = self.clone();
        tokio::spawn(async move {
            match supervisor.settings().await {
                Ok(settings) if settings.check_updates_at_start => {}
                Ok(_) => {
                    tracing::info!("the update check at start is off");
                    return;
                }
                Err(error) => {
                    tracing::warn!(%error, "could not read the settings; no update check at start");
                    return;
                }
            }
            match supervisor.check().await {
                Ok(check) => {
                    let failed = check.dashboards.iter().filter(|found| found.error.is_some());
                    tracing::info!(
                        checked = check.dashboards.len(),
                        failed = failed.count(),
                        "update check at start done"
                    );
                }
                Err(error) => tracing::warn!(%error, "could not check for updates at start"),
            }
        });
    }

    /// This supervisor's staging area, created at its first call. Blocking:
    /// creating it takes the hub directory's lock.
    fn staging_area(&self) -> Result<Arc<StagingArea>, HubError> {
        // Holds an area or nothing, whatever a holder did.
        let mut slot = self.shared.staging.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(area) = slot.as_ref() {
            return Ok(Arc::clone(area));
        }
        let area = Arc::new(StagingArea::create(self.hub_dir())?);
        *slot = Some(Arc::clone(&area));
        Ok(area)
    }

    /// The key the user accepted for `repo` in this hub.
    fn accepted_key(&self, repo: &str) -> Option<PublicKey> {
        self.remote_state().accepted.get(&repo.to_ascii_lowercase()).cloned()
    }

    /// Forgets the key accepted for `repo`, which an install has pinned.
    fn forget_accepted(&self, repo: &str) {
        self.remote_state().accepted.remove(&repo.to_ascii_lowercase());
    }

    /// Keeps a staged release until it is confirmed, replacing the one of
    /// the same repository and dropping the oldest beyond [`MAX_STAGED`].
    /// Returns its preview. Blocking: removes the files of those dropped.
    fn keep_staged(&self, staged: Staged) -> InstallPreview {
        let preview = staged.preview.clone();
        let dropped = {
            let mut state = self.remote_state();
            let mut dropped = Vec::new();
            if let Some(index) =
                state.staged.iter().position(|kept| same_repo(&kept.preview.repo, &preview.repo))
            {
                dropped.push(state.staged.remove(index));
            }
            while state.staged.len() >= MAX_STAGED {
                dropped.push(state.staged.remove(0));
            }
            state.staged.push(staged);
            dropped
        };
        // Their files are removed here, outside the lock.
        drop(dropped);
        preview
    }

    /// Takes the staged release of `repo` to install it, when its SHA-256 is
    /// `sha256` and its preview is complete: its key trusted and its
    /// manifest read. Otherwise leaves it and refuses.
    fn take_staged(&self, repo: &str, sha256: &str) -> Result<Staged, HubError> {
        let mut state = self.remote_state();
        let index = state
            .staged
            .iter()
            .position(|staged| same_repo(&staged.preview.repo, repo))
            .ok_or_else(|| HubError::NoPreview { repo: repo.to_owned() })?;
        let staged = &state.staged[index];
        if staged.preview.sha256 != sha256 {
            return Err(HubError::PreviewMismatch {
                repo: repo.to_owned(),
                sha256: sha256.to_owned(),
            });
        }
        staged.manifest()?;
        Ok(state.staged.remove(index))
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

fn check_repo(repo: &str) -> Result<(), HubError> {
    if is_repo(repo) { Ok(()) } else { Err(GitHubError::InvalidRepo(repo.to_owned()).into()) }
}

fn check_sha256(sha256: &str) -> Result<(), HubError> {
    if is_sha256(sha256) { Ok(()) } else { Err(HubError::InvalidSha256(sha256.to_owned())) }
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
pub(crate) fn detach(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
}

/// A process group of its own, so that a Ctrl+C in the hub's terminal does
/// not reach the dashboard.
#[cfg(unix)]
pub(crate) fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(any(windows, unix)))]
pub(crate) fn detach(_command: &mut Command) {}

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
/// `--eario-uninstall` does, under the hub directory's lock. Only the hub's
/// own directory for this slug is removed, never a path read from the
/// registry; it goes first, so if it cannot be removed the entry stays and
/// the user can try again.
fn remove_installed(hub: &HubDir, slug: &str) -> Result<(), HubError> {
    let _lock = hub.lock()?;
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
