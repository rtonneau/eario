//! Reader of delimiter-separated text: CSV and TSV.

use std::io::Cursor;

use polars::frame::DataFrame;
use polars::prelude::{CsvReadOptions, SerReader};

use crate::error::DataError;
use crate::reader::{ImportOptions, TableReader};

/// Reads delimiter-separated text with a header row.
///
/// Column types are inferred by Polars. Empty fields are missing values.
pub(crate) struct DelimitedReader {
    format: &'static str,
    extensions: &'static [&'static str],
    separator: u8,
}

impl DelimitedReader {
    pub(crate) fn csv() -> Self {
        Self { format: "csv", extensions: &["csv"], separator: b',' }
    }

    pub(crate) fn tsv() -> Self {
        Self { format: "tsv", extensions: &["tsv", "tab"], separator: b'\t' }
    }
}

impl TableReader for DelimitedReader {
    fn format(&self) -> &'static str {
        self.format
    }

    fn extensions(&self) -> &'static [&'static str] {
        self.extensions
    }

    fn sheets(&self, _bytes: &[u8]) -> Result<Vec<String>, DataError> {
        Ok(Vec::new())
    }

    fn read(
        &self,
        name: &str,
        bytes: &[u8],
        _options: &ImportOptions,
    ) -> Result<DataFrame, DataError> {
        if bytes.iter().all(u8::is_ascii_whitespace) {
            return Err(DataError::EmptySource { name: name.to_owned() });
        }
        let separator = self.separator;
        CsvReadOptions::default()
            .with_has_header(true)
            .map_parse_options(|options| options.with_separator(separator))
            .into_reader_with_file_handle(Cursor::new(bytes))
            .finish()
            .map_err(|error| DataError::Parse { format: self.format, message: error.to_string() })
    }
}
