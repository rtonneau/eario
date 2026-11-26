//! A loaded dataset.

use std::sync::Arc;

use eario_core::{DatasetId, DatasetInfo};
use polars::frame::DataFrame;

use crate::error::DataError;
use crate::query;
use crate::reader::{ImportOptions, ReaderRegistry};

/// A table together with where it came from.
///
/// The table is the imported data, unchanged. Filters and other queries
/// derive new tables from it and never modify it.
#[derive(Debug, Clone)]
pub struct Dataset {
    source_name: String,
    format: &'static str,
    source_bytes: u64,
    sheets: Vec<String>,
    selected_sheet: Option<String>,
    /// Raw content, kept only for sources with several worksheets so another
    /// worksheet can be read later.
    raw: Option<Arc<[u8]>>,
    frame: DataFrame,
}

impl Dataset {
    /// Reads a file into a dataset, with the reader matching its extension.
    ///
    /// For workbooks, the first worksheet is loaded.
    pub fn import(registry: &ReaderRegistry, name: &str, bytes: &[u8]) -> Result<Self, DataError> {
        let reader = registry.reader_for(name)?;
        let sheets = reader.sheets(bytes)?;
        let selected_sheet = sheets.first().cloned();
        let options = ImportOptions { sheet: selected_sheet.clone() };
        let frame = reader.read(name, bytes, &options)?;
        check_height(&frame)?;
        let raw = if sheets.len() > 1 { Some(Arc::from(bytes)) } else { None };
        Ok(Self {
            source_name: name.to_owned(),
            format: reader.format(),
            source_bytes: bytes.len() as u64,
            sheets,
            selected_sheet,
            raw,
            frame,
        })
    }

    /// Wraps a table built in memory by the application.
    pub fn from_frame(name: &str, frame: DataFrame) -> Result<Self, DataError> {
        check_height(&frame)?;
        Ok(Self {
            source_name: name.to_owned(),
            format: "memory",
            source_bytes: 0,
            sheets: Vec::new(),
            selected_sheet: None,
            raw: None,
            frame,
        })
    }

    /// Returns the same source with another worksheet loaded.
    pub fn with_sheet(&self, registry: &ReaderRegistry, sheet: &str) -> Result<Self, DataError> {
        if !self.sheets.iter().any(|candidate| candidate == sheet) {
            return Err(if self.sheets.is_empty() {
                DataError::NoSheets { name: self.source_name.clone() }
            } else {
                DataError::UnknownSheet {
                    sheet: sheet.to_owned(),
                    available: self.sheets.join(", "),
                }
            });
        }
        if self.selected_sheet.as_deref() == Some(sheet) {
            return Ok(self.clone());
        }
        let Some(raw) = &self.raw else {
            return Err(DataError::NoSheets { name: self.source_name.clone() });
        };
        let reader = registry.reader_for(&self.source_name)?;
        let options = ImportOptions { sheet: Some(sheet.to_owned()) };
        let frame = reader.read(&self.source_name, raw, &options)?;
        check_height(&frame)?;
        Ok(Self { selected_sheet: Some(sheet.to_owned()), frame, ..self.clone() })
    }

    /// The imported table. Cloning it is cheap: columns are shared.
    pub fn frame(&self) -> &DataFrame {
        &self.frame
    }

    /// Describes the dataset for the slot `id`.
    pub fn info(&self, id: &DatasetId) -> DatasetInfo {
        DatasetInfo {
            id: id.clone(),
            source_name: self.source_name.clone(),
            format: self.format.to_owned(),
            source_bytes: self.source_bytes,
            rows: query::row_count(&self.frame),
            columns: query::column_infos(&self.frame),
            sheets: self.sheets.clone(),
            selected_sheet: self.selected_sheet.clone(),
            derived: None,
        }
    }
}

/// Rejects tables whose row count does not fit the contract types.
fn check_height(frame: &DataFrame) -> Result<(), DataError> {
    match u32::try_from(frame.height()) {
        Ok(_) => Ok(()),
        Err(_) => Err(DataError::TooManyRows),
    }
}
