//! Signal lab: custom Rust logic in a dashboard.
//!
//! Two pages, each driven by computations written in Rust with Polars:
//!
//! - **Signal analysis**: a moving average of a time series, whose window a
//!   slider sets, and a histogram of the smoothed signal, whose bin count a
//!   number input sets. The histogram is computed from the smoothed dataset,
//!   so it follows both controls.
//! - **Group summary**: per-dose aggregates of the category a select chooses,
//!   with outliers optionally left out.
//!
//! ```text
//! cargo xtask fixtures                                   # once, writes fixtures/
//! cargo run -p signal-lab -- fixtures/csv/time-series.csv fixtures/csv/groups.csv
//! ```
//!
//! The first file is the time series (columns `time` and `signal`), the second
//! the groups (columns `category`, `dose` and `value`). Either can also be
//! imported from the browser.

use eario::polars::prelude::*;
use eario::prelude::*;

/// Largest distance from the mean of its dose, in standard deviations, a
/// value may have and still count when outliers are left out.
const OUTLIER_LIMIT: f64 = 2.5;

/// Adds `smoothed`, the mean of the `window` latest samples of `signal`
/// (fewer at the start), to the table `series`.
fn moving_average() -> Computation {
    let window = State::<u32>::new("window");
    Computation::new("moving_average", move |ctx| {
        let window = ctx.param(&window)? as usize;
        if window == 0 {
            return Err(ComputeError::new("the window must be at least 1"));
        }
        let mut frame = ctx.input("series")?.clone();
        let signal = numbers(&frame, "signal")?;
        let mut sum = 0.0;
        let mut smoothed = Vec::with_capacity(signal.len());
        for (index, value) in signal.iter().enumerate() {
            sum += value;
            if index >= window {
                sum -= signal[index - window];
            }
            smoothed.push(sum / (index + 1).min(window) as f64);
        }
        frame.with_column(Column::new("smoothed".into(), smoothed))?;
        Ok(frame)
    })
    .input("series")
    .param("window")
    .describe("The series with `smoothed`, the mean of the last `window` values of `signal`.")
}

/// Counts the values of `smoothed` in `bins` equal-width bins.
fn histogram() -> Computation {
    let bins = State::<u32>::new("bins");
    Computation::new("histogram", move |ctx| {
        let bins = ctx.param(&bins)?;
        if bins == 0 {
            return Err(ComputeError::new("there must be at least 1 bin"));
        }
        let values = numbers(ctx.input("smoothed")?, "smoothed")?;
        if values.is_empty() {
            return Err(ComputeError::new("the smoothed series has no value"));
        }
        let low = values.iter().copied().fold(f64::INFINITY, f64::min);
        let high = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let width = (high - low) / f64::from(bins);
        let mut counts = vec![0_u32; bins as usize];
        for value in &values {
            // Truncation is intended: the bin of a value is its whole part.
            let bin = if width > 0.0 { ((value - low) / width) as usize } else { 0 };
            // The largest value sits on the upper edge: it belongs to the last bin.
            counts[bin.min(bins as usize - 1)] += 1;
        }
        let start: Vec<f64> = (0..bins).map(|bin| low + width * f64::from(bin)).collect();
        let center: Vec<f64> = start.iter().map(|start| start + width / 2.0).collect();
        Ok(df!("bin_start" => start, "bin_center" => center, "count" => counts)?)
    })
    .input("smoothed")
    .param("bins")
    .describe("The number of values of `smoothed` in each of `bins` equal-width bins.")
}

/// Aggregates `value` per `dose` within one `category`, leaving out the
/// values far from the mean of their dose when asked.
fn group_summary() -> Computation {
    let category = State::<String>::new("category");
    let exclude = State::<bool>::new("exclude_outliers");
    Computation::new("group_summary", move |ctx| {
        let category = ctx.param(&category)?;
        let exclude = ctx.param(&exclude)?;
        let rows = ctx.input("table")?.clone().lazy().filter(col("category").eq(lit(category)));
        let rows = if exclude {
            let offset = col("value") - col("value").mean().over([col("dose")])?;
            let spread = col("value").std(1).over([col("dose")])? * lit(OUTLIER_LIMIT);
            rows.filter((offset.clone() * offset).lt_eq(spread.clone() * spread))
        } else {
            rows
        };
        let summary = rows
            .group_by([col("dose")])
            .agg([
                col("value").count().alias("count"),
                col("value").mean().alias("mean"),
                col("value").min().alias("min"),
                col("value").max().alias("max"),
            ])
            .sort(["dose"], SortMultipleOptions::default())
            .collect()?;
        Ok(summary)
    })
    .input("table")
    .param("category")
    .param("exclude_outliers")
    .describe("Count, mean, minimum and maximum of `value` per `dose` in one `category`.")
}

