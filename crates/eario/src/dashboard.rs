//! Builder of a dashboard application.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use eario_core::{
    ComponentId, ComponentKind, ComponentSpec, DashboardSpec, DatasetId, DatasetSlot, Dependency,
    PageSpec, RowSpec, SPEC_VERSION,
};
use eario_runtime::{App, AppOptions, ServerConfig, preferences};

use crate::error::{Error, Result};
use crate::widgets::Widget;

/// Number of columns of the layout grid.
const GRID_COLUMNS: u8 = 12;

/// One page of a dashboard under construction.
#[derive(Debug, Clone)]
pub struct Page {
    title: String,
    rows: Vec<Vec<Widget>>,
}

impl Page {
    fn new(title: String) -> Self {
        Self { title, rows: Vec::new() }
    }

    /// Adds a row of components, laid out from left to right.
    pub fn row(mut self, widgets: impl IntoIterator<Item = Widget>) -> Self {
        self.rows.push(widgets.into_iter().collect());
        self
    }
}

/// A dashboard application under construction.
///
/// Describe the pages, then call [`Dashboard::run`].
pub struct Dashboard {
    title: String,
    pages: Vec<Page>,
    files: Vec<(DatasetId, PathBuf)>,
    config: ServerConfig,
    remember_preferences: bool,
    #[cfg(feature = "mcp")]
    mcp: bool,
}

impl Dashboard {
    /// Starts a dashboard with the given title.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            pages: Vec::new(),
            files: Vec::new(),
            config: ServerConfig::default(),
            remember_preferences: true,
            #[cfg(feature = "mcp")]
            mcp: false,
        }
    }

    /// Adds a page. `build` receives the empty page and returns it filled.
    pub fn page(mut self, title: impl Into<String>, build: impl FnOnce(Page) -> Page) -> Self {
        self.pages.push(build(Page::new(title.into())));
        self
    }

    /// Loads a file into `dataset` when the application starts, so the
    /// dashboard opens with data. The user can still import another file.
    pub fn load_file(mut self, dataset: &DatasetId, path: impl Into<PathBuf>) -> Self {
        self.files.push((dataset.clone(), path.into()));
        self
    }

    /// Listens on a fixed port instead of one chosen by the system.
    pub fn port(mut self, port: u16) -> Self {
        self.config.port = port;
        self
    }

    /// Chooses whether the browser opens when the application starts.
    /// It does by default.
    pub fn open_browser(mut self, open: bool) -> Self {
        self.config.open_browser = open;
        self
    }

    /// Sets the largest file the user can import, in bytes. 256 MiB by default.
    pub fn max_upload_bytes(mut self, bytes: usize) -> Self {
        self.config.max_upload_bytes = bytes;
        self
    }

    /// Chooses whether user preferences, such as the theme, are stored on
    /// disk between runs. They are by default.
    pub fn remember_preferences(mut self, remember: bool) -> Self {
        self.remember_preferences = remember;
        self
    }

    /// Serves a read-only Model Context Protocol endpoint at `/mcp`, so an
    /// AI agent holding the session token can inspect the application.
    /// Off by default.
    #[cfg(feature = "mcp")]
    pub fn mcp(mut self, enabled: bool) -> Self {
        self.mcp = enabled;
        self
    }

    /// Produces the serializable specification of the dashboard.
    ///
    /// Components without an explicit identifier are named after their kind
    /// and order of appearance, for example `chart-1`. Dataset slots are
    /// declared from the datasets the components refer to.
    pub fn spec(&self) -> DashboardSpec {
        let mut counters: BTreeMap<&'static str, u32> = BTreeMap::new();
        let mut datasets: Vec<DatasetSlot> = Vec::new();
        let mut pages = Vec::with_capacity(self.pages.len());

        for (page_index, page) in self.pages.iter().enumerate() {
            let mut rows = Vec::with_capacity(page.rows.len());
            for row in &page.rows {
                let default_span = default_span(row);
                let mut items = Vec::with_capacity(row.len());
                for widget in row {
                    for dependency in widget.kind.dependencies() {
                        if let Dependency::Dataset(id) = dependency
                            && !datasets.iter().any(|slot| slot.id == id)
                        {
                            datasets.push(DatasetSlot { label: id.to_string(), id });
                        }
                    }
                    let id = widget.id.clone().unwrap_or_else(|| {
                        let prefix = kind_name(&widget.kind);
                        let counter = counters.entry(prefix).or_insert(0);
                        *counter += 1;
                        format!("{prefix}-{counter}")
                    });
                    items.push(ComponentSpec {
                        id: ComponentId::new(id),
                        title: widget.title.clone(),
                        span: widget.span.unwrap_or(default_span),
                        kind: widget.kind.clone(),
                    });
                }
                rows.push(RowSpec { items });
            }
            pages.push(PageSpec {
                id: format!("page-{}", page_index + 1),
                title: page.title.clone(),
                rows,
            });
        }

        DashboardSpec { spec_version: SPEC_VERSION, title: self.title.clone(), datasets, pages }
    }

    /// Builds the application without serving it: validates the dashboard
    /// and loads the files given to [`Dashboard::load_file`].
    ///
    /// Use this to serve the application yourself with
    /// [`runtime::start`](eario_runtime::start), for example in a test.
    pub fn build(self) -> Result<(App, ServerConfig)> {
        let spec = self.spec();
        let preferences_path =
            if self.remember_preferences { preferences::default_path(&self.title) } else { None };
        let options = AppOptions { preferences_path, ..AppOptions::default() };
        let app = App::new(spec, options)?;

        for (dataset, path) in &self.files {
            let bytes =
                fs::read(path).map_err(|source| Error::ReadFile { path: path.clone(), source })?;
            let name = path.file_name().map(|name| name.to_string_lossy()).unwrap_or_default();
            app.import_dataset(dataset, &name, &bytes)?;
        }

        #[cfg_attr(not(feature = "mcp"), expect(unused_mut))]
        let mut config = self.config.with_env_overrides();
        #[cfg(feature = "mcp")]
        if self.mcp {
            config.extra_routes = Some(eario_mcp::router(app.clone()));
        }
        Ok((app, config))
    }

    /// Serves the dashboard and opens it in the browser. Returns when the
    /// user quits the application or presses Ctrl+C.
    pub fn run(self) -> Result<()> {
        let (app, config) = self.build()?;
        eario_runtime::run(app, config)?;
        Ok(())
    }
}

