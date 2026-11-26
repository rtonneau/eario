//! Serializable dashboard specification.
//!
//! A [`DashboardSpec`] describes what a dashboard contains and how its parts
//! depend on each other. It holds no executable code and no reference to a
//! frontend technology: the Rust builder API produces one, the generic
//! frontend renders one, and it round-trips through JSON.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::id::{ComponentId, DatasetId, StateKey};

/// Version of the specification format written by this crate.
///
/// Version 2 added [`DatasetSlot::source`]. A version-1 specification, which
/// has no `source`, still deserializes: every slot is then
/// [`DatasetSource::Imported`].
pub const SPEC_VERSION: u32 = 2;

/// Number of columns of the layout grid a row is divided into.
pub const GRID_COLUMNS: u8 = 12;

/// A complete dashboard: its dataset slots and its pages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DashboardSpec {
    /// Version of the specification format, see [`SPEC_VERSION`].
    pub spec_version: u32,
    /// Title shown in the application header.
    pub title: String,
    /// Dataset slots the components refer to.
    pub datasets: Vec<DatasetSlot>,
    /// Pages, in navigation order.
    pub pages: Vec<PageSpec>,
}

/// A named place a dataset is loaded into, by an import or by the developer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DatasetSlot {
    /// Identifier components use to refer to the dataset.
    pub id: DatasetId,
    /// Human-readable name of the slot.
    pub label: String,
    /// Where the table comes from. Absent in a version-1 specification,
    /// which means [`DatasetSource::Imported`].
    #[serde(default)]
    pub source: DatasetSource,
}

impl DatasetSlot {
    /// Lists what the slot reads directly: for a derived dataset, its input
    /// datasets, their filters and its state-bound parameters; nothing for
    /// an imported one.
    ///
    /// See [`DashboardSpec::dataset_dependencies`] for the transitive closure.
    pub fn dependencies(&self) -> Vec<Dependency> {
        let DatasetSource::Computed { inputs, params, .. } = &self.source else {
            return Vec::new();
        };
        let mut dependencies = Vec::new();
        for view in inputs.values() {
            dependencies.extend(view_dependencies(view));
        }
        for binding in params.values() {
            if let ParamBinding::State(state) = binding {
                dependencies.push(Dependency::State(state.clone()));
            }
        }
        dependencies
    }
}

/// Where the table of a dataset slot comes from.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DatasetSource {
    /// Loaded from a file by the user, or passed in by the developer.
    #[default]
    Imported,
    /// Produced by a named computation registered by the developer.
    Computed {
        /// Name of the registered computation.
        function: String,
        /// Datasets the computation reads, by the name it knows them under.
        /// Each input is a [`DataView`], so it may carry a filter.
        inputs: BTreeMap<String, DataView>,
        /// Parameters of the computation, by the name it knows them under.
        params: BTreeMap<String, ParamBinding>,
    },
}

/// The value a computation parameter takes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ParamBinding {
    /// The current value of an application state entry, so the dataset is
    /// recomputed when an input component changes it.
    State(StateKey),
    /// A constant fixed by the developer.
    Literal(Value),
}

/// One page of a dashboard: a vertical stack of rows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PageSpec {
    /// Identifier of the page, unique within the dashboard.
    pub id: String,
    /// Title shown in the navigation.
    pub title: String,
    /// Rows, from top to bottom.
    pub rows: Vec<RowSpec>,
}

/// One row of a page: components laid out on a 12-column grid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct RowSpec {
    /// Components, from left to right.
    pub items: Vec<ComponentSpec>,
}

/// One component and its place in the layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ComponentSpec {
    /// Identifier of the component, unique within the dashboard.
    pub id: ComponentId,
    /// Optional title shown above the component.
    pub title: Option<String>,
    /// Width of the component, in grid columns (1 to 12).
    pub span: u8,
    /// What the component is and what it is bound to.
    pub kind: ComponentKind,
}