/// The values of a numeric column as `f64`, with missing values rejected.
fn numbers(frame: &DataFrame, name: &str) -> Result<Vec<f64>, ComputeError> {
    let column = frame
        .column(name)
        .map_err(|_| ComputeError::new(format!("the table has no column `{name}`")))?
        .cast(&DataType::Float64)
        .map_err(|_| ComputeError::new(format!("column `{name}` is not numeric")))?;
    column
        .f64()?
        .iter()
        .map(|value| {
            value.ok_or_else(|| ComputeError::new(format!("column `{name}` has a missing value")))
        })
        .collect()
}

/// Describes the dashboard. Everything specific to this application is here.
fn dashboard() -> (Dashboard, DatasetId, DatasetId) {
    let series = DatasetId::new("series");
    let smoothed = DatasetId::new("smoothed");
    let histogram_data = DatasetId::new("histogram");
    let groups = DatasetId::new("groups");
    let summary = DatasetId::new("summary");

    let window = State::<u32>::new("window");
    let bins = State::<u32>::new("bins");
    let category = State::<String>::new("category");
    let exclude = State::<bool>::new("exclude_outliers");

    let (time_axis, smoothed_axis) = (State::new("time_axis"), State::new("smoothed_axis"));
    let (bin_axis, count_axis) = (State::new("bin_axis"), State::new("count_axis"));
    let (dose_axis, mean_axis) = (State::new("dose_axis"), State::new("mean_axis"));

    let dashboard = Dashboard::new("Signal Lab")
        .computation(moving_average())
        .computation(histogram())
        .computation(group_summary())
        .derived(&smoothed, "moving_average", |derived| {
            derived.input("series", &series).param("window", &window)
        })
        .derived(&histogram_data, "histogram", |derived| {
            derived.input("smoothed", &smoothed).param("bins", &bins)
        })
        .derived(&summary, "group_summary", |derived| {
            derived
                .input("table", &groups)
                .param("category", &category)
                .param("exclude_outliers", &exclude)
        })
        .page("Signal analysis", |page| {
            page.row([
                file_import(&series).span(4),
                slider("Moving average window", &window, 1..=50, 5).span(4),
                number_input("Histogram bins", &bins, 1..=200, 20).span(4),
            ])
            .row([
                numeric_column_select("X axis", &smoothed, &time_axis, 0),
                numeric_column_select("Y axis", &smoothed, &smoothed_axis, 2),
            ])
            .row([line_chart(&smoothed, &time_axis, &smoothed_axis).title("Smoothed signal")])
            .row([
                numeric_column_select("X axis", &histogram_data, &bin_axis, 1),
                numeric_column_select("Y axis", &histogram_data, &count_axis, 2),
            ])
            .row([line_chart(&histogram_data, &bin_axis, &count_axis).title("Histogram")])
        })
        .page("Group summary", |page| {
            page.row([
                file_import(&groups).span(4),
                select_from_column("Category", &groups, "category", &category).span(4),
                checkbox("Leave out outliers", &exclude, false).span(4),
            ])
            .row([data_table(&summary).title("Per-dose aggregates")])
            .row([
                numeric_column_select("X axis", &summary, &dose_axis, 0),
                numeric_column_select("Y axis", &summary, &mean_axis, 2),
            ])
            .row([line_chart(&summary, &dose_axis, &mean_axis).title("Mean per dose")])
        });
    (dashboard, series, groups)
}

fn main() -> eario::Result<()> {
    let (mut dashboard, series, groups) = dashboard();
    for (dataset, path) in [&series, &groups].into_iter().zip(std::env::args().skip(1)) {
        dashboard = dashboard.load_file(dataset, path);
    }
    dashboard.run()
}

#[cfg(test)]
mod tests {
    use eario::data::ComputeContext;
    use serde_json::{Value, json};

    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    /// Runs `computation` on `inputs`, with the parameters it declares given
    /// by name.
    fn run(
        computation: &Computation,
        inputs: &[(&str, DataFrame)],
        params: &[(&str, Value)],
    ) -> Result<DataFrame, ComputeError> {
        let inputs = inputs.iter().map(|(name, frame)| ((*name).to_owned(), frame.clone()));
        let params = params.iter().map(|(name, value)| ((*name).to_owned(), value.clone()));
        let mut context = ComputeContext::new(inputs.collect(), params.collect());
        // Bind each state entry to the parameter of the same name, as a
        // derived dataset of the dashboard does.
        for name in ["window", "bins", "category", "exclude_outliers"] {
            context = context.bind_state(StateKey::new(name), name);
        }
        computation.run(&context)
    }

    fn floats(frame: &DataFrame, name: &str) -> Result<Vec<f64>, Box<dyn std::error::Error>> {
        Ok(numbers(frame, name)?)
    }

