//! Reader of Excel workbooks (XLSX).

use std::collections::BTreeSet;
use std::io::Cursor;

use calamine::{Data, Reader, Xlsx, open_workbook_from_rs};
use polars::frame::DataFrame;
use polars::prelude::Column;

use crate::error::DataError;
use crate::reader::{ImportOptions, TableReader};

const FORMAT: &str = "xlsx";

/// Reads one worksheet of an XLSX workbook.
///
/// The first used row is the header. Each column gets the narrowest type
/// holding all its cells: integer, floating point, boolean, otherwise text.
/// Excel stores every number as a float, so a column is an integer column
/// when all its numbers are whole. Empty cells and cells holding an Excel
/// error are missing values. Date cells are read as Excel serial numbers.
pub(crate) struct XlsxReader;

fn parse_error(error: impl std::fmt::Display) -> DataError {
    DataError::Parse { format: FORMAT, message: error.to_string() }
}

impl TableReader for XlsxReader {
    fn format(&self) -> &'static str {
        FORMAT
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["xlsx", "xlsm"]
    }

    fn sheets(&self, bytes: &[u8]) -> Result<Vec<String>, DataError> {
        let workbook: Xlsx<_> = open_workbook_from_rs(Cursor::new(bytes)).map_err(parse_error)?;
        Ok(workbook.sheet_names())
    }

    fn read(
        &self,
        name: &str,
        bytes: &[u8],
        options: &ImportOptions,
    ) -> Result<DataFrame, DataError> {
        let mut workbook: Xlsx<_> =
            open_workbook_from_rs(Cursor::new(bytes)).map_err(parse_error)?;
        let sheets = workbook.sheet_names();
        let sheet = match &options.sheet {
            Some(sheet) if sheets.contains(sheet) => sheet.clone(),
            Some(sheet) => {
                return Err(DataError::UnknownSheet {
                    sheet: sheet.clone(),
                    available: sheets.join(", "),
                });
            }
            None => match sheets.first() {
                Some(first) => first.clone(),
                None => return Err(DataError::EmptySource { name: name.to_owned() }),
            },
        };

        let range = workbook.worksheet_range(&sheet).map_err(parse_error)?;
        let mut rows = range.rows();
        let Some(header) = rows.next() else {
            return Err(DataError::EmptySource { name: format!("{name} [{sheet}]") });
        };
        let names = column_names(header);

        let mut cells_by_column: Vec<Vec<&Data>> = vec![Vec::new(); names.len()];
        let mut height = 0;
        for row in rows {
            for (cells, cell) in cells_by_column.iter_mut().zip(row) {
                cells.push(cell);
            }
            height += 1;
        }

        let columns =
            names.iter().zip(&cells_by_column).map(|(name, cells)| column(name, cells)).collect();
        Ok(DataFrame::new(height, columns)?)
    }
}

/// Derives unique, non-empty column names from the header row.
fn column_names(header: &[Data]) -> Vec<String> {
    let mut used = BTreeSet::new();
    let mut names = Vec::with_capacity(header.len());
    for (index, cell) in header.iter().enumerate() {
        let text = cell.to_string();
        let base = match text.trim() {
            "" => format!("column_{}", index + 1),
            trimmed => trimmed.to_owned(),
        };
        let mut name = base.clone();
        let mut suffix = 2;
        while !used.insert(name.clone()) {
            name = format!("{base}_{suffix}");
            suffix += 1;
        }
        names.push(name);
    }
    names
}

/// The narrowest type holding every non-empty cell of a column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CellType {
    Integer,
    Float,
    Boolean,
    Text,
}

fn is_missing(cell: &Data) -> bool {
    matches!(cell, Data::Empty | Data::Error(_))
}

/// Largest magnitude below which every whole `f64` is an exact integer.
const MAX_EXACT_INTEGER: f64 = 9_007_199_254_740_992.0;

/// Converts a float holding a whole number to that integer.
fn whole_number(value: f64) -> Option<i64> {
    // The bound makes the conversion exact.
    (value.fract() == 0.0 && value.abs() < MAX_EXACT_INTEGER).then_some(value as i64)
}

fn infer_type(cells: &[&Data]) -> CellType {
    let mut present = cells.iter().copied().filter(|cell| !is_missing(cell)).peekable();
    if present.peek().is_none() {
        return CellType::Text;
    }
    let mut inferred = None;
    for cell in present {
        let cell_type = match cell {
            Data::Int(_) => CellType::Integer,
            Data::Float(value) if whole_number(*value).is_some() => CellType::Integer,
            Data::Float(_) | Data::DateTime(_) => CellType::Float,
            Data::Bool(_) => CellType::Boolean,
            _ => CellType::Text,
        };
        inferred = Some(match (inferred, cell_type) {
            (None, cell_type) => cell_type,
            (Some(current), cell_type) if current == cell_type => current,
            (Some(CellType::Integer), CellType::Float)
            | (Some(CellType::Float), CellType::Integer) => CellType::Float,
            _ => return CellType::Text,
        });
    }
    inferred.unwrap_or(CellType::Text)
}

fn column(name: &str, cells: &[&Data]) -> Column {
    let name = name.into();
    match infer_type(cells) {
        CellType::Integer => {
            let values: Vec<Option<i64>> = cells
                .iter()
                .map(|cell| match cell {
                    Data::Int(value) => Some(*value),
                    Data::Float(value) => whole_number(*value),
                    _ => None,
                })
                .collect();
            Column::new(name, values)
        }
        CellType::Float => {
            let values: Vec<Option<f64>> = cells
                .iter()
                .map(|cell| match cell {
                    // Workbook integers are far below 2^53, where f64 is exact.
                    Data::Int(value) => Some(*value as f64),
                    Data::Float(value) => Some(*value),
                    Data::DateTime(value) => Some(value.as_f64()),
                    _ => None,
                })
                .collect();
            Column::new(name, values)
        }
        CellType::Boolean => {
            let values: Vec<Option<bool>> = cells
                .iter()
                .map(|cell| match cell {
                    Data::Bool(value) => Some(*value),
                    _ => None,
                })
                .collect();
            Column::new(name, values)
        }
        CellType::Text => {
            let values: Vec<Option<String>> = cells
                .iter()
                .map(|cell| if is_missing(cell) { None } else { Some(cell.to_string()) })
                .collect();
            Column::new(name, values)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_and_repeated_headers_get_unique_names() {
        let header = [
            Data::String("energy".to_owned()),
            Data::Empty,
            Data::String("energy".to_owned()),
            Data::String(" energy ".to_owned()),
        ];
        assert_eq!(column_names(&header), ["energy", "column_2", "energy_2", "energy_3"]);
    }

    #[test]
    fn integers_and_floats_together_are_floats() {
        let cells = [&Data::Float(1.0), &Data::Empty, &Data::Float(2.5)];
        assert_eq!(infer_type(&cells), CellType::Float);
    }

    #[test]
    fn whole_floats_are_integers() {
        let cells = [&Data::Float(500.0), &Data::Empty, &Data::Float(-3.0)];
        assert_eq!(infer_type(&cells), CellType::Integer);
        assert_eq!(whole_number(1e300), None);
        assert_eq!(whole_number(f64::NAN), None);
    }

    #[test]
    fn numbers_and_text_together_are_text() {
        let text = Data::String("n/a".to_owned());
        let cells = [&Data::Int(1), &text];
        assert_eq!(infer_type(&cells), CellType::Text);
    }

    #[test]
    fn column_of_empty_cells_is_text() {
        let cells = [&Data::Empty, &Data::Empty];
        assert_eq!(infer_type(&cells), CellType::Text);
    }
}
