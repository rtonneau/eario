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
    /// Lets the user type a number.
    NumberInput {
        /// Label of the input.
        label: String,
        /// State entry receiving the number.
        state: StateKey,
        /// Smallest accepted value; any when absent.
        min: Option<f64>,
        /// Largest accepted value; any when absent.
        max: Option<f64>,
        /// Accepted values are `min` (0 when absent) plus a whole number of
        /// steps; any value when absent.
        step: Option<f64>,
        /// Accept whole numbers only.
        integer: bool,
        /// Value the state entry starts with.
        default: f64,
    },
    /// Lets the user choose a number between two bounds by dragging.
    Slider {
        /// Label of the input.
        label: String,
        /// State entry receiving the number.
        state: StateKey,
        /// Smallest value.
        min: f64,
        /// Largest value.
        max: f64,
        /// Distance between two neighbouring values, counted from `min`.
        step: f64,
        /// Accept whole numbers only.
        integer: bool,
        /// Value the state entry starts with.
        default: f64,
    },
    /// Lets the user type a line of text.
    TextInput {
        /// Label of the input.
        label: String,
        /// State entry receiving the text.
        state: StateKey,
        /// Text the state entry starts with.
        default: String,
        /// Largest number of characters accepted.
        max_length: u32,
    },
    /// Lets the user choose one value from a list.
    Select {
        /// Label of the input.
        label: String,
        /// State entry receiving the chosen value.
        state: StateKey,
        /// Where the values on offer come from.
        options: SelectOptions,
        /// Value chosen at start-up, or when the values on offer change and
        /// no longer include the current one. When absent, or not on offer,
        /// the first value is chosen.
        default: Option<String>,
    },
    /// Lets the user switch an option on or off.
    Checkbox {
        /// Label of the input.
        label: String,
        /// State entry receiving `true` or `false`.
        state: StateKey,
        /// Value the state entry starts with.
        default: bool,
    },
}

/// The values a [`ComponentKind::Select`] offers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum SelectOptions {
    /// A fixed list, in the order given.
    Static(Vec<String>),
    /// The distinct values of a column of a dataset, sorted, as text. The
    /// list follows the dataset when it changes.
    Column {
        /// Dataset holding the column.
        dataset: DatasetId,
        /// Name of the column.
        column: String,
    },
}

/// The rules a number entered in a [`ComponentKind::NumberInput`] or a
/// [`ComponentKind::Slider`] must follow. Not part of the specification:
/// see [`ComponentKind::number_constraints`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NumberConstraints {
    /// Smallest accepted value, if any.
    pub min: Option<f64>,
    /// Largest accepted value, if any.
    pub max: Option<f64>,
    /// Accepted values are `min` (0 when absent) plus a whole number of
    /// steps, if set.
    pub step: Option<f64>,
    /// Accept whole numbers only.
    pub integer: bool,
}

/// Largest magnitude up to which every whole number is an exact `f64`: 2^53.
const MAX_EXACT_INTEGER: u64 = 1 << 53;

/// `number` as an `i64`, if it is a whole number within ±2^53.
fn exact_integer(number: f64) -> Option<i64> {
    let exact = number.fract() == 0.0 && number.abs() <= MAX_EXACT_INTEGER as f64;
    // Exact: whole and within ±2^53.
    exact.then_some(number as i64)
}

