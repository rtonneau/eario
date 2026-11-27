//! Error type of the facade.

use std::path::PathBuf;
use std::{fmt, io};

use eario_runtime::{AppError, HubDirError, ServeError};

/// Reason a dashboard could not be built or served.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The dashboard definition is inconsistent, or loading its data failed.
    #[error(transparent)]
    App(#[from] AppError),
    /// The local server could not start or stopped abnormally.
    #[error(transparent)]
    Serve(#[from] ServeError),
    /// A file named by the dashboard definition could not be read.
    #[error("could not read {path}: {source}")]
    ReadFile {
        /// The file.
        path: PathBuf,
        /// The underlying failure.
        source: io::Error,
    },
    /// Two computations were registered under the same name.
    #[error("computation `{0}` is registered more than once; give each computation its own name")]
    DuplicateComputation(String),
    /// A `--eario-*` command-line argument could not be carried out.
    #[error(transparent)]
    Launch(#[from] LaunchError),
}

impl Error {
    /// Stable, machine-readable code of the error.
    pub fn code(&self) -> &'static str {
        match self {
            Self::App(error) => error.code(),
            Self::Serve(error) => error.code(),
            Self::ReadFile { .. } => "read_file_failed",
            Self::DuplicateComputation(_) => "duplicate_computation",
            Self::Launch(error) => error.code(),
        }
    }
}

/// Reason a `--eario-*` command-line argument, the launch contract between a
/// dashboard executable and a hub, could not be carried out.
///
/// Its `Debug` form is its message followed by its code: a `main` returning
/// [`Result`] prints the `Debug` form, and the person reading it is the one
/// who typed the command.
#[derive(thiserror::Error)]
pub enum LaunchError {
    /// An argument starts with `--eario-` but is not part of the contract.
    #[error(
        "unknown argument `{0}`; the arguments starting with --eario- are --eario-manifest, --eario-install, --eario-uninstall and --eario-child <run file>"
    )]
    UnknownArgument(String),
    /// Two arguments asking for different actions were given.
    #[error("`{first}` and `{second}` cannot be given together; give one of them")]
    ConflictingArguments {
        /// The argument given first.
        first: String,
        /// The argument given next.
        second: String,
    },
    /// `--eario-child` was given without the path of the run file.
    #[error("--eario-child needs the path of the run file after it")]
    MissingRunFile,
    /// The hub directory or one of its files could not be used.
    #[error(transparent)]
    HubDir(#[from] HubDirError),
    /// The dashboard runs from the hub, so it cannot be installed or removed.
    #[error(
        "`{slug}` is running (run file {run_file}); stop it from the hub or with Quit in its interface, then try again"
    )]
    Running {
        /// The dashboard.
        slug: String,
        /// Its run file.
        run_file: PathBuf,
    },
    /// Whether the dashboard is running could not be checked.
    #[error("could not check whether `{slug}` is running: {source}")]
    Probe {
        /// The dashboard.
        slug: String,
        /// The underlying failure.
        source: io::Error,
    },
    /// The action needs a confirmation, and there is no terminal to ask in.
    #[error(
        "{action} `{slug}` needs a confirmation and standard input is not a terminal; run the command in a terminal, or add --yes"
    )]
    ConfirmationRequired {
        /// What would have been done: `installing` or `removing`.
        action: &'static str,
        /// The dashboard.
        slug: String,
    },
    /// The question could not be asked, or the answer read.
    #[error("could not use the terminal: {0}")]
    Terminal(#[source] io::Error),
    /// The path of the running executable is unknown.
    #[error("could not locate the running executable to install it: {0}")]
    CurrentExecutable(#[source] io::Error),
    /// The dashboard is not in the hub's registry.
    #[error("`{slug}` is not installed in the hub (registry {registry})")]
    NotInstalled {
        /// The dashboard.
        slug: String,
        /// The registry file.
        registry: PathBuf,
    },
    /// The hub's copy of the executable could not be removed.
    #[error("could not remove {path}: {source}; close any program using it, then try again")]
    Remove {
        /// The directory of the copy.
        path: PathBuf,
        /// The underlying failure.
        source: io::Error,
    },
    /// A dashboard of this slug is installed from a GitHub repository, which
    /// the hub updates; an executable on this computer does not replace it.
    #[error(
        "`{slug}` is installed from GitHub {repo}; uninstall it in the hub first, then install this executable again"
    )]
    InstalledFromGitHub {
        /// The dashboard.
        slug: String,
        /// The repository it was installed from, as `owner/repo`.
        repo: String,
    },
}

impl fmt::Debug for LaunchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self} ({})", self.code())
    }
}

impl LaunchError {
    /// Stable, machine-readable code of the error.
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnknownArgument(_) => "launch_unknown_argument",
            Self::ConflictingArguments { .. } => "launch_conflicting_arguments",
            Self::MissingRunFile => "launch_missing_run_file",
            Self::HubDir(error) => error.code(),
            Self::Running { .. } => "launch_dashboard_running",
            Self::Probe { .. } => "launch_probe_failed",
            Self::ConfirmationRequired { .. } => "launch_confirmation_required",
            Self::Terminal(_) => "launch_terminal_failed",
            Self::CurrentExecutable(_) => "launch_current_executable_unknown",
            Self::NotInstalled { .. } => "launch_not_installed",
            Self::Remove { .. } => "launch_remove_failed",
            Self::InstalledFromGitHub { .. } => "launch_installed_from_github",
        }
    }
}

/// Result of building or serving a dashboard.
pub type Result<T, E = Error> = std::result::Result<T, E>;
