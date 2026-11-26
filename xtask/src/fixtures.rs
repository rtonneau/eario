//! Generates the synthetic test data of `fixtures/`.
//!
//! The data imitates a small spectroscopy run but is entirely made up. It is
//! computed from the row number alone, so regenerating it changes nothing.

use std::fmt::Write as _;
use std::fs;
use std::io::{BufWriter, Write as _};
use std::path::Path;

use rust_xlsxwriter::{DocProperties, ExcelDateTime, Workbook};

use crate::TaskResult;

const SAMPLES: [&str; 4] = ["Si-ref", "Au-thin", "Ta-backing", "Mylar"];
const ROWS: u32 = 40;

/// One synthetic measurement. `None` stands for a missing value.
struct Measurement {
    sample: Option<&'static str>,
    energy_kev: f64,
    counts: Option<u32>,
    dead_time: f64,
    detector: &'static str,
}

fn measurement(row: u32) -> Measurement {
    let energy_kev = 500.0 + 25.0 * f64::from(row);
    let peak = 1200.0 * (-((energy_kev - 900.0) / 180.0).powi(2)).exp();
    let ripple = f64::from((row * 37) % 23);
    Measurement {
        sample: if row == 12 { None } else { Some(SAMPLES[(row % 4) as usize]) },
        energy_kev,
        // Truncation is intended: counts are whole numbers.
        counts: if row == 7 || row == 19 { None } else { Some((peak + ripple + 40.0) as u32) },
        dead_time: 0.02 + 0.001 * f64::from(row % 5),
        detector: if row.is_multiple_of(2) { "A" } else { "B" },
    }
}

fn delimited(separator: char) -> String {
    let mut text =
        ["sample", "energy_keV", "counts", "dead_time", "detector"].join(&separator.to_string());
    text.push('\n');
    for row in 0..ROWS {
        let measurement = measurement(row);
        let counts = measurement.counts.map(|counts| counts.to_string()).unwrap_or_default();
        // Writing to a String cannot fail.
        let _ = writeln!(
            text,
            "{sample}{separator}{energy}{separator}{counts}{separator}{dead_time}{separator}{detector}",
            sample = measurement.sample.unwrap_or_default(),
            energy = measurement.energy_kev,
            dead_time = measurement.dead_time,
            detector = measurement.detector,
        );
    }
    text
}

fn workbook() -> Result<Workbook, rust_xlsxwriter::XlsxError> {
    let mut workbook = Workbook::new();
    // A fixed creation date keeps the file identical from one run to the next.
    let created = ExcelDateTime::from_ymd(2026, 1, 1)?;
    workbook.set_properties(&DocProperties::new().set_creation_datetime(&created));

    let measurements = workbook.add_worksheet().set_name("Measurements")?;
    for (column, header) in (0..).zip(["sample", "energy_keV", "counts", "dead_time", "detector"]) {
        measurements.write(0, column, header)?;
    }
    for row in 0..ROWS {
        let measurement = measurement(row);
        let line = row + 1;
        if let Some(sample) = measurement.sample {
            measurements.write(line, 0, sample)?;
        }
        measurements.write(line, 1, measurement.energy_kev)?;
        if let Some(counts) = measurement.counts {
            measurements.write(line, 2, counts)?;
        }
        measurements.write(line, 3, measurement.dead_time)?;
        measurements.write(line, 4, measurement.detector)?;
    }

    // A second sheet with a column mixing numbers and text, and a blank header.
    let calibration = workbook.add_worksheet().set_name("Calibration")?;
    calibration.write(0, 0, "channel")?;
    calibration.write(0, 1, "energy_keV")?;
    calibration.write(0, 2, "quality")?;
    calibration.write(0, 4, "checked")?;
    for row in 0..8_u32 {
        let line = row + 1;
        calibration.write(line, 0, 100 + 50 * row)?;
        calibration.write(line, 1, 250.5 + 122.25 * f64::from(row))?;
        if row.is_multiple_of(3) {
            calibration.write(line, 2, "n/a")?;
        } else {
            calibration.write(line, 2, 0.9 + 0.01 * f64::from(row))?;
        }
        calibration.write(line, 3, format!("note {row}"))?;
        calibration.write(line, 4, row.is_multiple_of(2))?;
    }

    workbook.add_worksheet().set_name("Empty")?;
    Ok(workbook)
}

/// Writes every tracked fixture below `directory`.
pub(crate) fn write_all(directory: &Path) -> TaskResult {
    let csv = directory.join("csv");
    let excel = directory.join("excel");
    fs::create_dir_all(&csv)?;
    fs::create_dir_all(&excel)?;

    fs::write(csv.join("measurements.csv"), delimited(','))?;
    fs::write(csv.join("measurements.tsv"), delimited('\t'))?;
    fs::write(csv.join("empty.csv"), "")?;
    // The third line has more fields than the header.
    fs::write(
        csv.join("malformed.csv"),
        "sample,energy_keV,counts\nSi-ref,500,120\nAu-thin,525,130,unexpected,fields\nMylar,550,140\n",
    )?;
    // Not a workbook at all, despite the extension.
    fs::write(excel.join("not-a-workbook.xlsx"), "this is plain text, not a workbook\n")?;
    workbook()?.save(excel.join("samples.xlsx"))?;

    println!("Fixtures written to {}", directory.display());
    Ok(())
}

/// Writes a one-million-row CSV file for manual performance checks.
pub(crate) fn write_large_csv(directory: &Path) -> TaskResult {
    const LARGE_ROWS: u32 = 1_000_000;
    fs::create_dir_all(directory)?;
    let path = directory.join("large.csv");
    let mut file = BufWriter::new(fs::File::create(&path)?);
    writeln!(file, "index,time_s,signal,noise,channel")?;
    for row in 0..LARGE_ROWS {
        let time = f64::from(row) * 0.001;
        let signal = (time * 2.0).sin() * 10.0 + (time * 0.05).cos() * 3.0;
        let noise = f64::from((row.wrapping_mul(2_654_435_761)) % 1000) / 1000.0 - 0.5;
        writeln!(file, "{row},{time:.3},{signal:.5},{noise:.3},ch{}", row % 8)?;
    }
    file.flush()?;
    println!("{LARGE_ROWS} rows written to {}", path.display());
    Ok(())
}