    fn names(frame: &DataFrame) -> Vec<String> {
        frame.get_column_names().into_iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn dashboard_definition_is_valid() {
        let (dashboard, _, _) = dashboard();
        assert_eq!(dashboard.spec().validate(), Ok(()));
    }

    #[test]
    fn dashboard_builds_into_an_application() {
        let (dashboard, _, _) = dashboard();
        assert!(dashboard.build().is_ok());
    }

    #[test]
    fn moving_average_adds_the_smoothed_column() -> TestResult {
        let series = df!("time" => [0.0, 1.0, 2.0, 3.0], "signal" => [1.0, 3.0, 5.0, 11.0])?;
        let result = run(&moving_average(), &[("series", series)], &[("window", json!(2))])?;
        assert_eq!(names(&result), ["time", "signal", "smoothed"]);
        assert_eq!(floats(&result, "smoothed")?, [1.0, 2.0, 4.0, 8.0]);
        Ok(())
    }

    #[test]
    fn moving_average_rejects_an_empty_window() -> TestResult {
        let series = df!("time" => [0.0], "signal" => [1.0])?;
        let result = run(&moving_average(), &[("series", series)], &[("window", json!(0))]);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn histogram_counts_values_per_bin() -> TestResult {
        let smoothed = df!("smoothed" => [0.0, 1.0, 1.5, 2.0, 4.0])?;
        let result = run(&histogram(), &[("smoothed", smoothed)], &[("bins", json!(4))])?;
        assert_eq!(names(&result), ["bin_start", "bin_center", "count"]);
        assert_eq!(floats(&result, "bin_start")?, [0.0, 1.0, 2.0, 3.0]);
        assert_eq!(floats(&result, "bin_center")?, [0.5, 1.5, 2.5, 3.5]);
        assert_eq!(floats(&result, "count")?, [1.0, 2.0, 1.0, 1.0]);
        Ok(())
    }

    #[test]
    fn histogram_of_a_constant_series_fills_one_bin() -> TestResult {
        let smoothed = df!("smoothed" => [3.0, 3.0, 3.0])?;
        let result = run(&histogram(), &[("smoothed", smoothed)], &[("bins", json!(3))])?;
        assert_eq!(floats(&result, "count")?, [3.0, 0.0, 0.0]);
        Ok(())
    }

    /// Eleven values of category `a` at dose 1, the last an outlier, and two
    /// of category `b`.
    fn groups_table() -> PolarsResult<DataFrame> {
        let values = [10.0, 10.2, 9.8, 10.1, 9.9, 10.0, 10.3, 9.7, 10.0, 10.1, 60.0];
        let mut table = df!(
            "category" => ["a"; 11],
            "dose" => [1_i64; 11],
            "value" => values,
        )?;
        table.vstack_mut(&df!(
            "category" => ["b", "b"],
            "dose" => [1_i64, 1],
            "value" => [100.0, 200.0],
        )?)?;
        Ok(table)
    }

    #[test]
    fn group_summary_aggregates_the_chosen_category() -> TestResult {
        let params = [("category", json!("a")), ("exclude_outliers", json!(false))];
        let result = run(&group_summary(), &[("table", groups_table()?)], &params)?;
        assert_eq!(names(&result), ["dose", "count", "mean", "min", "max"]);
        assert_eq!(result.height(), 1);
        assert_eq!(floats(&result, "count")?, [11.0]);
        assert_eq!(floats(&result, "max")?, [60.0]);
        assert_eq!(floats(&result, "min")?, [9.7]);
        let mean = floats(&result, "mean")?[0];
        assert!((mean - 14.554_545).abs() < 1e-5, "mean was {mean}");
        Ok(())
    }

    #[test]
    fn group_summary_can_leave_out_outliers() -> TestResult {
        let params = [("category", json!("a")), ("exclude_outliers", json!(true))];
        let result = run(&group_summary(), &[("table", groups_table()?)], &params)?;
        assert_eq!(floats(&result, "count")?, [10.0]);
        assert_eq!(floats(&result, "max")?, [10.3]);
        let mean = floats(&result, "mean")?[0];
        assert!((mean - 10.01).abs() < 1e-9, "mean was {mean}");
        Ok(())
    }

    #[test]
    fn derived_datasets_chain_in_a_built_application() -> TestResult {
        let (dashboard, series, _) = dashboard();
        let (app, _) = dashboard.build()?;
        let text = "time,signal\n0,1\n1,3\n2,5\n3,11\n";
        app.import_dataset(&series, "series.csv", text.as_bytes())?;
        let histogram = app.dataset_frame(&DatasetId::new("histogram"))?;
        assert_eq!(histogram.height(), 20);
        Ok(())
    }
}
