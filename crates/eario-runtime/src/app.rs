//! Application services.
//!
//! An [`App`] owns one dashboard, its datasets and its state. Every client,
//! whether the HTTP API or the MCP server, goes through the methods below, so
//! behaviour is defined in one place.
//!
//! The methods are synchronous and may be CPU-intensive. Asynchronous callers
//! run them on a blocking thread.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use eario_core::{
    AppInfo, ColumnKind, ComponentData, ComponentId, ComponentKind, DashboardSpec, DataView,
    DatasetId, DatasetInfo, Dependency, Filter, Preferences, StateKey, StateMap, StateUpdate,
};
use eario_data::{DataFrame, Dataset, ReaderRegistry, query};
use serde_json::Value;

use crate::error::AppError;
use crate::preferences::PreferenceStore;

/// Settings of an [`App`] that are not part of the dashboard specification.
#[derive(Default)]
pub struct AppOptions {
    /// Readers used to import files.
    pub readers: ReaderRegistry,
    /// File the user preferences are stored in. `None` keeps them in memory.
    pub preferences_path: Option<PathBuf>,
}

/// A running dashboard application. Cloning it yields another handle to the
/// same application.
#[derive(Clone)]
pub struct App {
    inner: Arc<Inner>,
}

struct Inner {
    spec: DashboardSpec,
    readers: ReaderRegistry,
    preferences: PreferenceStore,
    session: RwLock<Session>,
}

/// The mutable part of an application.
#[derive(Default)]
struct Session {
    state: StateMap,
    revision: u64,
    datasets: BTreeMap<DatasetId, Dataset>,
}

impl App {
    /// Creates an application from a dashboard specification.
    ///
    /// Every state entry starts as `null` and every dataset slot starts empty.
    pub fn new(spec: DashboardSpec, options: AppOptions) -> Result<Self, AppError> {
        spec.validate()?;
        let state = spec
            .components()
            .filter_map(|component| component.kind.written_state())
            .map(|key| (key.clone(), Value::Null))
            .collect();
        let session = Session { state, ..Session::default() };
        Ok(Self {
            inner: Arc::new(Inner {
                spec,
                readers: options.readers,
                preferences: PreferenceStore::open(options.preferences_path),
                session: RwLock::new(session),
            }),
        })
    }

