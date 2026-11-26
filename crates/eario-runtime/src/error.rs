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
}