/// The kinds of component the generic frontend can render.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ComponentKind {
    /// Static text.
    Text {
        /// The text to show. Blank lines separate paragraphs.
        text: String,
    },
    /// Lets the user load a file into a dataset slot and, for workbooks,
    /// choose the worksheet.
    FileImport {
        /// Slot the file is loaded into.
        dataset: DatasetId,
    },
    /// Shows the metadata of a dataset: source, size and columns.
    DatasetInfo {
        /// Dataset to describe.
        dataset: DatasetId,
    },
    /// Lets the user choose one column of a dataset.
    ColumnSelect {
        /// Dataset whose columns are offered.
        dataset: DatasetId,
        /// State entry receiving the chosen column name.
        state: StateKey,
        /// Label of the input.
        label: String,
        /// Offer numeric columns only.
        numeric_only: bool,
        /// Position, among the offered columns, of the one chosen when a
        /// dataset is loaded.
        default_index: u32,
    },
    /// Lets the user define a row [`Filter`](crate::Filter) on a dataset.
    FilterInput {
        /// Dataset whose columns can be filtered.
        dataset: DatasetId,
        /// State entry receiving the filter, or `null` when there is none.
        state: StateKey,
    },
    /// A paged table of rows.
    DataTable {
        /// Rows to show.
        view: DataView,
        /// Number of rows per page.
        page_size: u32,
    },
    /// A chart of one numeric column against another.
    Chart {
        /// Rows to plot.
        view: DataView,
        /// How points are drawn.
        mark: ChartMark,
        /// State entry holding the name of the horizontal-axis column.
        x: StateKey,
        /// State entry holding the name of the vertical-axis column.
        y: StateKey,
        /// Upper bound on the number of points sent to the frontend.
        max_points: u32,
    },
    /// Descriptive statistics of the numeric columns.
    Statistics {
        /// Rows to summarize.
        view: DataView,
    },
}

/// The rows a component reads: a dataset, optionally filtered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DataView {
    /// Dataset the rows come from.
    pub dataset: DatasetId,
    /// State entry holding the filter to apply, if any.
    pub filter: Option<StateKey>,
}

impl DataView {
    /// All rows of `dataset`.
    pub fn of(dataset: &DatasetId) -> Self {
        Self { dataset: dataset.clone(), filter: None }
    }

    /// Restricts the view to the rows matching the filter held in `state`.
    pub fn filtered_by(mut self, state: &StateKey) -> Self {
        self.filter = Some(state.clone());
        self
    }
}

impl From<&DatasetId> for DataView {
    fn from(dataset: &DatasetId) -> Self {
        Self::of(dataset)
    }
}

/// How a chart draws its points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ChartMark {
    /// Unconnected points.
    Scatter,
    /// Points connected in row order.
    Line,
}

/// Something a component reads. When it changes, the component is stale.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Dependency {
    /// The content of a dataset slot.
    Dataset(DatasetId),
    /// An entry of the application state.
    State(StateKey),
}

impl ComponentKind {
    /// Lists what the component reads to produce its data.
    ///
    /// An input component does not depend on the state entry it writes.
    pub fn dependencies(&self) -> Vec<Dependency> {
        match self {
            Self::Text { .. } => Vec::new(),
            Self::FileImport { dataset }
            | Self::DatasetInfo { dataset }
            | Self::ColumnSelect { dataset, .. }
            | Self::FilterInput { dataset, .. } => vec![Dependency::Dataset(dataset.clone())],
            Self::DataTable { view, .. } | Self::Statistics { view } => view_dependencies(view),
            Self::Chart { view, x, y, .. } => {
                let mut dependencies = view_dependencies(view);
                dependencies.push(Dependency::State(x.clone()));
                dependencies.push(Dependency::State(y.clone()));
                dependencies
            }
        }
    }

    /// Returns the state entry the component writes, for input components.
    pub fn written_state(&self) -> Option<&StateKey> {
        match self {
            Self::ColumnSelect { state, .. } | Self::FilterInput { state, .. } => Some(state),
            _ => None,
        }
    }
}