impl NumberConstraints {
    /// Checks a JSON value against the rules and returns it as the state
    /// entry stores it: a whole number is stored as a JSON integer, so a
    /// computation can read it as `i64` or `u32`.
    ///
    /// An integer input accepts whole numbers within ±2^53 only, so that
    /// they compare exactly with its bounds.
    ///
    /// The error says what is wrong, in words meant for the user.
    pub fn check(&self, value: &Value) -> Result<Value, String> {
        let Some(number) = value.as_f64() else {
            return Err(format!("expected a number, got {value}"));
        };
        if !number.is_finite() {
            return Err(format!("{number} is not a finite number"));
        }
        // For an integer input, the value as an exact integer.
        let whole = if self.integer {
            if number.fract() != 0.0 {
                return Err(format!("{number} is not a whole number"));
            }
            // Read exactly when the JSON text holds an integer: `number` may
            // be rounded.
            let too_large = match value.as_i64() {
                Some(integer) => integer.unsigned_abs() > MAX_EXACT_INTEGER,
                None => number.abs() > MAX_EXACT_INTEGER as f64,
            };
            if too_large {
                return Err(format!(
                    "{value} is too large for a whole-number input, which holds numbers within ±2^53"
                ));
            }
            // Exact: `number` is whole and within ±2^53, so it fits an `i64`.
            Some(value.as_i64().unwrap_or(number as i64))
        } else {
            None
        };
        if let Some(min) = self.min
            && number < min
        {
            return Err(format!("{number} is below the minimum {min}"));
        }
        if let Some(max) = self.max
            && number > max
        {
            return Err(format!("{number} is above the maximum {max}"));
        }
        if let Some(step) = self.step {
            let base = self.min.unwrap_or(0.0);
            let on_step = match (whole, exact_integer(base), exact_integer(step)) {
                // Whole value, base and step: exact integer arithmetic. The
                // operands are within ±2^53, so `i128` cannot overflow.
                (Some(whole), Some(base), Some(step)) if step > 0 => {
                    (i128::from(whole) - i128::from(base)) % i128::from(step) == 0
                }
                _ => {
                    let nearest = base + ((number - base) / step).round() * step;
                    // The distance to the nearest accepted value, in value
                    // units, may only be the rounding of the arithmetic
                    // above: a few units of the last place of the operands,
                    // and a tiny share of a step for decimal steps such as
                    // 0.1.
                    let tolerance = 4.0 * f64::EPSILON * number.abs().max(base.abs()) + 1e-9 * step;
                    (number - nearest).abs() <= tolerance
                }
            };
            if !on_step {
                return Err(format!(
                    "{number} is not one of the accepted values, which go from {base} in steps of {step}"
                ));
            }
        }
        Ok(whole.map_or_else(|| value.clone(), Value::from))
    }

