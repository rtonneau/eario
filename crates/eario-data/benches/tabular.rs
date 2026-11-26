//! Benchmarks of the data layer on a generated table.
//!
//! Run with `cargo bench -p eario-data`. Results depend on the machine and
//! are not recorded in the repository.

use std::fmt::Write as _;
use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use eario_core::{ChartMark, Filter, FilterOp, FilterValue};
use eario_data::{DataFrame, Dataset, ReaderRegistry, query};
use rust_xlsxwriter::Workbook;

const CSV_ROWS: u32 = 200_000;
const XLSX_ROWS: u32 = 20_000;
const SAMPLES: [&str; 4] = ["alpha", "beta", "gamma", "delta"];

/// A deterministic measurement: no random generator, so runs are comparable.
fn measurement(row: u32) -> (f64, f64, &'static str) {
    let energy = f64::from(row) * 0.01;
    let counts = (energy.sin() * 100.0).round() + 500.0;
    (energy, counts, SAMPLES[(row % 4) as usize])
}

fn generated_csv(rows: u32) -> String {
    let mut csv = String::from("energy,counts,sample\n");
    for row in 0..rows {
        let (energy, counts, sample) = measurement(row);
        // Writing to a String cannot fail.
        let _ = writeln!(csv, "{energy},{counts},{sample}");
    }
    csv
}

fn write_workbook(rows: u32) -> Result<Vec<u8>, rust_xlsxwriter::XlsxError> {
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write(0, 0, "energy")?;
    sheet.write(0, 1, "counts")?;
    sheet.write(0, 2, "sample")?;
    for row in 0..rows {
        let (energy, counts, sample) = measurement(row);
        sheet.write(row + 1, 0, energy)?;
        sheet.write(row + 1, 1, counts)?;
        sheet.write(row + 1, 2, sample)?;
    }
    workbook.save_to_buffer()
}

fn generated_xlsx(rows: u32) -> Vec<u8> {
    write_workbook(rows)
        .unwrap_or_else(|error| panic!("could not generate the benchmark workbook: {error}"))
}

fn import(registry: &ReaderRegistry, name: &str, bytes: &[u8]) -> DataFrame {
    match Dataset::import(registry, name, bytes) {
        Ok(dataset) => dataset.frame().clone(),
        Err(error) => panic!("could not import {name}: {error}"),
    }
}

fn benchmarks(criterion: &mut Criterion) {
    let registry = ReaderRegistry::default();
    let csv = generated_csv(CSV_ROWS);
    let xlsx = generated_xlsx(XLSX_ROWS);
    let frame = import(&registry, "generated.csv", csv.as_bytes());
    let filter = Filter {
        column: "energy".to_owned(),
        op: FilterOp::Gt,
        value: FilterValue::Number(1000.0),
    };

    criterion.bench_function("csv_import_200k_rows", |bencher| {
        bencher.iter(|| import(&registry, "generated.csv", black_box(csv.as_bytes())));
    });
    criterion.bench_function("xlsx_import_20k_rows", |bencher| {
        bencher.iter(|| import(&registry, "generated.xlsx", black_box(&xlsx)));
    });
    criterion.bench_function("filter_200k_rows", |bencher| {
        bencher.iter(|| query::apply_filter(black_box(&frame), &filter));
    });
    criterion.bench_function("statistics_200k_rows", |bencher| {
        bencher.iter(|| query::statistics(black_box(&frame)));
    });
    criterion.bench_function("chart_data_200k_rows_to_5k_points", |bencher| {
        bencher.iter(|| {
            query::chart_data(black_box(&frame), ChartMark::Scatter, "energy", "counts", 5000)
        });
    });
    criterion.bench_function("table_page_100_rows_to_json", |bencher| {
        bencher.iter(|| {
            query::table_page(black_box(&frame), 1000, 100, CSV_ROWS)
                .map(|page| serde_json::to_string(&page))
        });
    });
}

criterion_group!(benches, benchmarks);
criterion_main!(benches);