fn view_dependencies(view: &DataView) -> Vec<Dependency> {
    let mut dependencies = vec![Dependency::Dataset(view.dataset.clone())];
    if let Some(filter) = &view.filter {
        dependencies.push(Dependency::State(filter.clone()));
    }
    dependencies
}

/// Reason a [`DashboardSpec`] is rejected by [`DashboardSpec::validate`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SpecError {
    /// The specification was written by a newer version of the format.
    #[error("unsupported specification version {found}, this build reads up to {supported}")]
    UnsupportedVersion {
        /// Version found in the specification.
        found: u32,
        /// Highest version this build understands.
        supported: u32,
    },
    /// The dashboard has no page.
    #[error("the dashboard has no page")]
    NoPages,
    /// Two pages share an identifier.
    #[error("page identifier `{0}` is used more than once")]
    DuplicatePage(String),
    /// Two components share an identifier.
    #[error("component identifier `{0}` is used more than once")]
    DuplicateComponent(ComponentId),
    /// Two dataset slots share an identifier.
    #[error("dataset identifier `{0}` is declared more than once")]
    DuplicateDataset(DatasetId),
    /// A component refers to a dataset slot that is not declared.
    #[error("component `{component}` refers to undeclared dataset `{dataset}`")]
    UnknownDataset {
        /// Component holding the reference.
        component: ComponentId,
        /// The undeclared dataset.
        dataset: DatasetId,
    },
    /// A component reads a state entry that no input component writes.
    #[error("component `{component}` reads state `{state}`, which no input writes")]
    UnknownState {
        /// Component holding the reference.
        component: ComponentId,
        /// The state entry nobody writes.
        state: StateKey,
    },
    /// Two input components write the same state entry.
    #[error("state `{0}` is written by more than one input")]
    DuplicateStateWriter(StateKey),
    /// A component is narrower than one column or wider than the grid.
    #[error("component `{component}` has span {span}, expected 1 to 12")]
    InvalidSpan {
        /// The misplaced component.
        component: ComponentId,
        /// Its span.
        span: u8,
    },
    /// A table shows zero rows per page.
    #[error("component `{0}` has a page size of zero")]
    ZeroPageSize(ComponentId),
    /// An input of a derived dataset refers to a dataset slot that is not
    /// declared.
    #[error(
        "derived dataset `{dataset}` reads input `{input}` from undeclared dataset `{source_dataset}`"
    )]
    UnknownComputedInput {
        /// The derived dataset holding the reference.
        dataset: DatasetId,
        /// Name the computation knows the input under.
        input: String,
        /// The undeclared dataset the input points at.
        source_dataset: DatasetId,
    },
    /// A derived dataset reads a state entry that no input component writes,
    /// as a parameter or as the filter of an input.
    #[error(
        "derived dataset `{dataset}` reads state `{state}` for `{name}`, which no input writes"
    )]
    UnknownComputedState {
        /// The derived dataset holding the reference.
        dataset: DatasetId,
        /// Name of the parameter, or of the input whose filter reads the state.
        name: String,
        /// The state entry nobody writes.
        state: StateKey,
    },
    /// Derived datasets read each other in a loop, so none could be computed
    /// first.
    #[error("derived datasets depend on each other in a cycle: {path}")]
    DerivedDatasetCycle {
        /// The datasets of the cycle, for example `a -> b -> a`.
        path: String,
    },
    /// A file import targets a derived dataset, which a computation fills.
    #[error(
        "component `{component}` imports a file into dataset `{dataset}`, which is derived by a computation"
    )]
    ImportIntoDerivedDataset {
        /// The file-import component.
        component: ComponentId,
        /// The derived dataset it targets.
        dataset: DatasetId,
    },
}

impl DashboardSpec {
    /// Iterates over every component of every page.
    pub fn components(&self) -> impl Iterator<Item = &ComponentSpec> {
        self.pages.iter().flat_map(|page| &page.rows).flat_map(|row| &row.items)
    }

    /// Finds a component by identifier.
    pub fn component(&self, id: &ComponentId) -> Option<&ComponentSpec> {
        self.components().find(|component| &component.id == id)
    }

