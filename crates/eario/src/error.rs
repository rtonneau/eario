//! Error type of the facade.

use std::io;
use std::path::PathBuf;

use eario_runtime::{AppError, ServeError};

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
}

/// Result of building or serving a dashboard.
pub type Result<T, E = Error> = std::result::Result<T, E>;
