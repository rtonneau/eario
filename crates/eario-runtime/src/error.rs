//! Errors of the runtime.

use std::io;
use std::path::PathBuf;

use eario_core::{ComponentId, DatasetId, SpecError, StateKey};
use eario_data::DataError;

/// Reason an application service call failed.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// The dashboard specification is inconsistent.
    #[error("invalid dashboard specification: {0}")]
    Spec(#[from] SpecError),
    /// An import or a query failed.
    #[error(transparent)]
    Data(#[from] DataError),
    /// No component has this identifier.
    #[error("component `{0}` does not exist")]
    UnknownComponent(ComponentId),
    /// No dataset slot has this identifier.
    #[error("dataset `{0}` is not declared by the dashboard")]
    UnknownDataset(DatasetId),
    /// The dataset slot is declared but nothing is loaded into it.
    #[error("no data is loaded into dataset `{0}`")]
    DatasetNotLoaded(DatasetId),
    /// No input component writes this state entry.
    #[error("state `{0}` does not exist")]
    UnknownState(StateKey),
    /// The value is not acceptable for this state entry.
    #[error("invalid value for state `{state}`: {message}")]
    InvalidStateValue {
        /// The state entry.
        state: StateKey,
        /// Why the value was rejected.
        message: String,
    },
    /// User preferences could not be stored.
    #[error("could not store preferences in {path}: {source}")]
    Preferences {
        /// File the preferences are stored in.
        path: PathBuf,
        /// The underlying failure.
        source: io::Error,
    },
    /// A file or a table was sent to a dataset slot that a computation fills.
    #[error("dataset `{0}` is derived by a computation; nothing can be imported or loaded into it")]
    DerivedDataset(DatasetId),
    /// A read that must not compute asked for a derived dataset whose table
    /// is not computed for the current state.
    #[error(
        "derived dataset `{0}` is not computed yet; the dashboard computes it when a component displays it"
    )]
    NotComputed(DatasetId),
    /// A derived dataset names a computation the application did not register.
    #[error(
        "dataset `{dataset}` is derived by computation `{computation}`, which is not registered"
    )]
    UnknownComputation {
        /// The derived dataset.
        dataset: DatasetId,
        /// Name of the missing computation.
        computation: String,
    },
    /// A derived dataset binds an input its computation does not declare.
    #[error(
        "dataset `{dataset}` binds input `{input}`, which computation `{computation}` does not declare"
    )]
    UndeclaredComputationInput {
        /// The derived dataset.
        dataset: DatasetId,
        /// Its computation.
        computation: String,
        /// The input bound without being declared.
        input: String,
    },
    /// A derived dataset leaves an input of its computation unbound.
    #[error("dataset `{dataset}` does not bind input `{input}` of computation `{computation}`")]
    MissingComputationInput {
        /// The derived dataset.
        dataset: DatasetId,
        /// Its computation.
        computation: String,
        /// The declared input nothing is bound to.
        input: String,
    },
    /// A derived dataset binds a parameter its computation does not declare.
    #[error(
        "dataset `{dataset}` binds parameter `{parameter}`, which computation `{computation}` does not declare"
    )]
    UndeclaredComputationParameter {
        /// The derived dataset.
        dataset: DatasetId,
        /// Its computation.
        computation: String,
        /// The parameter bound without being declared.
        parameter: String,
    },
    /// A derived dataset leaves a parameter of its computation unbound.
    #[error(
        "dataset `{dataset}` does not bind parameter `{parameter}` of computation `{computation}`"
    )]
    MissingComputationParameter {
        /// The derived dataset.
        dataset: DatasetId,
        /// Its computation.
        computation: String,
        /// The declared parameter nothing is bound to.
        parameter: String,
    },
    /// A select offers the values of a derived dataset that depends on the
    /// state the select writes, so it could never be given a first value.
    #[error(
        "the select writing state `{state}` offers values of derived dataset `{dataset}`, which depends on `{state}` itself; offer the values of a dataset that does not read `{state}`, or bind the computation to another state"
    )]
    SelectDependsOnItsState {
        /// The state the select writes.
        state: StateKey,
        /// The derived dataset whose values it offers.
        dataset: DatasetId,
    },
    /// A computation returned an error or panicked. Nothing was cached, and
    /// the application keeps running.
    #[error("computation `{name}` failed: {message}")]
    Computation {
        /// Name of the computation.
        name: String,
        /// What went wrong.
        message: String,
    },
}

