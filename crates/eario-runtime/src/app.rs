//! Application services.
//!
//! An [`App`] owns one dashboard, its datasets and its state. Every client,
//! whether the HTTP API or the MCP server, goes through the methods below, so
//! behaviour is defined in one place.
//!
//! A derived dataset is computed when something reads it and the result is
//! cached until one of the imported datasets or state entries it depends on
//! changes. The exception is a derived dataset with inputs bound to it (a
//! column selection, a filter input or a select offering the values of one
//! of its columns): it is recomputed during the change,
//! so those inputs can be reconciled with its new table before the change
//! returns.
//!
//! The methods are synchronous and may be CPU-intensive. Asynchronous callers
//! run them on a blocking thread.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use eario_core::{
    AppInfo, ColumnKind, ComponentData, ComponentId, ComponentKind, DashboardSpec, DataView,
    DatasetId, DatasetInfo, Dependency, Filter, ParamBinding, Preferences, SelectOptions, StateKey,
    StateMap, StateUpdate,
};
use eario_data::{ComputationRegistry, ComputeError, DataFrame, Dataset, ReaderRegistry, query};
use serde_json::Value;

use crate::compute::{self, CacheEntry, DerivedSlot, Revisions};
use crate::error::AppError;
use crate::preferences::PreferenceStore;

/// Reason shown by components whose dataset has nothing loaded yet.
const NOT_COMPUTED: &str =
    "this derived dataset is computed when the dashboard displays it, not on request.";
const NO_DATASET: &str = "No dataset loaded yet.";

/// Largest number of values a select offering the distinct values of a
/// column offers. The data of the select says when there are more.
pub const MAX_SELECT_OPTIONS: usize = 1_000;

/// Settings of an [`App`] that are not part of the dashboard specification.
#[derive(Default)]
pub struct AppOptions {
    /// Readers used to import files.
    pub readers: ReaderRegistry,
    /// Computations the derived datasets of the dashboard refer to by name.
    pub computations: ComputationRegistry,
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
    /// The derived datasets, every input before the datasets reading it.
    derived: Vec<DerivedSlot>,
    preferences: PreferenceStore,
    session: RwLock<Session>,
}

/// The mutable part of an application.
#[derive(Default)]
struct Session {
    state: StateMap,
    /// Number of changes since start-up, reported to clients.
    revision: u64,
    /// Changes of each imported dataset and state entry, the key of `cache`.
    revisions: Revisions,
    /// Imported datasets.
    datasets: BTreeMap<DatasetId, Dataset>,
    /// The latest table of each derived dataset that was computed.
    cache: BTreeMap<DatasetId, CacheEntry>,
}

impl Session {
    /// Records that an imported dataset or a state entry changed.
    fn touch(&mut self, dependency: Dependency) {
        *self.revisions.entry(dependency).or_default() += 1;
    }
}

/// What reading a dataset yields: a value, or why there is none yet, in
/// words meant for the user.
enum Outcome<T> {
    Ready(T),
    Empty(String),
}

impl<T> Outcome<T> {
    fn no_dataset() -> Self {
        Self::Empty(NO_DATASET.to_owned())
    }

    fn map<U>(self, convert: impl FnOnce(T) -> U) -> Outcome<U> {
        match self {
            Self::Ready(value) => Outcome::Ready(convert(value)),
            Self::Empty(reason) => Outcome::Empty(reason),
        }
    }

    /// Turns a value into component data; no value becomes
    /// [`ComponentData::Empty`] with its reason.
    fn show(
        self,
        convert: impl FnOnce(T) -> Result<ComponentData, AppError>,
    ) -> Result<ComponentData, AppError> {
        match self {
            Self::Ready(value) => convert(value),
            Self::Empty(reason) => Ok(ComponentData::Empty { reason }),
        }
    }
}

/// Whether reading a derived dataset may compute it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Access {
    /// Compute a derived dataset whose cached table is not up to date.
    Compute,
    /// Read the cached table only; a derived dataset without one is empty.
    CachedOnly,
}

/// What reconciliation does with a filter input whose dataset changed.
#[derive(Clone, Copy)]
enum Filters {
    /// Clear it: the table comes from a new source.
    Clear,
    /// Keep it while it still applies: the table was recomputed from the
    /// same sources.
    KeepApplicable,
}

