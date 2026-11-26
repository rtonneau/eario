//! Builder of a dashboard application.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use eario_core::{
    ComponentId, ComponentKind, ComponentSpec, DashboardSpec, DataView, DatasetId, DatasetSlot,
    DatasetSource, Dependency, PageSpec, ParamBinding, RowSpec, SPEC_VERSION,
};
use eario_data::{Computation, ComputationRegistry};
use eario_runtime::{App, AppOptions, ServerConfig, preferences};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::state::State;
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

/// A derived dataset under construction: the computation producing its
/// table and what each input and parameter of the computation is bound to.
///
/// Every input and parameter the computation declares must be bound, and
/// nothing else; the application checks it when it is built.
#[derive(Debug, Clone)]
pub struct Derived {
    function: String,
    inputs: BTreeMap<String, DataView>,
    params: BTreeMap<String, ParamBinding>,
}

impl Derived {
    fn new(function: String) -> Self {
        Self { function, inputs: BTreeMap::new(), params: BTreeMap::new() }
    }

    /// Binds the input table `name` to the rows of `view`: an imported or
    /// a derived dataset, filtered or not.
    pub fn input(mut self, name: impl Into<String>, view: impl Into<DataView>) -> Self {
        self.inputs.insert(name.into(), view.into());
        self
    }

    /// Binds the parameter `name` to the state entry of `state`, so the
    /// dataset is recomputed when an input component changes that entry.
    /// The computation reads it with `ctx.param(&state)`, typed as `T`, or
    /// with `ctx.param(name)`.
    pub fn param<T>(mut self, name: impl Into<String>, state: &State<T>) -> Self {
        self.params.insert(name.into(), ParamBinding::State(state.key().clone()));
        self
    }

    /// Binds the parameter `name` to a constant, such as `20`, `0.5`,
    /// `"total"` or `true`.
    pub fn literal(mut self, name: impl Into<String>, value: impl Into<Value>) -> Self {
        self.params.insert(name.into(), ParamBinding::Literal(value.into()));
        self
    }

    fn source(&self) -> DatasetSource {
        DatasetSource::Computed {
            function: self.function.clone(),
            inputs: self.inputs.clone(),
            params: self.params.clone(),
        }
    }
}

/// A dashboard application under construction.
///
/// Describe the pages, then call [`Dashboard::run`].
pub struct Dashboard {
    title: String,
    pages: Vec<Page>,
    files: Vec<(DatasetId, PathBuf)>,
    /// The computations, in the order they were registered. Two sharing a
    /// name are rejected by [`Dashboard::build`].
    computations: Vec<Computation>,
    /// The derived datasets, in the order they were declared. Two sharing
    /// an identifier make an invalid specification.
    derived: Vec<(DatasetId, Derived)>,
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
            computations: Vec::new(),
            derived: Vec::new(),
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

    /// Registers a computation, so derived datasets can name it.
    ///
    /// Names are unique: [`Dashboard::build`] fails with
    /// [`Error::DuplicateComputation`] if two computations share one.
    pub fn computation(mut self, computation: Computation) -> Self {
        self.computations.push(computation);
        self
    }

