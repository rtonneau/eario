//! Serializable dashboard specification.
//!
//! A [`DashboardSpec`] describes what a dashboard contains and how its parts
//! depend on each other. It holds no executable code and no reference to a
//! frontend technology: the Rust builder API produces one, the generic
//! frontend renders one, and it round-trips through JSON.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::id::{ComponentId, DatasetId, StateKey};

/// Version of the specification format written by this crate.
pub const SPEC_VERSION: u32 = 1;

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

        for component in self.components() {
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
            datasets: vec![DatasetSlot { id: data.clone(), label: "Data".to_owned() }],
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
