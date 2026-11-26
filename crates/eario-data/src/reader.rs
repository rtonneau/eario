//! Extension point for data formats.

use polars::frame::DataFrame;

use crate::delimited::DelimitedReader;
use crate::error::DataError;
use crate::xlsx::XlsxReader;

/// Settings of one import. Readers ignore the settings they have no use for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportOptions {
    /// Worksheet to read, for formats with worksheets. `None` reads the
    /// first one.
    pub sheet: Option<String>,
}

/// Reads one data format into a table.
///
/// Implement this trait to teach Eario a new tabular format, then add the
/// reader to a [`ReaderRegistry`].
pub trait TableReader: Send + Sync {
    /// Short name of the format, for example `csv`.
    fn format(&self) -> &'static str;

    /// Lower-case file extensions the reader handles, without the dot.
    fn extensions(&self) -> &'static [&'static str];

    /// Lists the worksheets of a source. Formats without worksheets return
    /// an empty list.
    fn sheets(&self, bytes: &[u8]) -> Result<Vec<String>, DataError>;

    /// Reads the source into a table. `name` is only used in error messages.
    fn read(
        &self,
        name: &str,
        bytes: &[u8],
        options: &ImportOptions,
    ) -> Result<DataFrame, DataError>;
}

/// The set of readers an application can import with.
pub struct ReaderRegistry {
    readers: Vec<Box<dyn TableReader>>,
}

impl Default for ReaderRegistry {
    /// A registry holding the built-in CSV, TSV and XLSX readers.
    fn default() -> Self {
        Self {
            readers: vec![
                Box::new(DelimitedReader::csv()),
                Box::new(DelimitedReader::tsv()),
                Box::new(XlsxReader),
            ],
        }
    }
}

impl ReaderRegistry {
    /// Adds a reader. It takes precedence over earlier readers handling the
    /// same extension.
    pub fn register(&mut self, reader: Box<dyn TableReader>) {
        self.readers.insert(0, reader);
    }

    /// Picks the reader for a file name, from its extension.
    pub fn reader_for(&self, name: &str) -> Result<&dyn TableReader, DataError> {
        let extension = name.rsplit_once('.').map(|(_, extension)| extension.to_lowercase());
        let reader = extension.as_deref().and_then(|extension| {
            self.readers.iter().find(|reader| reader.extensions().contains(&extension))
        });
        match reader {
            Some(reader) => Ok(reader.as_ref()),
            None => Err(DataError::UnsupportedFormat {
                name: name.to_owned(),
                supported: self.supported_extensions().join(", "),
            }),
        }
    }

    /// Lists every extension the registry can read.
    pub fn supported_extensions(&self) -> Vec<&'static str> {
        self.readers.iter().flat_map(|reader| reader.extensions().iter().copied()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_is_chosen_from_the_extension_ignoring_case() -> Result<(), DataError> {
        let registry = ReaderRegistry::default();
        assert_eq!(registry.reader_for("data.CSV")?.format(), "csv");
        assert_eq!(registry.reader_for("data.tsv")?.format(), "tsv");
        assert_eq!(registry.reader_for("my.book.xlsx")?.format(), "xlsx");
        Ok(())
    }

    #[test]
    fn unknown_extension_names_the_supported_ones() {
        let registry = ReaderRegistry::default();
        let message = match registry.reader_for("notes.pdf") {
            Ok(_) => String::new(),
            Err(error) => error.to_string(),
        };
        assert!(message.contains("notes.pdf"), "{message}");
        assert!(message.contains("xlsx"), "{message}");
    }

    #[test]
    fn file_without_extension_is_unsupported() {
        let registry = ReaderRegistry::default();
        assert!(matches!(registry.reader_for("README"), Err(DataError::UnsupportedFormat { .. })));
    }
}