impl App {
    /// Creates an application from a dashboard specification.
    ///
    /// Every state entry starts with the default of the input writing it
    /// (see [`ComponentKind::initial_state`]) and every imported dataset
    /// slot starts empty. Each derived dataset must name a computation of
    /// `options.computations` and bind exactly the inputs and parameters it
    /// declares. Derived datasets with inputs bound to them are computed
    /// now if they read no imported dataset, so those inputs start
    /// reconciled.
    pub fn new(spec: DashboardSpec, options: AppOptions) -> Result<Self, AppError> {
        spec.validate()?;
        let derived = compute::derived_slots(&spec, &options.computations)?;
        let state = spec
            .components()
            .filter_map(|component| {
                let key = component.kind.written_state()?;
                Some((key.clone(), component.kind.initial_state()))
            })
            .collect();
        let session = Session { state, ..Session::default() };
        let app = Self {
            inner: Arc::new(Inner {
                spec,
                readers: options.readers,
                derived,
                preferences: PreferenceStore::open(options.preferences_path),
                session: RwLock::new(session),
            }),
        };
        for slot in app.inner.derived.iter().filter(|slot| app.has_bound_inputs(&slot.id)) {
            app.reconcile_derived(slot);
        }
        // What this reconciliation set is the initial state, not a change.
        app.write().revision = 0;
        Ok(app)
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

    /// Describes the imported datasets currently loaded and every derived
    /// dataset, in declaration order. Nothing is computed: a derived dataset
    /// whose table is not computed for the current state is described as
    /// such, without rows or columns.
    pub fn datasets(&self) -> Vec<DatasetInfo> {
        let session = self.read();
        self.inner
            .spec
            .datasets
            .iter()
            .filter_map(|slot| match self.derived_slot(&slot.id) {
                Some(derived) => Some(derived.info(self.current_frame(&session, &slot.id))),
                None => session.datasets.get(&slot.id).map(|dataset| dataset.info(&slot.id)),
            })
            .collect()
    }

    /// Returns the table of a slot, unfiltered. A derived dataset is
    /// computed if its cached table is not up to date.
    pub fn dataset_frame(&self, id: &DatasetId) -> Result<DataFrame, AppError> {
        self.check_slot(id)?;
        match self.slot_table(id, Access::Compute)? {
            Outcome::Ready(frame) => Ok(frame),
            Outcome::Empty(_) => Err(AppError::DatasetNotLoaded(id.clone())),
        }
    }

    /// Returns the table of a slot, unfiltered, without computing anything.
    ///
    /// For a derived dataset this is the cached table; when it is not
    /// computed for the current state the result is
    /// [`AppError::NotComputed`]. Agents read through this method, so a
    /// read never runs a computation.
    pub fn cached_dataset_frame(&self, id: &DatasetId) -> Result<DataFrame, AppError> {
        self.check_slot(id)?;
        match self.slot_table(id, Access::CachedOnly)? {
            Outcome::Ready(frame) => Ok(frame),
            Outcome::Empty(_) if self.derived_slot(id).is_some() => {
                Err(AppError::NotComputed(id.clone()))
            }
            Outcome::Empty(_) => Err(AppError::DatasetNotLoaded(id.clone())),
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

    /// Accepts slots a table can be imported or loaded into: declared and
    /// not derived.
    fn check_loadable(&self, id: &DatasetId) -> Result<(), AppError> {
        self.check_slot(id)?;
        match self.derived_slot(id) {
            Some(_) => Err(AppError::DerivedDataset(id.clone())),
            None => Ok(()),
        }
    }

    fn derived_slot(&self, id: &DatasetId) -> Option<&DerivedSlot> {
        self.inner.derived.iter().find(|slot| &slot.id == id)
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
        self.check_loadable(id)?;
        let dataset = Dataset::import(&self.inner.readers, name, bytes)?;
        tracing::info!(dataset = %id, source = name, rows = dataset.frame().height(), "dataset imported");
        Ok(self.install(id, dataset))
    }

    /// Loads a table built by the application into a dataset slot.
    pub fn load_dataset(&self, id: &DatasetId, dataset: Dataset) -> Result<StateUpdate, AppError> {
        self.check_loadable(id)?;
        Ok(self.install(id, dataset))
    }

    /// Loads another worksheet of the workbook held by a dataset slot.
    pub fn select_sheet(&self, id: &DatasetId, sheet: &str) -> Result<StateUpdate, AppError> {
        self.check_loadable(id)?;
        let current = self.read().datasets.get(id).cloned();
        let Some(current) = current else {
            return Err(AppError::DatasetNotLoaded(id.clone()));
        };
        let dataset = current.with_sheet(&self.inner.readers, sheet)?;
        Ok(self.install(id, dataset))
    }

    /// Puts a dataset into its slot and brings the state in line with it.
    fn install(&self, id: &DatasetId, dataset: Dataset) -> StateUpdate {
        // Listed before the lock is taken: the table is the one installed.
        let offered = self.offered_values(id, dataset.frame());
        let changed = {
            let mut session = self.write();
            let mut changed = vec![Dependency::Dataset(id.clone())];
            let reconciled = self.reconcile_state(
                id,
                dataset.frame(),
                &offered,
                &mut session.state,
                Filters::Clear,
            );
            changed.extend(reconciled.into_iter().map(Dependency::State));
            session.datasets.insert(id.clone(), dataset);
            for dependency in &changed {
                session.touch(dependency.clone());
            }
            session.revision += 1;
            changed
        };
        let changed = self.refresh_derived(changed);
        self.update(&changed)
    }

    /// The state after a change, with the components it made stale.
    fn update(&self, changed: &[Dependency]) -> StateUpdate {
        let session = self.read();
        StateUpdate {
            revision: session.revision,
            state: session.state.clone(),
            invalidated: self.invalidated_by(changed),
        }
    }

    /// Whether a column selection, a filter input or a select offering the
    /// values of a column is bound to the dataset. These are the inputs
    /// `reconcile_state` reconciles.
    fn has_bound_inputs(&self, id: &DatasetId) -> bool {
        compute::bound_states(&self.inner.spec, id).next().is_some()
    }

    /// Resets the inputs bound to a dataset whose table has just been
    /// replaced: column selections that no longer name a column, and selects
    /// whose value is no longer on offer, fall back to their default; filters
    /// are cleared or kept as `filters` says. `offered` holds what
    /// [`offered_values`](Self::offered_values) lists for `frame`. Returns
    /// the entries that changed.
    ///
    /// Runs under the session lock, so it only does cheap work: the values
    /// of the selects, which take a scan of a column, are listed beforehand.
    fn reconcile_state(
        &self,
        id: &DatasetId,
        frame: &DataFrame,
        offered: &BTreeMap<StateKey, Vec<String>>,
        state: &mut StateMap,
        filters: Filters,
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
                    let options = column_options(frame, *numeric_only);
                    let current = state.get(key).and_then(Value::as_str);
                    if current.is_some_and(|current| options.iter().any(|name| name == current)) {
                        continue;
                    }
                    let fallback = options.get(*default_index as usize).or_else(|| options.first());
                    (key, fallback.map_or(Value::Null, |name| Value::String(name.clone())))
                }
                ComponentKind::Select {
                    state: key,
                    options: SelectOptions::Column { dataset: bound, .. },
                    default,
                    ..
                } if bound == id => {
                    let options = offered.get(key).map(Vec::as_slice).unwrap_or_default();
                    let current = state.get(key).and_then(Value::as_str);
                    if current.is_some_and(|current| options.iter().any(|value| value == current)) {
                        continue;
                    }
                    let fallback = default
                        .as_ref()
                        .filter(|value| options.contains(value))
                        .or(options.first());
                    (key, fallback.map_or(Value::Null, |value| Value::String(value.clone())))
                }
                ComponentKind::FilterInput { dataset: bound, state: key } if bound == id => {
                    let keep = match filters {
                        Filters::Clear => false,
                        Filters::KeepApplicable => {
                            state.get(key).is_some_and(|value| filter_applies(frame, value))
                        }
                    };
                    if keep {
                        continue;
                    }
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

    /// The values each select offering a column of the dataset `id` offers
    /// when the table of that dataset is `frame`, by the state entry of the
    /// select. Scans the columns: call it without holding the session lock.
    fn offered_values(&self, id: &DatasetId, frame: &DataFrame) -> BTreeMap<StateKey, Vec<String>> {
        let mut offered = BTreeMap::new();
        for component in self.inner.spec.components() {
            let ComponentKind::Select {
                state,
                options: SelectOptions::Column { dataset, column },
                ..
            } = &component.kind
            else {
                continue;
            };
            if dataset != id {
                continue;
            }
            // A table without the column offers nothing, so the select is
            // reset; it reports the missing column when it fetches its data.
            let values = query::distinct_values(frame, column, MAX_SELECT_OPTIONS)
                .map(|distinct| distinct.values)
                .unwrap_or_default();
            offered.insert(state.clone(), values);
        }
        offered
    }

    /// What the table of a slot is computed from, as of the session: it
    /// differs as soon as the table is replaced or becomes stale.
    fn table_version(&self, session: &Session, id: &DatasetId) -> Vec<u64> {
        match self.derived_slot(id) {
            Some(slot) => slot.key(&session.revisions),
            None => {
                let revision = session.revisions.get(&Dependency::Dataset(id.clone()));
                vec![revision.copied().unwrap_or(0)]
            }
        }
    }

    /// Recomputes the derived datasets with inputs bound to them that depend
    /// on something in `changed`, and reconciles those inputs with the new
    /// tables. Returns `changed` plus the state entries the reconciliation
    /// changed.
    ///
    /// The datasets are visited in the order of [`compute::derived_slots`]:
    /// after their derived inputs and after the datasets whose bound inputs
    /// write a state entry they read, so each sees the entries reconciled
    /// before it. A derived dataset whose inputs are reconciled into a state
    /// entry it reads itself is not recomputed a second time here; it is
    /// when read.
    fn refresh_derived(&self, mut changed: Vec<Dependency>) -> Vec<Dependency> {
        for slot in &self.inner.derived {
            if slot.reads_any(&changed) && self.has_bound_inputs(&slot.id) {
                changed.extend(self.reconcile_derived(slot).into_iter().map(Dependency::State));
            }
        }
        changed
    }

    /// Computes a derived dataset and reconciles the inputs bound to it.
    /// Returns the state entries that changed; if any did, they are
    /// published under a new revision.
    fn reconcile_derived(&self, slot: &DerivedSlot) -> Vec<StateKey> {
        match self.derived_table(slot) {
            Ok(Outcome::Ready(_)) => {}
            // Nothing to reconcile the inputs with until it can be computed.
            Ok(Outcome::Empty(_)) => return Vec::new(),
            Err(error) => {
                // The change that triggered this succeeded. The components
                // reading the dataset report the failure when they fetch
                // their data, and a failing computation is logged as such.
                tracing::debug!(dataset = %slot.id, %error, "bound inputs not reconciled");
                return Vec::new();
            }
        }
        // `None` when a concurrent change already made the table stale; that
        // change reconciles the inputs itself.
        let current = {
            let session = self.read();
            let frame = self.current_frame(&session, &slot.id).cloned();
            frame.map(|frame| (frame, self.table_version(&session, &slot.id)))
        };
        let Some((frame, version)) = current else {
            return Vec::new();
        };
        let offered = self.offered_values(&slot.id, &frame);
        let mut session = self.write();
        // Same reason: the table changed while the values were listed.
        if self.table_version(&session, &slot.id) != version {
            return Vec::new();
        }
        let changed = self.reconcile_state(
            &slot.id,
            &frame,
            &offered,
            &mut session.state,
            Filters::KeepApplicable,
        );
        for key in &changed {
            session.touch(Dependency::State(key.clone()));
        }
        if !changed.is_empty() {
            // The change that triggered this was published already; a
            // client that read it must be able to tell this state from it.
            session.revision += 1;
        }
        changed
    }

    /// Lists the components reading at least one of the changed things,
    /// directly or through a derived dataset.
    fn invalidated_by(&self, changed: &[Dependency]) -> Vec<ComponentId> {
        self.inner
            .spec
            .components()
            .filter(|component| {
                component.kind.dependencies().iter().any(|read| self.is_stale(read, changed))
            })
            .map(|component| component.id.clone())
            .collect()
    }

    /// Whether `read` is one of the changed things, or a derived dataset
    /// depending on one.
    fn is_stale(&self, read: &Dependency, changed: &[Dependency]) -> bool {
        if changed.contains(read) {
            return true;
        }
        match read {
            Dependency::Dataset(id) => {
                self.derived_slot(id).is_some_and(|slot| slot.reads_any(changed))
            }
            Dependency::State(_) => false,
        }
    }

    /// Sets one state entry, as an input component does.
    ///
    /// The value is checked against the input that owns the entry:
    ///
    /// - a column selection accepts `null` or the name of an offered column;
    /// - a filter input accepts `null` or a [`Filter`] applicable to the
    ///   dataset;
    /// - a number input or a slider accepts a number within its bounds, on
    ///   its steps, and whole if it is an integer input; a whole number is
    ///   stored as a JSON integer;
    /// - a text input accepts a string of at most its maximum length;
    /// - a select accepts one of the values it offers; one offering the
    ///   values of a column also accepts `null`, and accepts nothing else
    ///   while its dataset has no table;
    /// - a checkbox accepts `true` or `false`.
    ///
    /// When the input reads a derived dataset, it is computed first if
    /// needed.
    pub fn set_state(&self, key: &StateKey, value: Value) -> Result<StateUpdate, AppError> {
        let writer = self
            .inner
            .spec
            .components()
            .find(|component| component.kind.written_state() == Some(key))
            .ok_or_else(|| AppError::UnknownState(key.clone()))?;

        // The form of the value is checked before anything is computed.
        let value = check_value(&writer.kind, key, value, None)?;
        if let ComponentKind::Select { options: SelectOptions::Column { dataset, column }, .. } =
            &writer.kind
            && let Value::String(choice) = &value
        {
            return self.set_offered_value(key, choice, dataset, column);
        }

        let dataset = checked_dataset(&writer.kind);
        if let Some(dataset) = dataset
            && !value.is_null()
        {
            // Only computes a derived dataset whose cached table is stale, so
            // the check below can read it; its outcome is read again there.
            self.slot_table(dataset, Access::Compute)?;
        }

        let session = self.write();
        let value = match dataset {
            Some(dataset) => {
                check_value(&writer.kind, key, value, self.current_frame(&session, dataset))?
            }
            None => value,
        };
        Ok(self.store(session, key, value))
    }

    /// Sets the state entry of a select offering the values of `column` in
    /// `dataset` to `choice`, if it is one of them.
    ///
    /// The values are listed without the session lock, then the entry is
    /// set only if the table they were listed from is still current. If the
    /// table changed in between, the check is made once more against the
    /// new one.
    fn set_offered_value(
        &self,
        key: &StateKey,
        choice: &str,
        dataset: &DatasetId,
        column: &str,
    ) -> Result<StateUpdate, AppError> {
        let invalid = |message: String| AppError::InvalidStateValue { state: key.clone(), message };
        for _attempt in 0..2 {
            if let Outcome::Empty(_) = self.slot_table(dataset, Access::Compute)? {
                let wait = match self.derived_slot(dataset) {
                    Some(_) => format!("dataset `{dataset}` is computed"),
                    None => format!("a table is loaded into dataset `{dataset}`"),
                };
                return Err(invalid(format!("no values are on offer until {wait}")));
            }
            let current = {
                let session = self.read();
                let frame = self.current_frame(&session, dataset).cloned();
                frame.map(|frame| (frame, self.table_version(&session, dataset)))
            };
            // `None`: the table became stale since it was computed.
            let Some((frame, version)) = current else { continue };
            let offered = query::distinct_values(&frame, column, MAX_SELECT_OPTIONS)?;
            if !offered.values.iter().any(|value| value == choice) {
                return Err(invalid(format!(
                    "`{choice}` is not one of the values of `{column}` on offer"
                )));
            }
            let session = self.write();
            if self.table_version(&session, dataset) == version {
                return Ok(self.store(session, key, Value::String(choice.to_owned())));
            }
        }
        Err(invalid(format!(
            "the values of `{column}` changed while `{choice}` was checked; choose again"
        )))
    }

    /// Stores a checked value and refreshes what depends on it. Takes the
    /// session lock held since the check, and releases it.
    fn store(
        &self,
        mut session: RwLockWriteGuard<'_, Session>,
        key: &StateKey,
        value: Value,
    ) -> StateUpdate {
        if session.state.get(key) == Some(&value) {
            return StateUpdate {
                revision: session.revision,
                state: session.state.clone(),
                invalidated: Vec::new(),
            };
        }
        session.state.insert(key.clone(), value);
        session.touch(Dependency::State(key.clone()));
        session.revision += 1;
        drop(session);

        let changed = self.refresh_derived(vec![Dependency::State(key.clone())]);
        self.update(&changed)
    }

    /// Computes the data of one component from the current state.
    ///
    /// `offset` is the first row of the requested page; only tables use it.
    pub fn component_data(&self, id: &ComponentId, offset: u32) -> Result<ComponentData, AppError> {
        self.component_data_with(id, offset, Access::Compute)
    }

    /// Like [`component_data`](Self::component_data), but never computes a
    /// derived dataset: a component reading one without a cached table gets
    /// [`ComponentData::Empty`] saying it is not computed yet.
    pub fn cached_component_data(
        &self,
        id: &ComponentId,
        offset: u32,
    ) -> Result<ComponentData, AppError> {
        self.component_data_with(id, offset, Access::CachedOnly)
    }

    fn component_data_with(
        &self,
        id: &ComponentId,
        offset: u32,
        access: Access,
    ) -> Result<ComponentData, AppError> {
        let component =
            self.inner.spec.component(id).ok_or_else(|| AppError::UnknownComponent(id.clone()))?;

        match &component.kind {
            ComponentKind::Text { .. } => Ok(ComponentData::Text),
            ComponentKind::NumberInput { .. }
            | ComponentKind::Slider { .. }
            | ComponentKind::TextInput { .. }
            | ComponentKind::Checkbox { .. } => Ok(ComponentData::Input),
            ComponentKind::Select { options: SelectOptions::Static(values), .. } => {
                Ok(ComponentData::Select { options: values.clone(), truncated: false })
            }
            ComponentKind::Select {
                options: SelectOptions::Column { dataset, column }, ..
            } => self.slot_table(dataset, access)?.show(|frame| {
                let distinct = query::distinct_values(&frame, column, MAX_SELECT_OPTIONS)?;
                Ok(ComponentData::Select {
                    options: distinct.values,
                    truncated: distinct.truncated,
                })
            }),
            ComponentKind::FileImport { dataset } | ComponentKind::DatasetInfo { dataset } => {
                self.dataset_info(dataset, access)?.show(|info| Ok(ComponentData::Dataset { info }))
            }
            ComponentKind::ColumnSelect { dataset, numeric_only, .. } => {
                self.slot_table(dataset, access)?.show(|frame| {
                    Ok(ComponentData::ColumnSelect {
                        options: column_options(&frame, *numeric_only),
                    })
                })
            }
            ComponentKind::FilterInput { dataset, .. } => {
                self.slot_table(dataset, access)?.show(|frame| {
                    Ok(ComponentData::FilterInput { columns: query::column_infos(&frame) })
                })
            }
            ComponentKind::DataTable { view, page_size } => {
                self.resolve_view(view, access)?.show(|rows| {
                    let table =
                        query::table_page(&rows.frame, offset, *page_size, rows.unfiltered_rows)?;
                    Ok(ComponentData::DataTable { table })
                })
            }
            ComponentKind::Statistics { view } => self.resolve_view(view, access)?.show(|rows| {
                Ok(ComponentData::Statistics { stats: query::statistics(&rows.frame)? })
            }),
            ComponentKind::Chart { view, mark, x, y, max_points } => {
                let (x_column, y_column) = {
                    let session = self.read();
                    let column = |key: &StateKey| {
                        session.state.get(key).and_then(Value::as_str).map(str::to_owned)
                    };
                    (column(x), column(y))
                };
                self.resolve_view(view, access)?.show(|rows| {
                    let (Some(x_column), Some(y_column)) = (x_column, y_column) else {
                        return Ok(ComponentData::Empty {
                            reason: "Choose two numeric columns to plot.".to_owned(),
                        });
                    };
                    let chart =
                        query::chart_data(&rows.frame, *mark, &x_column, &y_column, *max_points)?;
                    Ok(ComponentData::Chart { chart })
                })
            }
        }
    }

    /// Describes the dataset of a slot, computing a derived one if needed.
    fn dataset_info(
        &self,
        id: &DatasetId,
        access: Access,
    ) -> Result<Outcome<DatasetInfo>, AppError> {
        if let Some(slot) = self.derived_slot(id) {
            if access == Access::CachedOnly {
                let cached = self.current_frame(&self.read(), id).cloned();
                return Ok(Outcome::Ready(slot.info(cached.as_ref())));
            }
            return Ok(self.derived_table(slot)?.map(|frame| slot.info(Some(&frame))));
        }
        let info = self.read().datasets.get(id).map(|dataset| dataset.info(id));
        Ok(info.map_or_else(Outcome::no_dataset, Outcome::Ready))
    }

    /// The table of a slot as of the current state, without computing
    /// anything: the imported dataset, or the cached table of a derived
    /// dataset if nothing it depends on changed since it was computed.
    fn current_frame<'a>(&self, session: &'a Session, id: &DatasetId) -> Option<&'a DataFrame> {
        match self.derived_slot(id) {
            None => session.datasets.get(id).map(Dataset::frame),
            Some(slot) => session
                .cache
                .get(id)
                .filter(|entry| entry.key == slot.key(&session.revisions))
                .map(|entry| &entry.frame),
        }
    }

    /// The table of a slot, computing a derived dataset if needed.
    fn slot_table(&self, id: &DatasetId, access: Access) -> Result<Outcome<DataFrame>, AppError> {
        if let Some(slot) = self.derived_slot(id) {
            if access == Access::CachedOnly {
                let cached = self.current_frame(&self.read(), id).cloned();
                return Ok(cached.map_or_else(
                    || Outcome::Empty(format!("Not computed yet: {NOT_COMPUTED}")),
                    Outcome::Ready,
                ));
            }
            return self.derived_table(slot);
        }
        let frame = self.read().datasets.get(id).map(|dataset| dataset.frame().clone());
        Ok(frame.map_or_else(Outcome::no_dataset, Outcome::Ready))
    }

    /// The table of a derived dataset: the cached one if it is up to date,
    /// otherwise the result of its computation on its current inputs, which
    /// are produced first, recursively.
    ///
    /// The session lock is not held while the computation runs. Concurrent
    /// readers of the same dataset wait for one result rather than computing
    /// their own. The result is cached only if nothing it depends on changed
    /// while it was computed; a failure caches nothing.
    fn derived_table(&self, slot: &DerivedSlot) -> Result<Outcome<DataFrame>, AppError> {
        let cached = self.current_frame(&self.read(), &slot.id).cloned();
        if let Some(frame) = cached {
            return Ok(Outcome::Ready(frame));
        }

        // Lock order follows the derived datasets from readers to inputs, an
        // acyclic order, and the session lock is never held while waiting.
        let _evaluating = slot.evaluating.lock().unwrap_or_else(PoisonError::into_inner);
        let (key, params) = {
            let session = self.read();
            // Another reader may have computed it while this one waited.
            if let Some(frame) = self.current_frame(&session, &slot.id) {
                return Ok(Outcome::Ready(frame.clone()));
            }
            (slot.key(&session.revisions), slot.param_values(&session.state))
        };
        let mut inputs = BTreeMap::new();
        for (name, view) in &slot.inputs {
            match self.resolve_view(view, Access::Compute)? {
                Outcome::Ready(rows) => inputs.insert(name.clone(), rows.frame),
                Outcome::Empty(reason) => return Ok(Outcome::Empty(reason)),
            };
        }

        let name = slot.computation.name();
        match compute::run(&slot.computation, &slot.context(inputs, params)) {
            Ok(frame) => {
                let mut session = self.write();
                if slot.key(&session.revisions) == key {
                    session.cache.insert(slot.id.clone(), CacheEntry { key, frame: frame.clone() });
                }
                tracing::debug!(dataset = %slot.id, computation = name, rows = frame.height(), "derived dataset computed");
                Ok(Outcome::Ready(frame))
            }
            Err(ComputeError::UnsetParameter(param)) => {
                Ok(Outcome::Empty(self.unset_reason(slot, &param)))
            }
            Err(error) => {
                tracing::warn!(dataset = %slot.id, computation = name, %error, "computation failed");
                Err(AppError::Computation { name: name.to_owned(), message: error.to_string() })
            }
        }
    }

    /// Why a derived dataset waits for a parameter, naming the input that
    /// sets it.
    fn unset_reason(&self, slot: &DerivedSlot, param: &str) -> String {
        let Some(ParamBinding::State(key)) = slot.params.get(param) else {
            return format!(
                "Parameter `{param}` of computation `{}` has no value.",
                slot.computation.name()
            );
        };
        let writer = self
            .inner
            .spec
            .components()
            .find(|component| component.kind.written_state() == Some(key));
        let label = writer.and_then(|writer| input_label(&writer.kind).or(writer.title.as_deref()));
        let input = match label {
            Some(label) => format!("“{label}”"),
            None => format!("`{key}`"),
        };
        format!("Choose a value for {input} to see this data.")
    }

    /// Produces the rows of a view, or why there are none.
    ///
    /// The session lock is released before the filter runs.
    fn resolve_view(&self, view: &DataView, access: Access) -> Result<Outcome<ViewRows>, AppError> {
        let frame = match self.slot_table(&view.dataset, access)? {
            Outcome::Ready(frame) => frame,
            Outcome::Empty(reason) => return Ok(Outcome::Empty(reason)),
        };
        let filter = {
            let session = self.read();
            match view.filter.as_ref().and_then(|key| session.state.get(key)) {
                None | Some(Value::Null) => None,
                Some(value) => Some(serde_json::from_value::<Filter>(value.clone())),
            }
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
        Ok(Outcome::Ready(ViewRows { frame, unfiltered_rows }))
    }
}

/// The rows of a view, after filtering.
struct ViewRows {
    frame: DataFrame,
    unfiltered_rows: u32,
}

/// The label of an input component, for messages naming it.
fn input_label(kind: &ComponentKind) -> Option<&str> {
    match kind {
        ComponentKind::ColumnSelect { label, .. }
        | ComponentKind::NumberInput { label, .. }
        | ComponentKind::Slider { label, .. }
        | ComponentKind::TextInput { label, .. }
        | ComponentKind::Select { label, .. }
        | ComponentKind::Checkbox { label, .. } => Some(label),
        _ => None,
    }
}

/// The dataset whose table a value written by the input `kind` is checked
/// against, if any.
fn checked_dataset(kind: &ComponentKind) -> Option<&DatasetId> {
    match kind {
        ComponentKind::ColumnSelect { dataset, .. }
        | ComponentKind::FilterInput { dataset, .. } => Some(dataset),
        _ => None,
    }
}

/// Checks a value for the state entry `key` written by the input `kind`,
/// and returns it as the entry stores it. With `table`, the table of the
/// dataset the input reads (see [`checked_dataset`]), also checks that a
/// column exists or a filter applies; both are cheap.
fn check_value(
    kind: &ComponentKind,
    key: &StateKey,
    value: Value,
    table: Option<&DataFrame>,
) -> Result<Value, AppError> {
    let invalid = |message: String| AppError::InvalidStateValue { state: key.clone(), message };
    match kind {
        ComponentKind::ColumnSelect { numeric_only, .. } => match &value {
            Value::Null => Ok(value),
            Value::String(column) => match table {
                Some(frame) if !column_options(frame, *numeric_only).contains(column) => {
                    Err(invalid(format!("`{column}` is not an available column")))
                }
                _ => Ok(value),
            },
            _ => Err(invalid("expected a column name or null".to_owned())),
        },
        ComponentKind::FilterInput { .. } => {
            if value.is_null() {
                return Ok(value);
            }
            let filter: Filter = serde_json::from_value(value.clone())
                .map_err(|error| invalid(format!("not a filter: {error}")))?;
            if let Some(frame) = table {
                // Applying the filter to one row checks the column and the
                // value type without scanning the table.
                query::apply_filter(&frame.slice(0, 1), &filter)?;
            }
            Ok(value)
        }
        ComponentKind::NumberInput { .. } | ComponentKind::Slider { .. } => {
            match kind.number_constraints() {
                Some(constraints) => constraints.check(&value).map_err(invalid),
                None => Err(AppError::UnknownState(key.clone())),
            }
        }
        ComponentKind::TextInput { max_length, .. } => {
            let Value::String(text) = &value else {
                return Err(invalid(format!("expected a text, got {value}")));
            };
            let length = text.chars().count();
            if length > *max_length as usize {
                return Err(invalid(format!(
                    "the text has {length} characters, more than the maximum of {max_length}"
                )));
            }
            Ok(value)
        }
        ComponentKind::Checkbox { .. } => match value {
            Value::Bool(_) => Ok(value),
            _ => Err(invalid(format!("expected true or false, got {value}"))),
        },
        ComponentKind::Select { options: SelectOptions::Static(values), .. } => match &value {
            Value::String(choice) if values.contains(choice) => Ok(value),
            _ => Err(invalid(format!("{value} is not one of the options: {}", values.join(", ")))),
        },
        // Whether a value is on offer is checked by `App::set_offered_value`.
        ComponentKind::Select { options: SelectOptions::Column { column, .. }, .. } => match &value
        {
            Value::Null | Value::String(_) => Ok(value),
            _ => Err(invalid(format!("expected a value of `{column}` or null"))),
        },
        ComponentKind::Text { .. }
        | ComponentKind::FileImport { .. }
        | ComponentKind::DatasetInfo { .. }
        | ComponentKind::DataTable { .. }
        | ComponentKind::Chart { .. }
        | ComponentKind::Statistics { .. } => Err(AppError::UnknownState(key.clone())),
    }
}

/// Whether the state value of a filter input is a filter applicable to
/// `frame`. A value that is not, for any reason, is what reconciliation
/// clears, so the reason is not needed.
fn filter_applies(frame: &DataFrame, value: &Value) -> bool {
    serde_json::from_value::<Filter>(value.clone())
        .is_ok_and(|filter| query::apply_filter(&frame.slice(0, 1), &filter).is_ok())
}

/// Names of the columns a column selection offers.
fn column_options(frame: &DataFrame, numeric_only: bool) -> Vec<String> {
    query::column_infos(frame)
        .into_iter()
        .filter(|column| !numeric_only || column.kind == ColumnKind::Numeric)
        .map(|column| column.name)
        .collect()
}
