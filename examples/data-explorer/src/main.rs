//! A data explorer: import a CSV, TSV or XLSX file, then preview, filter,
//! plot and summarize it.
//!
//! ```text
//! cargo run -p data-explorer                      # start empty, import from the browser
//! cargo run -p data-explorer -- data.csv          # start with a file loaded
//! cargo run -p data-explorer -- --mcp data.csv    # also serve the read-only MCP endpoint
//! ```

use eario::prelude::*;

/// Describes the dashboard. Everything specific to this application is here.
fn dashboard() -> (Dashboard, DatasetId) {
    let data = DatasetId::new("data");
    let x = StateKey::new("x");
    let y = StateKey::new("y");
    let filter = StateKey::new("filter");
    let filtered = DataView::of(&data).filtered_by(&filter);

    let dashboard = Dashboard::new("Data Explorer")
        .page("Explore", |page| {
            page.row([file_import(&data).span(4), dataset_info(&data)])
                .row([
                    numeric_column_select("X axis", &data, &x, 0),
                    numeric_column_select("Y axis", &data, &y, 1),
                    filter_input(&data, &filter).span(6),
                ])
                .row([scatter_chart(filtered.clone(), &x, &y).title("Scatter plot")])
                .row([statistics(filtered.clone())])
        })
        .page("Rows", |page| page.row([data_table(filtered)]));
    (dashboard, data)
}

fn main() -> eario::Result<()> {
    let (mut dashboard, data) = dashboard();
    for argument in std::env::args().skip(1) {
        dashboard = match argument.as_str() {
            "--mcp" => dashboard.mcp(true),
            path => dashboard.load_file(&data, path),
        };
    }
    dashboard.run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashboard_definition_is_valid() {
        let (dashboard, _) = dashboard();
        assert_eq!(dashboard.spec().validate(), Ok(()));
    }
}