    /// Declares `dataset` as a derived dataset: its table is produced by
    /// the registered computation named `computation`. `bind` receives the
    /// declaration and returns it with every input and parameter of the
    /// computation bound.
    ///
    /// Components read a derived dataset like an imported one. A dataset
    /// is declared once: declaring it again makes the specification invalid
    /// (`dataset identifier ... is declared more than once`), and
    /// [`Dashboard::build`] fails.
    pub fn derived(
        mut self,
        dataset: &DatasetId,
        computation: impl Into<String>,
        bind: impl FnOnce(Derived) -> Derived,
    ) -> Self {
        let declaration = bind(Derived::new(computation.into()));
        self.derived.push((dataset.clone(), declaration));
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
    /// declared from the datasets the components and the derived datasets
    /// refer to, in order of first reference, followed by the derived
    /// datasets nothing refers to. A slot is imported unless it was
    /// declared with [`Dashboard::derived`]. A dataset declared more than
    /// once appears once per declaration, which
    /// [`DashboardSpec::validate`] rejects.
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
                        if let Dependency::Dataset(id) = dependency {
                            self.declare(&mut datasets, &id);
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

        for (index, (id, declaration)) in self.derived.iter().enumerate() {
            if self.derived[..index].iter().any(|(earlier, _)| earlier == id) {
                // Kept, so that validation reports the duplicate.
                datasets.push(DatasetSlot {
                    id: id.clone(),
                    label: id.to_string(),
                    source: declaration.source(),
                });
            } else {
                self.declare(&mut datasets, id);
            }
        }

        DashboardSpec { spec_version: SPEC_VERSION, title: self.title.clone(), datasets, pages }
    }

    /// Adds the slot `id` to `datasets` unless it is there already; for a
    /// derived dataset, also the slots of its inputs, after it.
    fn declare(&self, datasets: &mut Vec<DatasetSlot>, id: &DatasetId) {
        if datasets.iter().any(|slot| &slot.id == id) {
            return;
        }
        let derived = self.derived.iter().find(|(derived, _)| derived == id).map(|(_, d)| d);
        let source = derived.map_or(DatasetSource::Imported, Derived::source);
        // Pushed before its inputs are declared, so that a cycle between
        // derived datasets stops here; `DashboardSpec::validate` reports it.
        datasets.push(DatasetSlot { id: id.clone(), label: id.to_string(), source });
        for view in derived.into_iter().flat_map(|derived| derived.inputs.values()) {
            self.declare(datasets, &view.dataset);
        }
    }

    /// Builds the application without serving it: validates the dashboard
    /// and loads the files given to [`Dashboard::load_file`].
    ///
    /// Use this to serve the application yourself with
    /// [`runtime::start`](eario_runtime::start), for example in a test.
    pub fn build(self) -> Result<(App, ServerConfig)> {
        let spec = self.spec();
        let mut computations = ComputationRegistry::default();
        for computation in self.computations {
            let name = computation.name().to_owned();
            if computations.register(computation).is_some() {
                return Err(Error::DuplicateComputation(name));
            }
        }
        let preferences_path =
            if self.remember_preferences { preferences::default_path(&self.title) } else { None };
        let options = AppOptions { preferences_path, computations, ..AppOptions::default() };
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
        ComponentKind::NumberInput { .. } => "number-input",
        ComponentKind::Slider { .. } => "slider",
        ComponentKind::TextInput { .. } => "text-input",
        ComponentKind::Select { .. } => "select",
        ComponentKind::Checkbox { .. } => "checkbox",
    }
}

#[cfg(test)]
mod tests {
    use eario_core::{Filter, StateKey};
    use eario_data::{ComputeError, DataFrame};
    use serde_json::json;

    use super::*;
    use crate::state::filtered;
    use crate::widgets::{
        column_select, data_table, file_import, filter_input, numeric_column_select, scatter_chart,
        statistics, text,
    };

