//! Errors of the data layer.

use polars::error::PolarsError;

/// Reason an import or a query failed.
///
/// Messages are written for the person using the dashboard: they name the
/// file, worksheet or column at fault.
#[derive(Debug, thiserror::Error)]
pub enum DataError {
    /// No reader is registered for the extension of the file.
    #[error("`{name}` has an unsupported format; supported extensions: {supported}")]
    UnsupportedFormat {
        /// Name of the rejected file.
        name: String,
        /// Comma-separated list of the extensions that can be read.
        supported: String,
    },
    /// The source holds no table: no header row, or no worksheet.
    #[error("`{name}` contains no data")]
    EmptySource {
        /// Name of the empty source.
        name: String,
    },
    /// The content of the file could not be read as its format.
    #[error("could not read {format} data: {message}")]
    Parse {
        /// Format the file was read as.
        format: &'static str,
        /// What went wrong, as reported by the parser.
        message: String,
    },
    /// The requested worksheet does not exist.
    #[error("worksheet `{sheet}` does not exist; available worksheets: {available}")]
    UnknownSheet {
        /// The requested worksheet.
        sheet: String,
        /// Comma-separated list of the worksheets of the file.
        available: String,
    },
    /// The source has no worksheets to choose from.
    #[error("`{name}` has no worksheets")]
    NoSheets {
        /// Name of the source.
        name: String,
    },
    /// The requested column does not exist.
    #[error("column `{0}` does not exist")]
    UnknownColumn(String),
    /// A numeric column was required.
    #[error("column `{0}` is not numeric")]
    NotNumeric(String),
    /// The filter cannot be applied to its column.
    #[error("invalid filter: {0}")]
    InvalidFilter(String),
    /// The table has more rows than the engine indexes.
    #[error("the table has too many rows")]
    TooManyRows,
    /// The data engine failed.
    #[error("data engine error: {0}")]
    Engine(#[from] PolarsError),
}

impl DataError {
    /// Stable, machine-readable code of the error.
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnsupportedFormat { .. } => "unsupported_format",
            Self::EmptySource { .. } => "empty_source",
            Self::Parse { .. } => "parse_error",
            Self::UnknownSheet { .. } => "unknown_sheet",
            Self::NoSheets { .. } => "no_sheets",
            Self::UnknownColumn(_) => "unknown_column",
            Self::NotNumeric(_) => "not_numeric",
            Self::InvalidFilter(_) => "invalid_filter",
            Self::TooManyRows => "too_many_rows",
            Self::Engine(_) => "engine_error",
        }
    }
}