    /// Finds a dataset slot by identifier.
    pub fn dataset(&self, id: &DatasetId) -> Option<&DatasetSlot> {
        self.datasets.iter().find(|slot| &slot.id == id)
    }

    /// Lists everything a dataset depends on, following derived datasets.
    ///
    /// For a derived dataset this is its input datasets, the filters of those
    /// inputs and its state-bound parameters, then the same for every input
    /// that is itself derived. The result holds the input datasets
    /// themselves, not `id`. It is empty for an imported or an undeclared
    /// dataset, and terminates even on a specification with a cycle.
    pub fn dataset_dependencies(&self, id: &DatasetId) -> BTreeSet<Dependency> {
        let mut dependencies = BTreeSet::new();
        let mut visited = BTreeSet::new();
        let mut pending = vec![id];
        while let Some(current) = pending.pop() {
            if !visited.insert(current) {
                continue;
            }
            let Some(slot) = self.dataset(current) else { continue };
            dependencies.extend(slot.dependencies());
            if let DatasetSource::Computed { inputs, .. } = &slot.source {
                pending.extend(inputs.values().map(|view| &view.dataset));
            }
        }
        dependencies
    }

    /// Checks that the specification is internally consistent.
    ///
    /// The runtime calls this before serving a dashboard, so the frontend
    /// and the data layer can rely on every reference being resolvable.
    pub fn validate(&self) -> Result<(), SpecError> {
        if self.spec_version > SPEC_VERSION {
            return Err(SpecError::UnsupportedVersion {
                found: self.spec_version,
                supported: SPEC_VERSION,
            });
        }
        if self.pages.is_empty() {
            return Err(SpecError::NoPages);
        }

        let mut page_ids = BTreeSet::new();
        for page in &self.pages {
            if !page_ids.insert(page.id.as_str()) {
                return Err(SpecError::DuplicatePage(page.id.clone()));
            }
        }

        let mut datasets = BTreeSet::new();
        for slot in &self.datasets {
            if !datasets.insert(&slot.id) {
                return Err(SpecError::DuplicateDataset(slot.id.clone()));
            }
        }

        let mut component_ids = BTreeSet::new();
        let mut written_states = BTreeSet::new();
        for component in self.components() {
            if !component_ids.insert(&component.id) {
                return Err(SpecError::DuplicateComponent(component.id.clone()));
            }
            if component.span == 0 || component.span > GRID_COLUMNS {
                return Err(SpecError::InvalidSpan {
                    component: component.id.clone(),
                    span: component.span,
                });
            }
            if let ComponentKind::DataTable { page_size: 0, .. } = component.kind {
                return Err(SpecError::ZeroPageSize(component.id.clone()));
            }
            if let Some(state) = component.kind.written_state()
                && !written_states.insert(state)
            {
                return Err(SpecError::DuplicateStateWriter(state.clone()));
            }
        }

        self.validate_derived_datasets(&datasets, &written_states)?;

        for component in self.components() {
            if let ComponentKind::FileImport { dataset } = &component.kind
                && self
                    .dataset(dataset)
                    .is_some_and(|slot| matches!(slot.source, DatasetSource::Computed { .. }))
            {
                return Err(SpecError::ImportIntoDerivedDataset {
                    component: component.id.clone(),
                    dataset: dataset.clone(),
                });
            }
            for dependency in component.kind.dependencies() {
                match dependency {
                    Dependency::Dataset(dataset) if !datasets.contains(&dataset) => {
                        return Err(SpecError::UnknownDataset {
                            component: component.id.clone(),
                            dataset,
                        });
                    }
                    Dependency::State(state) if !written_states.contains(&state) => {
                        return Err(SpecError::UnknownState {
                            component: component.id.clone(),
                            state,
                        });
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// Checks the references of derived datasets and that they form no cycle.
    ///
    /// Whether the computation exists and whether its input and parameter
    /// names are the ones it declares is known only to the runtime.
    fn validate_derived_datasets(
        &self,
        datasets: &BTreeSet<&DatasetId>,
        written_states: &BTreeSet<&StateKey>,
    ) -> Result<(), SpecError> {
        for slot in &self.datasets {
            let DatasetSource::Computed { inputs, params, .. } = &slot.source else { continue };
            for (name, view) in inputs {
                if !datasets.contains(&view.dataset) {
                    return Err(SpecError::UnknownComputedInput {
                        dataset: slot.id.clone(),
                        input: name.clone(),
                        source_dataset: view.dataset.clone(),
                    });
                }
                if let Some(filter) = &view.filter
                    && !written_states.contains(filter)
                {
                    return Err(SpecError::UnknownComputedState {
                        dataset: slot.id.clone(),
                        name: name.clone(),
                        state: filter.clone(),
                    });
                }
            }
            for (name, binding) in params {
                if let ParamBinding::State(state) = binding
                    && !written_states.contains(state)
                {
                    return Err(SpecError::UnknownComputedState {
                        dataset: slot.id.clone(),
                        name: name.clone(),
                        state: state.clone(),
                    });
                }
            }
        }

        let mut finished = BTreeSet::new();
        for slot in &self.datasets {
            self.visit_derived(&slot.id, &mut Vec::new(), &mut finished)?;
        }
        Ok(())
    }

    /// Depth-first search along the "reads" edges between derived datasets.
    /// `path` holds the datasets being visited, `finished` those proven
    /// free of cycles.
    fn visit_derived<'a>(
        &'a self,
        id: &'a DatasetId,
        path: &mut Vec<&'a DatasetId>,
        finished: &mut BTreeSet<&'a DatasetId>,
    ) -> Result<(), SpecError> {
        if finished.contains(id) {
            return Ok(());
        }
        if let Some(start) = path.iter().position(|visited| *visited == id) {
            let mut cycle: Vec<String> = path[start..].iter().map(ToString::to_string).collect();
            cycle.push(id.to_string());
            return Err(SpecError::DerivedDatasetCycle { path: cycle.join(" -> ") });
        }
        if let Some(DatasetSlot { source: DatasetSource::Computed { inputs, .. }, .. }) =
            self.dataset(id)
        {
            path.push(id);
            for view in inputs.values() {
                self.visit_derived(&view.dataset, path, finished)?;
            }
            path.pop();
        }
        finished.insert(id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn component(id: &str, kind: ComponentKind) -> ComponentSpec {
        ComponentSpec { id: ComponentId::new(id), title: None, span: 6, kind }
    }

    fn sample_spec() -> DashboardSpec {
        let data = DatasetId::new("data");
        let x = StateKey::new("x");
        let y = StateKey::new("y");
        let filter = StateKey::new("filter");
        let view = DataView::of(&data).filtered_by(&filter);
        DashboardSpec {
            spec_version: SPEC_VERSION,
            title: "Sample".to_owned(),
            datasets: vec![DatasetSlot {
                id: data.clone(),
                label: "Data".to_owned(),
                source: DatasetSource::Imported,
            }],
            pages: vec![PageSpec {
                id: "main".to_owned(),
                title: "Main".to_owned(),
                rows: vec![RowSpec {
                    items: vec![
                        component("import", ComponentKind::FileImport { dataset: data.clone() }),
                        component(
                            "x",
                            ComponentKind::ColumnSelect {
                                dataset: data.clone(),
                                state: x.clone(),
                                label: "X".to_owned(),
                                numeric_only: true,
                                default_index: 0,
                            },
                        ),
                        component(
                            "y",
                            ComponentKind::ColumnSelect {
                                dataset: data.clone(),
                                state: y.clone(),
                                label: "Y".to_owned(),
                                numeric_only: true,
                                default_index: 1,
                            },
                        ),
                        component(
                            "filter",
                            ComponentKind::FilterInput { dataset: data.clone(), state: filter },
                        ),
                        component(
                            "chart",
                            ComponentKind::Chart {
                                view: view.clone(),
                                mark: ChartMark::Scatter,
                                x,
                                y,
                                max_points: 1000,
                            },
                        ),
                        component("table", ComponentKind::DataTable { view, page_size: 25 }),
                    ],
                }],
            }],
        }
    }

    #[test]
    fn valid_spec_passes_validation() {
        assert_eq!(sample_spec().validate(), Ok(()));
    }

    #[test]
    fn spec_round_trips_through_json() -> Result<(), serde_json::Error> {
        let spec = sample_spec();
        let json = serde_json::to_string(&spec)?;
        let restored: DashboardSpec = serde_json::from_str(&json)?;
        assert_eq!(restored, spec);
        Ok(())
    }

    #[test]
    fn component_kind_is_tagged_with_snake_case_type() -> Result<(), serde_json::Error> {
        let kind = ComponentKind::FileImport { dataset: DatasetId::new("data") };
        let json = serde_json::to_value(&kind)?;
        assert_eq!(json, serde_json::json!({ "type": "file_import", "dataset": "data" }));
        Ok(())
    }

    #[test]
    fn duplicate_component_id_is_rejected() {
        let mut spec = sample_spec();
        let duplicate = spec.pages[0].rows[0].items[0].clone();
        spec.pages[0].rows[0].items.push(duplicate);
        assert_eq!(spec.validate(), Err(SpecError::DuplicateComponent(ComponentId::new("import"))));
    }

    #[test]
    fn undeclared_dataset_is_rejected() {
        let mut spec = sample_spec();
        spec.datasets.clear();
        assert!(matches!(spec.validate(), Err(SpecError::UnknownDataset { .. })));
    }

    #[test]
    fn state_without_writer_is_rejected() {
        let mut spec = sample_spec();
        spec.pages[0].rows[0].items.retain(|item| item.id.as_str() != "filter");
        assert!(matches!(spec.validate(), Err(SpecError::UnknownState { .. })));
    }

    #[test]
    fn two_writers_of_one_state_are_rejected() {
        let mut spec = sample_spec();
        let mut second = spec.pages[0].rows[0].items[1].clone();
        second.id = ComponentId::new("x-again");
        spec.pages[0].rows[0].items.push(second);
        assert_eq!(spec.validate(), Err(SpecError::DuplicateStateWriter(StateKey::new("x"))));
    }

    #[test]
    fn out_of_grid_span_is_rejected() {
        let mut spec = sample_spec();
        spec.pages[0].rows[0].items[0].span = 13;
        assert!(matches!(spec.validate(), Err(SpecError::InvalidSpan { span: 13, .. })));
    }

    #[test]
    fn newer_spec_version_is_rejected() {
        let mut spec = sample_spec();
        spec.spec_version = SPEC_VERSION + 1;
        assert!(matches!(spec.validate(), Err(SpecError::UnsupportedVersion { .. })));
    }

    fn derived(
        function: &str,
        inputs: &[(&str, DataView)],
        params: &[(&str, ParamBinding)],
    ) -> DatasetSource {
        DatasetSource::Computed {
            function: function.to_owned(),
            inputs: inputs.iter().map(|(name, view)| ((*name).to_owned(), view.clone())).collect(),
            params: params
                .iter()
                .map(|(name, binding)| ((*name).to_owned(), binding.clone()))
                .collect(),
        }
    }

    fn slot(id: &str, source: DatasetSource) -> DatasetSlot {
        DatasetSlot { id: DatasetId::new(id), label: id.to_owned(), source }
    }

    /// `sample_spec` plus a derived dataset `summary` computed from `data`,
    /// filtered by the `filter` state and parameterized by the `x` state.
    fn derived_spec() -> DashboardSpec {
        let mut spec = sample_spec();
        let data = DatasetId::new("data");
        spec.datasets.push(slot(
            "summary",
            derived(
                "summarize",
                &[("table", DataView::of(&data).filtered_by(&StateKey::new("filter")))],
                &[
                    ("column", ParamBinding::State(StateKey::new("x"))),
                    ("bins", ParamBinding::Literal(serde_json::json!(10))),
                ],
            ),
        ));
        spec
    }

    #[test]
    fn spec_version_is_two() {
        assert_eq!(SPEC_VERSION, 2);
    }

    #[test]
    fn version_one_spec_without_source_reads_as_imported() -> Result<(), serde_json::Error> {
        let json = r#"{
            "spec_version": 1,
            "title": "Old",
            "datasets": [{ "id": "data", "label": "Data" }],
            "pages": [{
                "id": "main",
                "title": "Main",
                "rows": [{ "items": [{
                    "id": "table",
                    "title": null,
                    "span": 12,
                    "kind": {
                        "type": "data_table",
                        "view": { "dataset": "data", "filter": null },
                        "page_size": 10
                    }
                }] }]
            }]
        }"#;
        let spec: DashboardSpec = serde_json::from_str(json)?;
        assert_eq!(spec.datasets[0].source, DatasetSource::Imported);
        assert_eq!(spec.validate(), Ok(()));
        Ok(())
    }

    #[test]
    fn dataset_source_is_tagged_and_round_trips() -> Result<(), serde_json::Error> {
        let spec = derived_spec();
        let json = serde_json::to_value(&spec.datasets[1].source)?;
        assert_eq!(
            json,
            serde_json::json!({
                "type": "computed",
                "function": "summarize",
                "inputs": { "table": { "dataset": "data", "filter": "filter" } },
                "params": {
                    "bins": { "type": "literal", "value": 10 },
                    "column": { "type": "state", "value": "x" }
                }
            })
        );
        let restored: DashboardSpec = serde_json::from_str(&serde_json::to_string(&spec)?)?;
        assert_eq!(restored, spec);
        Ok(())
    }

    #[test]
    fn derived_spec_passes_validation() {
        assert_eq!(derived_spec().validate(), Ok(()));
    }

    #[test]
    fn computed_input_naming_an_undeclared_dataset_is_rejected() {
        let mut spec = derived_spec();
        spec.datasets[1].source =
            derived("summarize", &[("table", DataView::of(&DatasetId::new("missing")))], &[]);
        assert_eq!(
            spec.validate(),
            Err(SpecError::UnknownComputedInput {
                dataset: DatasetId::new("summary"),
                input: "table".to_owned(),
                source_dataset: DatasetId::new("missing"),
            })
        );
    }

    #[test]
    fn param_bound_to_an_unwritten_state_is_rejected() {
        let mut spec = derived_spec();
        spec.datasets[1].source = derived(
            "summarize",
            &[("table", DataView::of(&DatasetId::new("data")))],
            &[("column", ParamBinding::State(StateKey::new("nobody")))],
        );
        let error = spec.validate();
        assert_eq!(
            error,
            Err(SpecError::UnknownComputedState {
                dataset: DatasetId::new("summary"),
                name: "column".to_owned(),
                state: StateKey::new("nobody"),
            })
        );
    }

    #[test]
    fn input_filter_on_an_unwritten_state_is_rejected() {
        let mut spec = derived_spec();
        spec.datasets[1].source = derived(
            "summarize",
            &[(
                "table",
                DataView::of(&DatasetId::new("data")).filtered_by(&StateKey::new("nobody")),
            )],
            &[],
        );
        assert!(matches!(spec.validate(), Err(SpecError::UnknownComputedState { .. })));
    }

    #[test]
    fn cycle_between_derived_datasets_is_rejected() {
        let mut spec = derived_spec();
        let summary = DatasetId::new("summary");
        let other = DatasetId::new("other");
        spec.datasets[1].source = derived("f", &[("in", DataView::of(&other))], &[]);
        spec.datasets.push(slot("other", derived("g", &[("in", DataView::of(&summary))], &[])));
        let error = spec.validate();
        assert_eq!(
            error,
            Err(SpecError::DerivedDatasetCycle { path: "summary -> other -> summary".to_owned() })
        );
        let message = error.map_err(|error| error.to_string()).err().unwrap_or_default();
        assert!(message.contains("summary -> other -> summary"));
    }

    #[test]
    fn derived_dataset_reading_itself_is_a_cycle() {
        let mut spec = derived_spec();
        let summary = DatasetId::new("summary");
        spec.datasets[1].source = derived("f", &[("in", DataView::of(&summary))], &[]);
        assert_eq!(
            spec.validate(),
            Err(SpecError::DerivedDatasetCycle { path: "summary -> summary".to_owned() })
        );
    }

    #[test]
    fn diamond_of_derived_datasets_is_not_a_cycle() {
        let mut spec = derived_spec();
        let data = DatasetId::new("data");
        let left = DatasetId::new("left");
        let right = DatasetId::new("right");
        spec.datasets.push(slot("left", derived("f", &[("in", DataView::of(&data))], &[])));
        spec.datasets.push(slot("right", derived("f", &[("in", DataView::of(&data))], &[])));
        spec.datasets.push(slot(
            "joined",
            derived("join", &[("a", DataView::of(&left)), ("b", DataView::of(&right))], &[]),
        ));
        assert_eq!(spec.validate(), Ok(()));
    }

    #[test]
    fn file_import_into_a_derived_dataset_is_rejected() {
        let mut spec = derived_spec();
        spec.pages[0].rows[0].items.push(component(
            "import-summary",
            ComponentKind::FileImport { dataset: DatasetId::new("summary") },
        ));
        assert_eq!(
            spec.validate(),
            Err(SpecError::ImportIntoDerivedDataset {
                component: ComponentId::new("import-summary"),
                dataset: DatasetId::new("summary"),
            })
        );
    }

    #[test]
    fn imported_dataset_has_no_dependencies() {
        let spec = derived_spec();
        assert!(spec.dataset_dependencies(&DatasetId::new("data")).is_empty());
        assert!(spec.dataset_dependencies(&DatasetId::new("undeclared")).is_empty());
    }

    #[test]
    fn derived_dataset_depends_on_inputs_filters_and_state_params() {
        let spec = derived_spec();
        let dependencies = spec.dataset_dependencies(&DatasetId::new("summary"));
        let expected: BTreeSet<Dependency> = [
            Dependency::Dataset(DatasetId::new("data")),
            Dependency::State(StateKey::new("filter")),
            Dependency::State(StateKey::new("x")),
        ]
        .into_iter()
        .collect();
        assert_eq!(dependencies, expected);
    }

    #[test]
    fn dependencies_follow_chains_of_derived_datasets() {
        let mut spec = derived_spec();
        let summary = DatasetId::new("summary");
        spec.datasets.push(slot(
            "report",
            derived(
                "report",
                &[("summary", DataView::of(&summary))],
                &[("title", ParamBinding::State(StateKey::new("y")))],
            ),
        ));
        let dependencies = spec.dataset_dependencies(&DatasetId::new("report"));
        let expected: BTreeSet<Dependency> = [
            Dependency::Dataset(summary),
            Dependency::Dataset(DatasetId::new("data")),
            Dependency::State(StateKey::new("filter")),
            Dependency::State(StateKey::new("x")),
            Dependency::State(StateKey::new("y")),
        ]
        .into_iter()
        .collect();
        assert_eq!(dependencies, expected);
    }

    #[test]
    fn dependency_resolution_terminates_on_a_cycle() {
        let mut spec = derived_spec();
        let summary = DatasetId::new("summary");
        spec.datasets[1].source = derived("f", &[("in", DataView::of(&summary))], &[]);
        let dependencies = spec.dataset_dependencies(&summary);
        assert_eq!(dependencies, BTreeSet::from([Dependency::Dataset(summary)]));
    }

    #[test]
    fn chart_depends_on_dataset_filter_and_axes() {
        let spec = sample_spec();
        let chart = spec.component(&ComponentId::new("chart"));
        let dependencies = chart.map(|chart| chart.kind.dependencies()).unwrap_or_default();
        assert_eq!(
            dependencies,
            vec![
                Dependency::Dataset(DatasetId::new("data")),
                Dependency::State(StateKey::new("filter")),
                Dependency::State(StateKey::new("x")),
                Dependency::State(StateKey::new("y")),
            ]
        );
    }
}