    fn explorer() -> Dashboard {
        let data = DatasetId::new("data");
        let x = State::new("x");
        let y = State::new("y");
        let filter = State::<Filter>::new("filter");
        let view = filtered(&data, &filter);
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

    /// `raw` is imported; `scaled` multiplies the chosen column of `raw` by
    /// a constant; `summary` reads `scaled` and nothing reads `summary`.
    fn with_derived() -> Dashboard {
        let raw = DatasetId::new("raw");
        let scaled = DatasetId::new("scaled");
        let summary = DatasetId::new("summary");
        let column = State::<String>::new("column");
        let filter = State::<Filter>::new("filter");
        let echo =
            |ctx: &eario_data::ComputeContext| -> std::result::Result<DataFrame, ComputeError> {
                Ok(ctx.input("table")?.clone())
            };
        Dashboard::new("Derived")
            .remember_preferences(false)
            .computation(
                Computation::new("scale", echo).input("table").param("column").param("factor"),
            )
            .computation(Computation::new("summary", echo).input("table"))
            .derived(&summary, "summary", |d| d.input("table", &scaled))
            .derived(&scaled, "scale", |d| {
                d.input("table", filtered(&raw, &filter))
                    .param("column", &column)
                    .literal("factor", 2.5)
            })
            .page("Main", |page| {
                page.row([file_import(&raw), column_select("Column", &raw, &column)])
                    .row([filter_input(&raw, &filter), statistics(&scaled)])
            })
    }

    #[test]
    fn derived_datasets_are_declared_with_their_bindings() {
        let spec = with_derived().spec();
        assert_eq!(spec.validate(), Ok(()));
        let slots: Vec<&str> = spec.datasets.iter().map(|slot| slot.id.as_str()).collect();
        assert_eq!(slots, ["raw", "scaled", "summary"]);
        assert_eq!(spec.datasets[0].source, DatasetSource::Imported);
        assert_eq!(
            spec.datasets[1].source,
            DatasetSource::Computed {
                function: "scale".to_owned(),
                inputs: BTreeMap::from([(
                    "table".to_owned(),
                    DataView::of(&DatasetId::new("raw")).filtered_by(&StateKey::new("filter")),
                )]),
                params: BTreeMap::from([
                    ("column".to_owned(), ParamBinding::State(StateKey::new("column"))),
                    ("factor".to_owned(), ParamBinding::Literal(json!(2.5))),
                ]),
            }
        );
        assert!(matches!(
            &spec.datasets[2].source,
            DatasetSource::Computed { function, .. } if function == "summary"
        ));
    }

    #[test]
    fn inputs_of_a_derived_dataset_are_declared_even_if_no_component_reads_them() {
        let raw = DatasetId::new("raw");
        let copy = DatasetId::new("copy");
        let spec = Dashboard::new("Copy")
            .derived(&copy, "copy", |d| d.input("table", &raw))
            .page("Main", |page| page.row([statistics(&copy)]))
            .spec();
        let slots: Vec<(&str, bool)> = spec
            .datasets
            .iter()
            .map(|slot| (slot.id.as_str(), slot.source == DatasetSource::Imported))
            .collect();
        assert_eq!(slots, [("copy", false), ("raw", true)]);
    }

    #[test]
    fn dataset_declared_twice_is_rejected_at_build() {
        let scaled = DatasetId::new("scaled");
        let raw = DatasetId::new("raw");
        let twice = || with_derived().derived(&scaled, "summary", |d| d.input("table", &raw));
        assert_eq!(
            twice().spec().validate(),
            Err(eario_core::SpecError::DuplicateDataset(scaled.clone()))
        );
        let error = twice().build().err();
        assert_eq!(error.as_ref().map(Error::code), Some("invalid_spec"));
        let message = error.map(|error| error.to_string()).unwrap_or_default();
        assert!(message.contains("`scaled` is declared more than once"), "{message}");
    }

    #[test]
    fn computation_registered_twice_is_rejected_at_build() {
        let again = Computation::new("scale", |ctx| Ok(ctx.input("table")?.clone()));
        let error = with_derived().computation(again).build().err();
        assert!(
            matches!(&error, Some(Error::DuplicateComputation(name)) if name == "scale"),
            "{error:?}"
        );
        assert_eq!(error.as_ref().map(Error::code), Some("duplicate_computation"));
        let message = error.map(|error| error.to_string()).unwrap_or_default();
        assert!(message.contains("`scale` is registered more than once"), "{message}");
    }

    #[test]
    fn registered_computations_are_given_to_the_application() {
        assert!(with_derived().build().is_ok());

        let unregistered = DatasetId::new("missing");
        let result = with_derived()
            .derived(&unregistered, "nowhere", |d| d.input("table", &DatasetId::new("raw")))
            .build();
        let message = result.err().map(|error| error.to_string()).unwrap_or_default();
        assert!(message.contains("nowhere"), "{message}");
    }
}