impl AppError {
    /// Stable, machine-readable code of the error.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Spec(_) => "invalid_spec",
            Self::Data(error) => error.code(),
            Self::UnknownComponent(_) => "unknown_component",
            Self::UnknownDataset(_) => "unknown_dataset",
            Self::DatasetNotLoaded(_) => "dataset_not_loaded",
            Self::UnknownState(_) => "unknown_state",
            Self::InvalidStateValue { .. } => "invalid_state_value",
            Self::Preferences { .. } => "preferences_not_stored",
            Self::DerivedDataset(_) => "derived_dataset",
            Self::NotComputed(_) => "dataset_not_computed",
            Self::UnknownComputation { .. } => "unknown_computation",
            Self::UndeclaredComputationInput { .. } => "undeclared_computation_input",
            Self::MissingComputationInput { .. } => "missing_computation_input",
            Self::UndeclaredComputationParameter { .. } => "undeclared_computation_parameter",
            Self::MissingComputationParameter { .. } => "missing_computation_parameter",
            Self::SelectDependsOnItsState { .. } => "select_depends_on_its_state",
            Self::Computation { .. } => "computation_failed",
        }
    }
}

/// Reason the local server could not start or stopped abnormally.
#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    /// The asynchronous runtime could not be created.
    #[error("could not start the asynchronous runtime: {0}")]
    Runtime(#[source] io::Error),
    /// The loopback address could not be bound.
    #[error("could not listen on 127.0.0.1:{port}: {source}")]
    Bind {
        /// Requested port; 0 asks the system for a free one.
        port: u16,
        /// The underlying failure.
        source: io::Error,
    },
    /// The operating system did not provide random bytes for the session token.
    #[error("could not generate the session token: {0}")]
    Token(String),
    /// The server stopped on an input/output error.
    #[error("the server failed: {0}")]
    Serve(#[source] io::Error),
    /// The server task ended abnormally.
    #[error("the server task ended abnormally: {0}")]
    Task(String),
    /// The log file of a dashboard started by a hub could not be opened.
    #[error("could not open the log file {path}: {source}")]
    LogFile {
        /// The log file.
        path: PathBuf,
        /// The underlying failure.
        source: io::Error,
    },
    /// The run file of a dashboard started by a hub could not be written or
    /// removed.
    #[error(transparent)]
    RunFile(#[from] HubDirError),
}

impl ServeError {
    /// Stable, machine-readable code of the error.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Runtime(_)
            | Self::Bind { .. }
            | Self::Token(_)
            | Self::Serve(_)
            | Self::Task(_) => "serve_failed",
            Self::LogFile { .. } => "log_file_failed",
            Self::RunFile(error) => error.code(),
        }
    }
}

/// Reason a file of the hub directory could not be read, written or checked.
///
/// Every variant names the file or directory concerned.
#[derive(Debug, thiserror::Error)]
pub enum HubDirError {
    /// The platform reports no per-user configuration directory and
    /// `EARIO_HUB_DIR` is not set.
    #[error("no configuration directory is available; set EARIO_HUB_DIR to the hub directory")]
    NoConfigDir,
    /// A file or directory could not be read, written, renamed or removed.
    #[error("could not access {path}: {source}")]
    Io {
        /// The file or directory.
        path: PathBuf,
        /// The underlying failure.
        source: io::Error,
    },
    /// A file does not hold what it should.
    #[error("{path} is not valid: {message}")]
    Invalid {
        /// The file.
        path: PathBuf,
        /// What is wrong with it.
        message: String,
    },
    /// The registry was written in a format this version cannot read.
    #[error(
        "{path} has format version {found}, but this version of Eario reads up to {supported}; update Eario or restore the file"
    )]
    UnknownRegistryVersion {
        /// The registry file.
        path: PathBuf,
        /// Version found in the file.
        found: u32,
        /// Newest version this build reads.
        supported: u32,
    },
    /// A name that is not a slug was used where a file name is derived.
    #[error("`{0}` is not a valid dashboard slug")]
    InvalidSlug(String),
}

impl HubDirError {
    /// Stable, machine-readable code of the error.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NoConfigDir => "hub_dir_unavailable",
            Self::Io { .. } => "hub_file_io",
            Self::Invalid { .. } => "hub_file_invalid",
            Self::UnknownRegistryVersion { .. } => "hub_registry_unknown_version",
            Self::InvalidSlug(_) => "hub_invalid_slug",
        }
    }
}