    /// Checks the rules themselves and the default value of the input.
    fn check_definition(&self, default: f64) -> Result<(), String> {
        let named = [("minimum", self.min), ("maximum", self.max), ("step", self.step)];
        for (name, bound) in named.into_iter().filter_map(|(name, bound)| Some((name, bound?))) {
            if !bound.is_finite() {
                return Err(format!("the {name} is not a finite number"));
            }
            if !self.integer {
                continue;
            }
            if bound.fract() != 0.0 {
                return Err(format!(
                    "the {name} {bound} is not a whole number, but the input accepts whole numbers only"
                ));
            }
            if bound.abs() > MAX_EXACT_INTEGER as f64 {
                return Err(format!(
                    "the {name} {bound} is beyond ±2^53, the whole numbers an input holds exactly"
                ));
            }
        }
        if let (Some(min), Some(max)) = (self.min, self.max)
            && min > max
        {
            return Err(format!("the minimum {min} is above the maximum {max}"));
        }
        if let Some(step) = self.step
            && step <= 0.0
        {
            return Err(format!("the step {step} is not positive"));
        }
        self.check(&Value::from(default))
            .map(drop)
            .map_err(|reason| format!("the default {reason}"))
    }
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
            Self::Text { .. }
            | Self::NumberInput { .. }
            | Self::Slider { .. }
            | Self::TextInput { .. }
            | Self::Checkbox { .. } => Vec::new(),
            Self::Select { options, .. } => match options {
                SelectOptions::Static(_) => Vec::new(),
                SelectOptions::Column { dataset, .. } => vec![Dependency::Dataset(dataset.clone())],
            },
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
            Self::ColumnSelect { state, .. }
            | Self::FilterInput { state, .. }
            | Self::NumberInput { state, .. }
            | Self::Slider { state, .. }
            | Self::TextInput { state, .. }
            | Self::Select { state, .. }
            | Self::Checkbox { state, .. } => Some(state),
            Self::Text { .. }
            | Self::FileImport { .. }
            | Self::DatasetInfo { .. }
            | Self::DataTable { .. }
            | Self::Chart { .. }
            | Self::Statistics { .. } => None,
        }
    }

    /// The value the state entry written by an input starts with: its
    /// default. It is `null` for a column selection, a filter input and a
    /// select offering the values of a column, which get a value when their
    /// dataset is loaded, and for components that write no state.
    ///
    /// The kind is expected to have passed [`DashboardSpec::validate`].
    pub fn initial_state(&self) -> Value {
        match self {
            Self::NumberInput { integer: true, default, .. }
            | Self::Slider { integer: true, default, .. } => {
                // Exact once validated: the default is whole and within ±2^53.
                Value::from(*default as i64)
            }
            Self::NumberInput { default, .. } | Self::Slider { default, .. } => {
                Value::from(*default)
            }
            Self::TextInput { default, .. } => Value::String(default.clone()),
            Self::Checkbox { default, .. } => Value::Bool(*default),
            Self::Select { options: SelectOptions::Static(values), default, .. } => default
                .as_ref()
                .filter(|default| values.contains(default))
                .or_else(|| values.first())
                .map_or(Value::Null, |value| Value::String(value.clone())),
            _ => Value::Null,
        }
    }

    /// The rules a number entered in a number input or a slider follows;
    /// `None` for other components.
    pub fn number_constraints(&self) -> Option<NumberConstraints> {
        match self {
            Self::NumberInput { min, max, step, integer, .. } => {
                Some(NumberConstraints { min: *min, max: *max, step: *step, integer: *integer })
            }
            Self::Slider { min, max, step, integer, .. } => Some(NumberConstraints {
                min: Some(*min),
                max: Some(*max),
                step: Some(*step),
                integer: *integer,
            }),
            _ => None,
        }
    }

    /// Checks the definition of an input: its bounds, its default and its
    /// options. The error says what is wrong.
    fn check_input(&self) -> Result<(), String> {
        match self {
            Self::NumberInput { default, .. } | Self::Slider { default, .. } => {
                match self.number_constraints() {
                    Some(constraints) => constraints.check_definition(*default),
                    None => Ok(()),
                }
            }
            Self::TextInput { default, max_length, .. } => {
                if *max_length == 0 {
                    return Err("the maximum length is zero".to_owned());
                }
                let length = default.chars().count();
                if length > *max_length as usize {
                    return Err(format!(
                        "the default has {length} characters, more than the maximum length {max_length}"
                    ));
                }
                Ok(())
            }
            Self::Select { options: SelectOptions::Static(values), default, .. } => {
                if values.is_empty() {
                    return Err("the list of options is empty".to_owned());
                }
                let mut seen = BTreeSet::new();
                if let Some(duplicate) = values.iter().find(|value| !seen.insert(value.as_str())) {
                    return Err(format!("option `{duplicate}` is listed more than once"));
                }
                match default {
                    Some(default) if !values.contains(default) => {
                        Err(format!("the default `{default}` is not one of the options"))
                    }
                    _ => Ok(()),
                }
            }
            _ => Ok(()),
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
    /// An input is defined inconsistently: bounds in the wrong order, a
    /// default it would reject, an empty list of options.
    #[error("input `{component}` is not valid: {reason}")]
    InvalidInput {
        /// The input.
        component: ComponentId,
        /// What is wrong.
        reason: String,
    },
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
            component.kind.check_input().map_err(|reason| SpecError::InvalidInput {
                component: component.id.clone(),
                reason,
            })?;
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

    fn bins() -> ComponentKind {
        ComponentKind::NumberInput {
            label: "Bins".to_owned(),
            state: StateKey::new("bins"),
            min: Some(1.0),
            max: Some(200.0),
            step: Some(1.0),
            integer: true,
            default: 20.0,
        }
    }

    fn window() -> ComponentKind {
        ComponentKind::Slider {
            label: "Window".to_owned(),
            state: StateKey::new("window"),
            min: 0.0,
            max: 1.0,
            step: 0.1,
            integer: false,
            default: 0.3,
        }
    }

    fn group(options: SelectOptions, default: Option<&str>) -> ComponentKind {
        ComponentKind::Select {
            label: "Group".to_owned(),
            state: StateKey::new("group"),
            options,
            default: default.map(str::to_owned),
        }
    }

    fn colours() -> SelectOptions {
        SelectOptions::Static(vec!["red".to_owned(), "green".to_owned()])
    }

    /// `sample_spec` plus one component of `kind`, named `input`.
    fn with_input(kind: ComponentKind) -> DashboardSpec {
        let mut spec = sample_spec();
        spec.pages[0].rows[0].items.push(component("input", kind));
        spec
    }

    fn input_rejection(kind: ComponentKind) -> String {
        match with_input(kind).validate() {
            Err(SpecError::InvalidInput { component, reason }) if component.as_str() == "input" => {
                reason
            }
            other => format!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn inputs_start_with_their_default_and_write_their_state() {
        let text = ComponentKind::TextInput {
            label: "Title".to_owned(),
            state: StateKey::new("title"),
            default: "Run 1".to_owned(),
            max_length: 40,
        };
        let outliers = ComponentKind::Checkbox {
            label: "Outliers".to_owned(),
            state: StateKey::new("outliers"),
            default: true,
        };
        let cases = [
            (bins(), "bins", serde_json::json!(20)),
            (window(), "window", serde_json::json!(0.3)),
            (text, "title", serde_json::json!("Run 1")),
            (outliers, "outliers", serde_json::json!(true)),
            (group(colours(), Some("green")), "group", serde_json::json!("green")),
            (group(colours(), None), "group", serde_json::json!("red")),
        ];
        for (kind, state, initial) in cases {
            assert_eq!(kind.written_state().map(StateKey::as_str), Some(state));
            assert_eq!(kind.initial_state(), initial, "{state}");
            assert!(kind.dependencies().is_empty(), "{state}");
            assert_eq!(with_input(kind).validate(), Ok(()), "{state}");
        }
    }

    #[test]
    fn select_from_a_column_depends_on_its_dataset_and_starts_unset() {
        let kind = group(
            SelectOptions::Column { dataset: DatasetId::new("data"), column: "sample".to_owned() },
            None,
        );
        assert_eq!(kind.dependencies(), [Dependency::Dataset(DatasetId::new("data"))]);
        assert_eq!(kind.initial_state(), Value::Null);
        assert_eq!(with_input(kind.clone()).validate(), Ok(()));

        let mut spec = with_input(kind);
        spec.datasets.clear();
        assert!(matches!(spec.validate(), Err(SpecError::UnknownDataset { .. })));
    }

    #[test]
    fn select_options_are_tagged() -> Result<(), serde_json::Error> {
        assert_eq!(
            serde_json::to_value(colours())?,
            serde_json::json!({ "type": "static", "value": ["red", "green"] })
        );
        let column =
            SelectOptions::Column { dataset: DatasetId::new("data"), column: "sample".to_owned() };
        assert_eq!(
            serde_json::to_value(&column)?,
            serde_json::json!({ "type": "column", "value": { "dataset": "data", "column": "sample" } })
        );
        let spec = with_input(group(column, Some("Si")));
        let restored: DashboardSpec = serde_json::from_str(&serde_json::to_string(&spec)?)?;
        assert_eq!(restored, spec);
        Ok(())
    }

    #[test]
    fn inconsistent_input_definitions_are_rejected_with_a_reason() {
        let number = |min: f64, max: f64, step: Option<f64>, integer: bool, default: f64| {
            ComponentKind::NumberInput {
                label: "N".to_owned(),
                state: StateKey::new("n"),
                min: Some(min),
                max: Some(max),
                step,
                integer,
                default,
            }
        };
        let cases = [
            (number(5.0, 1.0, None, false, 3.0), "the minimum 5 is above the maximum 1"),
            (number(1.0, 200.0, None, false, 300.0), "the default 300 is above the maximum 200"),
            (number(0.0, 1.0, Some(0.0), false, 0.0), "the step 0 is not positive"),
            (number(0.5, 10.0, None, true, 1.0), "the minimum 0.5 is not a whole number"),
            (number(0.0, 10.0, Some(2.0), true, 3.0), "the default 3 is not one of the accepted"),
            (number(0.0, f64::INFINITY, None, false, 1.0), "the maximum is not a finite number"),
            (
                number(-1e16, 10.0, None, true, 1.0),
                "the minimum -10000000000000000 is beyond ±2^53, the whole numbers an input holds exactly",
            ),
            (
                ComponentKind::TextInput {
                    label: "T".to_owned(),
                    state: StateKey::new("t"),
                    default: "too long".to_owned(),
                    max_length: 3,
                },
                "the default has 8 characters, more than the maximum length 3",
            ),
            (
                ComponentKind::TextInput {
                    label: "T".to_owned(),
                    state: StateKey::new("t"),
                    default: String::new(),
                    max_length: 0,
                },
                "the maximum length is zero",
            ),
            (group(SelectOptions::Static(Vec::new()), None), "the list of options is empty"),
            (group(colours(), Some("blue")), "the default `blue` is not one of the options"),
            (
                group(SelectOptions::Static(vec!["a".to_owned(), "a".to_owned()]), None),
                "option `a` is listed more than once",
            ),
        ];
        for (kind, expected) in cases {
            let reason = input_rejection(kind);
            assert!(reason.starts_with(expected), "{reason}");
        }
        let message =
            SpecError::InvalidInput { component: ComponentId::new("bins"), reason: "x".to_owned() };
        assert_eq!(message.to_string(), "input `bins` is not valid: x");
    }

    #[test]
    fn number_constraints_check_range_step_and_integrality() {
        let constraints = bins().number_constraints();
        let check = |value: Value| constraints.map(|constraints| constraints.check(&value));
        assert_eq!(check(serde_json::json!(30)), Some(Ok(serde_json::json!(30))));
        assert_eq!(check(serde_json::json!(30.0)), Some(Ok(serde_json::json!(30))), "normalized");
        assert_eq!(
            check(serde_json::json!(2.5)),
            Some(Err("2.5 is not a whole number".to_owned()))
        );
        assert_eq!(check(serde_json::json!(0)), Some(Err("0 is below the minimum 1".to_owned())));
        assert_eq!(
            check(serde_json::json!(201)),
            Some(Err("201 is above the maximum 200".to_owned()))
        );
        assert_eq!(
            check(serde_json::json!("20")),
            Some(Err(r#"expected a number, got "20""#.to_owned()))
        );

        let whole = NumberConstraints { min: None, max: None, step: None, integer: true };
        assert_eq!(
            whole.check(&serde_json::json!(1.5)),
            Err("1.5 is not a whole number".to_owned())
        );
        let exact = 1_i64 << 53;
        assert_eq!(whole.check(&serde_json::json!(exact)), Ok(serde_json::json!(exact)));
        assert_eq!(whole.check(&serde_json::json!(-exact)), Ok(serde_json::json!(-exact)));
        for beyond in
            [serde_json::json!(exact + 1), serde_json::json!(i64::MAX), serde_json::json!(u64::MAX)]
        {
            let error = whole.check(&beyond).err().unwrap_or_default();
            assert!(error.contains("is too large for a whole-number input"), "{beyond}: {error}");
        }
        assert!(whole.check(&serde_json::json!(1e300)).is_err());
        // 2^53 + 1 reads as 2^53 in `f64`: without the check it would pass
        // a maximum of 2^53.
        let bounded = NumberConstraints { max: Some(exact as f64), ..whole };
        assert!(bounded.check(&serde_json::json!(exact + 1)).is_err());

        let decimal = window().number_constraints();
        let check = |value: f64| decimal.map(|decimal| decimal.check(&Value::from(value)).is_ok());
        assert_eq!(check(0.7), Some(true), "0.7 is 7 steps of 0.1 despite rounding");
        assert_eq!(check(0.35), Some(false));

        // The tolerance is in value units, not in steps.
        let units = NumberConstraints { min: None, max: None, step: Some(1.0), integer: false };
        assert!(units.check(&serde_json::json!(600_000_000.5)).is_err());
        assert!(units.check(&serde_json::json!(600_000_000.0)).is_ok());
        let tenths = NumberConstraints { min: Some(1e9), step: Some(0.1), ..units };
        assert!(tenths.check(&serde_json::json!(1_000_000_000.3)).is_ok());
        assert!(tenths.check(&serde_json::json!(1_000_000_000.35)).is_err());

        // Integer inputs check the step exactly, whatever its size.
        let integer = |step: f64| NumberConstraints {
            min: Some(0.0),
            max: None,
            step: Some(step),
            integer: true,
        };
        assert!(integer(1e9).check(&serde_json::json!(3_000_000_001_i64)).is_err());
        assert!(integer(1e9).check(&serde_json::json!(3_000_000_000_i64)).is_ok());
        assert!(integer(3.0).check(&serde_json::json!(9_007_199_254_740_991_i64)).is_err());
        assert_eq!(
            integer(3.0).check(&serde_json::json!(9_007_199_254_740_990_i64)),
            Ok(serde_json::json!(9_007_199_254_740_990_i64))
        );
        assert_eq!(bins().number_constraints().map(|c| c.integer), Some(true));
        assert_eq!(ComponentKind::Text { text: String::new() }.number_constraints(), None);
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