    fn read(&self) -> RwLockReadGuard<'_, Session> {
        self.inner.session.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write(&self) -> RwLockWriteGuard<'_, Session> {
        self.inner.session.write().unwrap_or_else(PoisonError::into_inner)
    }

    /// The dashboard specification.
    pub fn spec(&self) -> &DashboardSpec {
        &self.inner.spec
    }

    /// Everything a client needs to render the dashboard.
    pub fn info(&self) -> AppInfo {
        let session = self.read();
        AppInfo {
            eario_version: env!("CARGO_PKG_VERSION").to_owned(),
            spec: self.inner.spec.clone(),
            state: session.state.clone(),
            revision: session.revision,
            preferences: self.inner.preferences.get(),
        }
    }

    /// Describes the datasets currently loaded.
    pub fn datasets(&self) -> Vec<DatasetInfo> {
        self.read().datasets.iter().map(|(id, dataset)| dataset.info(id)).collect()
    }

    /// Returns the table loaded into a slot.
    pub fn dataset_frame(&self, id: &DatasetId) -> Result<DataFrame, AppError> {
        self.check_slot(id)?;
        match self.read().datasets.get(id) {
            Some(dataset) => Ok(dataset.frame().clone()),
            None => Err(AppError::DatasetNotLoaded(id.clone())),
        }
    }

    /// The stored user preferences.
    pub fn preferences(&self) -> Preferences {
        self.inner.preferences.get()
    }

    /// Stores the user preferences.
    pub fn set_preferences(&self, preferences: Preferences) -> Result<(), AppError> {
        self.inner.preferences.set(preferences)
    }

    fn check_slot(&self, id: &DatasetId) -> Result<(), AppError> {
        if self.inner.spec.datasets.iter().any(|slot| &slot.id == id) {
            Ok(())
        } else {
            Err(AppError::UnknownDataset(id.clone()))
        }
    }

    /// Imports a file into a dataset slot, replacing what it held.
    ///
    /// `name` is the file name; its extension selects the reader.
    pub fn import_dataset(
        &self,
        id: &DatasetId,
        name: &str,
        bytes: &[u8],
    ) -> Result<StateUpdate, AppError> {
        self.check_slot(id)?;
        let dataset = Dataset::import(&self.inner.readers, name, bytes)?;
        tracing::info!(dataset = %id, source = name, rows = dataset.frame().height(), "dataset imported");
        Ok(self.install(id, dataset))
    }

    /// Loads a table built by the application into a dataset slot.
    pub fn load_dataset(&self, id: &DatasetId, dataset: Dataset) -> Result<StateUpdate, AppError> {
        self.check_slot(id)?;
        Ok(self.install(id, dataset))
    }

    /// Loads another worksheet of the workbook held by a dataset slot.
    pub fn select_sheet(&self, id: &DatasetId, sheet: &str) -> Result<StateUpdate, AppError> {
        self.check_slot(id)?;
        let current = self.read().datasets.get(id).cloned();
        let Some(current) = current else {
            return Err(AppError::DatasetNotLoaded(id.clone()));
        };
        let dataset = current.with_sheet(&self.inner.readers, sheet)?;
        Ok(self.install(id, dataset))
    }

    /// Puts a dataset into its slot and brings the state in line with it.
    fn install(&self, id: &DatasetId, dataset: Dataset) -> StateUpdate {
        let mut session = self.write();
        let mut changed = vec![Dependency::Dataset(id.clone())];
        for key in self.reconcile_state(id, &dataset, &mut session.state) {
            changed.push(Dependency::State(key));
        }
        session.datasets.insert(id.clone(), dataset);
        session.revision += 1;
        StateUpdate {
            revision: session.revision,
            state: session.state.clone(),
            invalidated: self.invalidated_by(&changed),
        }
    }

    /// Resets the inputs bound to a dataset that has just been replaced:
    /// column selections that no longer name a column fall back to their
    /// default, and filters are cleared. Returns the entries that changed.
    fn reconcile_state(
        &self,
        id: &DatasetId,
        dataset: &Dataset,
        state: &mut StateMap,
    ) -> Vec<StateKey> {
        let mut changed = Vec::new();
        for component in self.inner.spec.components() {
            let (key, value) = match &component.kind {
                ComponentKind::ColumnSelect {
                    dataset: bound,
                    state: key,
                    numeric_only,
                    default_index,
                    ..
                } if bound == id => {
                    let options = column_options(dataset.frame(), *numeric_only);
                    let current = state.get(key).and_then(Value::as_str);
                    if current.is_some_and(|current| options.iter().any(|name| name == current)) {
                        continue;
                    }
                    let fallback = options.get(*default_index as usize).or_else(|| options.first());
                    (key, fallback.map_or(Value::Null, |name| Value::String(name.clone())))
                }
                ComponentKind::FilterInput { dataset: bound, state: key } if bound == id => {
                    (key, Value::Null)
                }
                _ => continue,
            };
            if state.get(key) != Some(&value) {
                state.insert(key.clone(), value);
                changed.push(key.clone());
            }
        }
        changed
    }

    /// Lists the components reading at least one of the changed things.
    fn invalidated_by(&self, changed: &[Dependency]) -> Vec<ComponentId> {
        self.inner
            .spec
            .components()
            .filter(|component| {
                component.kind.dependencies().iter().any(|dependency| changed.contains(dependency))
            })
            .map(|component| component.id.clone())
            .collect()
    }

    /// Sets one state entry, as an input component does.
    ///
    /// The value is checked against the input that owns the entry: a column
    /// selection accepts `null` or the name of an offered column; a filter
    /// input accepts `null` or a [`Filter`] applicable to the loaded dataset.
    pub fn set_state(&self, key: &StateKey, value: Value) -> Result<StateUpdate, AppError> {
        let writer = self
            .inner
            .spec
            .components()
            .find(|component| component.kind.written_state() == Some(key))
            .ok_or_else(|| AppError::UnknownState(key.clone()))?;

        let mut session = self.write();
        let invalid = |message: String| AppError::InvalidStateValue { state: key.clone(), message };
        match &writer.kind {
            ComponentKind::ColumnSelect { dataset, numeric_only, .. } => match &value {
                Value::Null => {}
                Value::String(column) => {
                    if let Some(loaded) = session.datasets.get(dataset) {
                        let options = column_options(loaded.frame(), *numeric_only);
                        if !options.contains(column) {
                            return Err(invalid(format!("`{column}` is not an available column")));
                        }
                    }
                }
                _ => return Err(invalid("expected a column name or null".to_owned())),
            },
            ComponentKind::FilterInput { dataset, .. } => {
                if !value.is_null() {
                    let filter: Filter = serde_json::from_value(value.clone())
                        .map_err(|error| invalid(format!("not a filter: {error}")))?;
                    if let Some(loaded) = session.datasets.get(dataset) {
                        // Applying the filter to one row checks the column and
                        // the value type without scanning the table.
                        query::apply_filter(&loaded.frame().slice(0, 1), &filter)?;
                    }
                }
            }
            _ => return Err(AppError::UnknownState(key.clone())),
        }

        let invalidated = if session.state.get(key) == Some(&value) {
            Vec::new()
        } else {
            session.state.insert(key.clone(), value);
            session.revision += 1;
            self.invalidated_by(&[Dependency::State(key.clone())])
        };
        Ok(StateUpdate { revision: session.revision, state: session.state.clone(), invalidated })
    }

    /// Computes the data of one component from the current state.
    ///
    /// `offset` is the first row of the requested page; only tables use it.
    pub fn component_data(&self, id: &ComponentId, offset: u32) -> Result<ComponentData, AppError> {
        let component =
            self.inner.spec.component(id).ok_or_else(|| AppError::UnknownComponent(id.clone()))?;

        let no_dataset = || ComponentData::Empty { reason: "No dataset loaded yet.".to_owned() };
        match &component.kind {
            ComponentKind::Text { .. } => Ok(ComponentData::Text),
            ComponentKind::FileImport { dataset } | ComponentKind::DatasetInfo { dataset } => {
                let info = self.read().datasets.get(dataset).map(|loaded| loaded.info(dataset));
                Ok(info.map_or_else(no_dataset, |info| ComponentData::Dataset { info }))
            }
            ComponentKind::ColumnSelect { dataset, numeric_only, .. } => {
                let frame = self.read().datasets.get(dataset).map(|loaded| loaded.frame().clone());
                Ok(frame.map_or_else(no_dataset, |frame| ComponentData::ColumnSelect {
                    options: column_options(&frame, *numeric_only),
                }))
            }
            ComponentKind::FilterInput { dataset, .. } => {
                let frame = self.read().datasets.get(dataset).map(|loaded| loaded.frame().clone());
                Ok(frame.map_or_else(no_dataset, |frame| ComponentData::FilterInput {
                    columns: query::column_infos(&frame),
                }))
            }
            ComponentKind::DataTable { view, page_size } => {
                let Some(rows) = self.resolve_view(view)? else {
                    return Ok(no_dataset());
                };
                let table =
                    query::table_page(&rows.frame, offset, *page_size, rows.unfiltered_rows)?;
                Ok(ComponentData::DataTable { table })
            }
            ComponentKind::Statistics { view } => {
                let Some(rows) = self.resolve_view(view)? else {
                    return Ok(no_dataset());
                };
                Ok(ComponentData::Statistics { stats: query::statistics(&rows.frame)? })
            }
            ComponentKind::Chart { view, mark, x, y, max_points } => {
                let (x_column, y_column) = {
                    let session = self.read();
                    let column = |key: &StateKey| {
                        session.state.get(key).and_then(Value::as_str).map(str::to_owned)
                    };
                    (column(x), column(y))
                };
                let Some(rows) = self.resolve_view(view)? else {
                    return Ok(no_dataset());
                };
                let (Some(x_column), Some(y_column)) = (x_column, y_column) else {
                    return Ok(ComponentData::Empty {
                        reason: "Choose two numeric columns to plot.".to_owned(),
                    });
                };
                let chart =
                    query::chart_data(&rows.frame, *mark, &x_column, &y_column, *max_points)?;
                Ok(ComponentData::Chart { chart })
            }
        }
    }

    /// Produces the rows of a view, or `None` when its dataset is not loaded.
    ///
    /// The session lock is released before the filter runs.
    fn resolve_view(&self, view: &DataView) -> Result<Option<ViewRows>, AppError> {
        let (frame, filter) = {
            let session = self.read();
            let Some(dataset) = session.datasets.get(&view.dataset) else {
                return Ok(None);
            };
            let filter = match view.filter.as_ref().and_then(|key| session.state.get(key)) {
                None | Some(Value::Null) => None,
                Some(value) => Some(serde_json::from_value::<Filter>(value.clone())),
            };
            (dataset.frame().clone(), filter)
        };
        let unfiltered_rows = query::row_count(&frame);
        let frame = match filter {
            None => frame,
            Some(Ok(filter)) => query::apply_filter(&frame, &filter)?,
            Some(Err(error)) => {
                return Err(AppError::Data(eario_data::DataError::InvalidFilter(
                    error.to_string(),
                )));
            }
        };
        Ok(Some(ViewRows { frame, unfiltered_rows }))
    }
}

/// The rows of a view, after filtering.
struct ViewRows {
    frame: DataFrame,
    unfiltered_rows: u32,
}

/// Names of the columns a column selection offers.
fn column_options(frame: &DataFrame, numeric_only: bool) -> Vec<String> {
    query::column_infos(frame)
        .into_iter()
        .filter(|column| !numeric_only || column.kind == ColumnKind::Numeric)
        .map(|column| column.name)
        .collect()
}