/// Columns given to each component of a row that has no explicit span: an
/// even share of what the others leave free.
fn default_span(row: &[Widget]) -> u8 {
    let taken: u8 = row.iter().filter_map(|widget| widget.span).fold(0, u8::saturating_add);
    let flexible = row.iter().filter(|widget| widget.span.is_none()).count();
    match u8::try_from(flexible) {
        Ok(flexible) if flexible > 0 => (GRID_COLUMNS.saturating_sub(taken) / flexible).max(1),
        _ => GRID_COLUMNS,
    }
}

fn kind_name(kind: &ComponentKind) -> &'static str {
    match kind {
        ComponentKind::Text { .. } => "text",
        ComponentKind::FileImport { .. } => "file-import",
        ComponentKind::DatasetInfo { .. } => "dataset-info",
        ComponentKind::ColumnSelect { .. } => "column-select",
        ComponentKind::FilterInput { .. } => "filter-input",
        ComponentKind::DataTable { .. } => "data-table",
        ComponentKind::Chart { .. } => "chart",
        ComponentKind::Statistics { .. } => "statistics",
    }
}

#[cfg(test)]
mod tests {
    use eario_core::{DataView, StateKey};

    use super::*;
    use crate::widgets::{
        data_table, file_import, filter_input, numeric_column_select, scatter_chart, text,
    };

    fn explorer() -> Dashboard {
        let data = DatasetId::new("data");
        let x = StateKey::new("x");
        let y = StateKey::new("y");
        let filter = StateKey::new("filter");
        let view = DataView::of(&data).filtered_by(&filter);
        Dashboard::new("Explorer").page("Explore", |page| {
            page.row([file_import(&data).span(4), filter_input(&data, &filter)])
                .row([
                    numeric_column_select("X", &data, &x, 0),
                    numeric_column_select("Y", &data, &y, 1),
                ])
                .row([scatter_chart(view.clone(), &x, &y).id("main-chart")])
                .row([data_table(view)])
        })
    }

    #[test]
    fn built_spec_is_valid() {
        assert_eq!(explorer().spec().validate(), Ok(()));
    }

    #[test]
    fn components_are_named_after_kind_and_order_unless_given_an_id() {
        let spec = explorer().spec();
        let ids: Vec<&str> = spec.components().map(|component| component.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "file-import-1",
                "filter-input-1",
                "column-select-1",
                "column-select-2",
                "main-chart",
                "data-table-1"
            ]
        );
    }

    #[test]
    fn dataset_slots_are_declared_from_references() {
        let spec = explorer().spec();
        let slots: Vec<&str> = spec.datasets.iter().map(|slot| slot.id.as_str()).collect();
        assert_eq!(slots, ["data"]);
    }

    #[test]
    fn components_without_span_share_the_free_columns() {
        let spec = explorer().spec();
        let spans: Vec<u8> = spec.components().map(|component| component.span).collect();
        assert_eq!(spans, [4, 8, 6, 6, 12, 12]);
    }

    #[test]
    fn crowded_row_never_yields_a_zero_span() {
        let row: Vec<Widget> = (0..20).map(|index| text(index.to_string())).collect();
        assert_eq!(default_span(&row), 1);
    }

    #[test]
    fn dashboard_without_page_is_rejected_at_build() {
        let result = Dashboard::new("Empty").remember_preferences(false).build();
        assert!(matches!(result, Err(Error::App(_))));
    }

    #[test]
    fn missing_startup_file_is_reported_with_its_path() {
        let data = DatasetId::new("data");
        let result = Dashboard::new("Missing")
            .remember_preferences(false)
            .page("Main", |page| page.row([file_import(&data)]))
            .load_file(&data, "does-not-exist.csv")
            .build();
        let message = match result {
            Ok(_) => String::new(),
            Err(error) => error.to_string(),
        };
        assert!(message.contains("does-not-exist.csv"), "{message}");
    }
}
