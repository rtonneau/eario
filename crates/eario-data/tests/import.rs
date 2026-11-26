//! Import of the files in `fixtures/`, as a user would import them.

use std::fs;
use std::path::PathBuf;

use eario_core::{ColumnKind, DatasetId, DatasetInfo};
use eario_data::{DataError, Dataset, ReaderRegistry, query};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn fixture(relative: &str) -> Result<Vec<u8>, std::io::Error> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures").join(relative);
    fs::read(path)
}

fn import(relative: &str) -> Result<Dataset, Box<dyn std::error::Error>> {
    let name = relative.rsplit('/').next().unwrap_or(relative);
    Ok(Dataset::import(&ReaderRegistry::default(), name, &fixture(relative)?)?)
}

fn import_error(relative: &str) -> Result<DataError, Box<dyn std::error::Error>> {
    let name = relative.rsplit('/').next().unwrap_or(relative);
    match Dataset::import(&ReaderRegistry::default(), name, &fixture(relative)?) {
        Ok(_) => Err(format!("{relative} was imported, an error was expected").into()),
        Err(error) => Ok(error),
    }
}

fn info(dataset: &Dataset) -> DatasetInfo {
    dataset.info(&DatasetId::new("data"))
}

fn kinds(info: &DatasetInfo) -> Vec<(&str, ColumnKind)> {
    info.columns.iter().map(|column| (column.name.as_str(), column.kind)).collect()
}

const MEASUREMENT_COLUMNS: [(&str, ColumnKind); 5] = [
    ("sample", ColumnKind::Text),
    ("energy_keV", ColumnKind::Numeric),
    ("counts", ColumnKind::Numeric),
    ("dead_time", ColumnKind::Numeric),
    ("detector", ColumnKind::Text),
];

#[test]
fn csv_is_imported_with_inferred_types_and_missing_values() -> TestResult {
    let info = info(&import("csv/measurements.csv")?);
    assert_eq!(info.format, "csv");
    assert_eq!(info.rows, 40);
    assert_eq!(kinds(&info), MEASUREMENT_COLUMNS);
    assert!(info.sheets.is_empty());

    let missing: Vec<u32> = info.columns.iter().map(|column| column.null_count).collect();
    assert_eq!(missing, [1, 0, 2, 0, 0]);
    Ok(())
}

#[test]
fn tsv_holds_the_same_table_as_csv() -> TestResult {
    let csv = import("csv/measurements.csv")?;
    let tsv = import("csv/measurements.tsv")?;
    assert_eq!(info(&tsv).format, "tsv");
    assert!(csv.frame().equals_missing(tsv.frame()));
    Ok(())
}

#[test]
fn empty_csv_is_reported_as_empty() -> TestResult {
    let error = import_error("csv/empty.csv")?;
    assert!(matches!(error, DataError::EmptySource { .. }), "{error}");
    Ok(())
}

#[test]
fn csv_with_a_ragged_line_is_a_parse_error() -> TestResult {
    let error = import_error("csv/malformed.csv")?;
    assert!(matches!(error, DataError::Parse { format: "csv", .. }), "{error}");
    Ok(())
}

#[test]
fn workbook_lists_its_sheets_and_loads_the_first() -> TestResult {
    let info = info(&import("excel/samples.xlsx")?);
    assert_eq!(info.format, "xlsx");
    assert_eq!(info.sheets, ["Measurements", "Calibration", "Empty"]);
    assert_eq!(info.selected_sheet.as_deref(), Some("Measurements"));
    assert_eq!(info.rows, 40);
    assert_eq!(kinds(&info), MEASUREMENT_COLUMNS);
    Ok(())
}

#[test]
fn workbook_holds_the_same_table_as_csv() -> TestResult {
    let csv = import("csv/measurements.csv")?;
    let workbook = import("excel/samples.xlsx")?;
    assert!(csv.frame().equals_missing(workbook.frame()));
    Ok(())
}

#[test]
fn workbook_and_csv_agree_on_the_numbers() -> TestResult {
    let from_csv = query::statistics(import("csv/measurements.csv")?.frame())?;
    let from_xlsx = query::statistics(import("excel/samples.xlsx")?.frame())?;
    assert_eq!(from_csv.columns.len(), 3);
    for (csv, xlsx) in from_csv.columns.iter().zip(&from_xlsx.columns) {
        assert_eq!(csv.column, xlsx.column);
        assert_eq!(csv.count, xlsx.count, "{}", csv.column);
        assert_eq!(csv.null_count, xlsx.null_count, "{}", csv.column);
        let difference = (csv.mean.unwrap_or(f64::NAN) - xlsx.mean.unwrap_or(f64::NAN)).abs();
        assert!(difference < 1e-9, "{}: means differ by {difference}", csv.column);
    }
    Ok(())
}

#[test]
fn another_sheet_can_be_selected() -> TestResult {
    let registry = ReaderRegistry::default();
    let workbook = import("excel/samples.xlsx")?;
    let calibration = workbook.with_sheet(&registry, "Calibration")?;
    let info = info(&calibration);
    assert_eq!(info.selected_sheet.as_deref(), Some("Calibration"));
    assert_eq!(info.rows, 8);
    assert_eq!(
        kinds(&info),
        [
            ("channel", ColumnKind::Numeric),
            ("energy_keV", ColumnKind::Numeric),
            // Numbers mixed with "n/a" make a text column.
            ("quality", ColumnKind::Text),
            // The header cell of this column is blank.
            ("column_4", ColumnKind::Text),
            ("checked", ColumnKind::Boolean),
        ]
    );
    // The original selection is untouched.
    assert_eq!(self::info(&workbook).selected_sheet.as_deref(), Some("Measurements"));
    Ok(())
}

#[test]
fn selecting_an_unknown_sheet_names_the_available_ones() -> TestResult {
    let workbook = import("excel/samples.xlsx")?;
    let message = match workbook.with_sheet(&ReaderRegistry::default(), "Results") {
        Ok(_) => String::new(),
        Err(error) => error.to_string(),
    };
    assert!(message.contains("Results") && message.contains("Calibration"), "{message}");
    Ok(())
}

#[test]
fn selecting_an_empty_sheet_is_reported_as_empty() -> TestResult {
    let workbook = import("excel/samples.xlsx")?;
    let result = workbook.with_sheet(&ReaderRegistry::default(), "Empty");
    assert!(matches!(result, Err(DataError::EmptySource { .. })));
    Ok(())
}

#[test]
fn text_file_named_xlsx_is_a_parse_error() -> TestResult {
    let error = import_error("excel/not-a-workbook.xlsx")?;
    assert!(matches!(error, DataError::Parse { format: "xlsx", .. }), "{error}");
    Ok(())
}

#[test]
fn csv_has_no_sheet_to_select() -> TestResult {
    let csv = import("csv/measurements.csv")?;
    let result = csv.with_sheet(&ReaderRegistry::default(), "Sheet1");
    assert!(matches!(result, Err(DataError::NoSheets { .. })));
    Ok(())
}
