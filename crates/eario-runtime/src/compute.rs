//! Derived datasets, as the runtime evaluates them.
//!
//! [`derived_slots`] checks the derived datasets of a specification against
//! the registered computations and describes each one as a [`DerivedSlot`]:
//! its computation, its bindings and what its table depends on. [`run`]
//! calls a computation without letting a panic escape. The [`App`](crate::App)
//! decides when to evaluate and caches the results.

use std::any::Any;
use std::collections::{BTreeMap, BTreeSet};
use std::panic::{self, AssertUnwindSafe};
use std::sync::Mutex;

use eario_core::{
    ComponentKind, DashboardSpec, DataView, DatasetId, DatasetInfo, DatasetSource, Dependency,
    DerivedStatus, ParamBinding, SelectOptions, StateKey, StateMap,
};
use eario_data::{
    Computation, ComputationRegistry, ComputeContext, ComputeError, DataFrame, query,
};
use serde_json::Value;

use crate::error::AppError;

/// How many times each imported dataset and each state entry changed.
/// Absent means never.
pub(crate) type Revisions = BTreeMap<Dependency, u64>;

/// One derived dataset of the dashboard.
pub(crate) struct DerivedSlot {
    /// The slot.
    pub(crate) id: DatasetId,
    /// The registered computation producing the table.
    pub(crate) computation: Computation,
    /// Input tables, by the name the computation knows them under.
    pub(crate) inputs: BTreeMap<String, DataView>,
    /// Parameter bindings, by the name the computation knows them under.
    pub(crate) params: BTreeMap<String, ParamBinding>,
    /// The imported datasets and state entries the table depends on,
    /// directly or through other derived datasets. Their revisions are the
    /// key of the cached table.
    dependencies: Vec<Dependency>,
    /// Held while the table is computed, so concurrent readers wait for one
    /// result instead of computing their own.
    pub(crate) evaluating: Mutex<()>,
}

impl DerivedSlot {
    /// Whether the table depends on one of `changed`.
    pub(crate) fn reads_any(&self, changed: &[Dependency]) -> bool {
        self.dependencies.iter().any(|dependency| changed.contains(dependency))
    }

    /// The revisions of what the table depends on: the key of the cache.
    pub(crate) fn key(&self, revisions: &Revisions) -> Vec<u64> {
        self.dependencies
            .iter()
            .map(|dependency| revisions.get(dependency).copied().unwrap_or(0))
            .collect()
    }

    /// The current value of every parameter.
    pub(crate) fn param_values(&self, state: &StateMap) -> BTreeMap<String, Value> {
        self.params
            .iter()
            .map(|(name, binding)| {
                let value = match binding {
                    ParamBinding::State(key) => state.get(key).cloned().unwrap_or(Value::Null),
                    ParamBinding::Literal(value) => value.clone(),
                };
                (name.clone(), value)
            })
            .collect()
    }

    /// The context of one evaluation: `inputs` and `params` (see
    /// [`param_values`](Self::param_values)), and which parameter each state
    /// entry is bound to, so the computation can read a parameter through a
    /// state handle.
    pub(crate) fn context(
        &self,
        inputs: BTreeMap<String, DataFrame>,
        params: BTreeMap<String, Value>,
    ) -> ComputeContext {
        self.params.iter().fold(ComputeContext::new(inputs, params), |context, (name, binding)| {
            match binding {
                ParamBinding::State(key) => context.bind_state(key.clone(), name.clone()),
                ParamBinding::Literal(_) => context,
            }
        })
    }

    /// Describes the dataset; `table` is its table if it is computed for the
    /// current state.
    pub(crate) fn info(&self, table: Option<&DataFrame>) -> DatasetInfo {
        DatasetInfo {
            id: self.id.clone(),
            source_name: self.computation.name().to_owned(),
            format: "computed".to_owned(),
            source_bytes: 0,
            rows: table.map_or(0, query::row_count),
            columns: table.map(query::column_infos).unwrap_or_default(),
            sheets: Vec::new(),
            selected_sheet: None,
            derived: Some(match table {
                Some(_) => DerivedStatus::Computed,
                None => DerivedStatus::NotComputed,
            }),
        }
    }
}

/// The latest table of a derived dataset.
pub(crate) struct CacheEntry {
    /// [`DerivedSlot::key`] when the table was computed.
    pub(crate) key: Vec<u64>,
    /// The table.
    pub(crate) frame: DataFrame,
}

/// Describes the derived datasets of `spec`, in the order a change
/// refreshes them (see [`dependency_order`]), and checks that each names a
/// registered computation and binds exactly the inputs and parameters it
/// declares.
///
/// `spec` must have passed [`DashboardSpec::validate`], which rules out
/// cycles between derived datasets.
pub(crate) fn derived_slots(
    spec: &DashboardSpec,
    registry: &ComputationRegistry,
) -> Result<Vec<DerivedSlot>, AppError> {
    let derived: BTreeSet<&DatasetId> = spec
        .datasets
        .iter()
        .filter(|slot| matches!(slot.source, DatasetSource::Computed { .. }))
        .map(|slot| &slot.id)
        .collect();

    let mut slots = Vec::with_capacity(derived.len());
    for id in dependency_order(spec) {
        // `dependency_order` lists derived datasets only.
        let Some(DatasetSource::Computed { function, inputs, params }) =
            spec.dataset(&id).map(|slot| &slot.source)
        else {
            continue;
        };
        let Some(computation) = registry.get(function) else {
            return Err(AppError::UnknownComputation {
                dataset: id,
                computation: function.clone(),
            });
        };
        check_bindings(&id, computation, inputs, params)?;
        let dependencies: Vec<Dependency> = spec
            .dataset_dependencies(&id)
            .into_iter()
            .filter(|dependency| {
                !matches!(dependency, Dependency::Dataset(input) if derived.contains(input))
            })
            .collect();
        check_selects(spec, &id, &dependencies)?;
        slots.push(DerivedSlot {
            computation: computation.clone(),
            inputs: inputs.clone(),
            params: params.clone(),
            dependencies,
            evaluating: Mutex::new(()),
            id,
        });
    }
    Ok(slots)
}

/// Checks that a derived dataset binds exactly what its computation declares.
fn check_bindings(
    id: &DatasetId,
    computation: &Computation,
    inputs: &BTreeMap<String, DataView>,
    params: &BTreeMap<String, ParamBinding>,
) -> Result<(), AppError> {
    let dataset = || id.clone();
    let name = || computation.name().to_owned();
    if let Some(input) = inputs.keys().find(|input| !computation.inputs().contains(*input)) {
        return Err(AppError::UndeclaredComputationInput {
            dataset: dataset(),
            computation: name(),
            input: input.clone(),
        });
    }
    if let Some(input) = computation.inputs().iter().find(|input| !inputs.contains_key(*input)) {
        return Err(AppError::MissingComputationInput {
            dataset: dataset(),
            computation: name(),
            input: input.clone(),
        });
    }
    if let Some(parameter) = params.keys().find(|param| !computation.params().contains(*param)) {
        return Err(AppError::UndeclaredComputationParameter {
            dataset: dataset(),
            computation: name(),
            parameter: parameter.clone(),
        });
    }
    if let Some(parameter) = computation.params().iter().find(|param| !params.contains_key(*param))
    {
        return Err(AppError::MissingComputationParameter {
            dataset: dataset(),
            computation: name(),
            parameter: parameter.clone(),
        });
    }
    Ok(())
}

/// Checks that no select offering the values of the derived dataset `id`
/// writes a state entry among its `dependencies`: such a select gets no value
/// until the dataset is computed, and the dataset waits for that value.
fn check_selects(
    spec: &DashboardSpec,
    id: &DatasetId,
    dependencies: &[Dependency],
) -> Result<(), AppError> {
    for component in spec.components() {
        if let ComponentKind::Select {
            state, options: SelectOptions::Column { dataset, .. }, ..
        } = &component.kind
            && dataset == id
            && dependencies.contains(&Dependency::State(state.clone()))
        {
            return Err(AppError::SelectDependsOnItsState {
                state: state.clone(),
                dataset: id.clone(),
            });
        }
    }
    Ok(())
}

/// The state entries written by the inputs bound to the dataset `id`: the
/// column selections, filter inputs and selects offering the values of a
/// column reading it, which are reconciled with its table when that table
/// changes.
pub(crate) fn bound_states<'a>(
    spec: &'a DashboardSpec,
    id: &'a DatasetId,
) -> impl Iterator<Item = &'a StateKey> + 'a {
    spec.components().filter_map(move |component| match &component.kind {
        ComponentKind::ColumnSelect { dataset, state, .. }
        | ComponentKind::FilterInput { dataset, state }
        | ComponentKind::Select { state, options: SelectOptions::Column { dataset, .. }, .. }
            if dataset == id =>
        {
            Some(state)
        }
        _ => None,
    })
}

/// The derived datasets of `spec` in the order a change refreshes them.
///
/// A derived dataset comes after its derived inputs, and after every other
/// derived dataset whose bound inputs write a state entry it depends on:
/// refreshing that one can reconcile the entry. Where these edges form a
/// cycle, the walk, in declaration order, breaks it where it meets a
/// dataset it is already placing.
fn dependency_order(spec: &DashboardSpec) -> Vec<DatasetId> {
    fn visit(
        id: &DatasetId,
        before: &BTreeMap<DatasetId, Vec<DatasetId>>,
        visited: &mut BTreeSet<DatasetId>,
        order: &mut Vec<DatasetId>,
    ) {
        // Imported datasets have no entry and are not listed.
        let Some(predecessors) = before.get(id) else { return };
        if !visited.insert(id.clone()) {
            return;
        }
        for predecessor in predecessors {
            visit(predecessor, before, visited, order);
        }
        order.push(id.clone());
    }

    let derived: Vec<(&DatasetId, &BTreeMap<String, DataView>)> = spec
        .datasets
        .iter()
        .filter_map(|slot| match &slot.source {
            DatasetSource::Computed { inputs, .. } => Some((&slot.id, inputs)),
            DatasetSource::Imported => None,
        })
        .collect();
    let mut writers: BTreeMap<&StateKey, Vec<&DatasetId>> = BTreeMap::new();
    for (id, _) in &derived {
        for state in bound_states(spec, id) {
            writers.entry(state).or_default().push(id);
        }
    }
    let before: BTreeMap<DatasetId, Vec<DatasetId>> = derived
        .iter()
        .map(|(id, inputs)| {
            let mut predecessors: Vec<DatasetId> =
                inputs.values().map(|view| view.dataset.clone()).collect();
            for dependency in spec.dataset_dependencies(id) {
                let Dependency::State(state) = dependency else { continue };
                let written_by = writers.get(&state).into_iter().flatten().copied();
                predecessors.extend(written_by.filter(|writer| writer != id).cloned());
            }
            (DatasetId::clone(id), predecessors)
        })
        .collect();

    let mut visited = BTreeSet::new();
    let mut order = Vec::new();
    for (id, _) in &derived {
        visit(id, &before, &mut visited, &mut order);
    }
    order
}

/// Runs a computation, turning a panic into an error.
pub(crate) fn run(
    computation: &Computation,
    context: &ComputeContext,
) -> Result<DataFrame, ComputeError> {
    // The computation gets a context of its own and runs while no lock is
    // held, so a panic leaves nothing half-updated behind it.
    match panic::catch_unwind(AssertUnwindSafe(|| computation.run(context))) {
        Ok(result) => result,
        Err(payload) => Err(ComputeError::new(format!("panicked: {}", panic_message(&*payload)))),
    }
}

/// The message a panic was raised with, when it has one.
fn panic_message(payload: &(dyn Any + Send)) -> &str {
    if let Some(message) = payload.downcast_ref::<&str>() {
        message
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message
    } else {
        "no message"
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, mpsc};
    use std::time::Duration;

    use eario_core::{
        ComponentData, ComponentId, ComponentSpec, DatasetSlot, PageSpec, RowSpec, SPEC_VERSION,
    };
    use eario_data::Dataset;
    use eario_data::polars::prelude::Column;
    use serde_json::json;

    use super::*;
    use crate::app::{App, AppOptions};

    /// A dashboard with an imported dataset `data` and a dataset `derived`
    /// computed by `function` with the given bindings.
    fn spec(function: &str, inputs: &[&str], params: &[&str]) -> DashboardSpec {
        let data = DatasetId::new("data");
        let source = DatasetSource::Computed {
            function: function.to_owned(),
            inputs: inputs.iter().map(|name| ((*name).to_owned(), DataView::of(&data))).collect(),
            params: params
                .iter()
                .map(|name| ((*name).to_owned(), ParamBinding::Literal(Value::from(1))))
                .collect(),
        };
        let table = ComponentSpec {
            id: ComponentId::new("table"),
            title: None,
            span: 12,
            kind: ComponentKind::DataTable {
                view: DataView::of(&DatasetId::new("derived")),
                page_size: 10,
            },
        };
        DashboardSpec {
            spec_version: SPEC_VERSION,
            title: "Derived".to_owned(),
            datasets: vec![
                DatasetSlot { id: data, label: "Data".to_owned(), source: DatasetSource::Imported },
                DatasetSlot { id: DatasetId::new("derived"), label: "Derived".to_owned(), source },
            ],
            pages: vec![PageSpec {
                id: "main".to_owned(),
                title: "Main".to_owned(),
                rows: vec![RowSpec { items: vec![table] }],
            }],
        }
    }

    fn registry() -> ComputationRegistry {
        let mut registry = ComputationRegistry::default();
        let summarize = Computation::new("summarize", |ctx| Ok(ctx.input("table")?.clone()))
            .input("table")
            .param("bins");
        registry.register(summarize);
        registry
    }

    fn rejection(spec: DashboardSpec) -> Option<AppError> {
        let options = AppOptions { computations: registry(), ..AppOptions::default() };
        App::new(spec, options).err()
    }

    #[test]
    fn matching_bindings_are_accepted() {
        assert!(rejection(spec("summarize", &["table"], &["bins"])).is_none());
    }

    #[test]
    fn unknown_computation_is_rejected() {
        let error = rejection(spec("missing", &["table"], &["bins"]));
        assert_eq!(error.as_ref().map(AppError::code), Some("unknown_computation"));
        assert!(matches!(
            error,
            Some(AppError::UnknownComputation { dataset, computation })
                if dataset.as_str() == "derived" && computation == "missing"
        ));
    }

    #[test]
    fn undeclared_and_missing_bindings_are_rejected_with_their_names() {
        let cases = [
            (spec("summarize", &["table", "extra"], &["bins"]), "undeclared_computation_input"),
            (spec("summarize", &[], &["bins"]), "missing_computation_input"),
            (spec("summarize", &["table"], &["bins", "extra"]), "undeclared_computation_parameter"),
            (spec("summarize", &["table"], &[]), "missing_computation_parameter"),
        ];
        for (spec, code) in cases {
            let error = rejection(spec);
            assert_eq!(error.as_ref().map(AppError::code), Some(code));
            let message = error.map(|error| error.to_string()).unwrap_or_default();
            assert!(message.contains("`derived`") && message.contains("`summarize`"), "{message}");
            let named = ["`table`", "`extra`", "`bins`"].iter().any(|name| message.contains(name));
            assert!(named, "{message}");
        }
    }

    /// `spec` with the parameter `bins` of `derived` bound to the state
    /// `bins`, and a select writing `state` offering the values of `derived`.
    fn select_on_derived(state: &str) -> DashboardSpec {
        let mut spec = spec("summarize", &["table"], &["bins"]);
        if let DatasetSource::Computed { params, .. } = &mut spec.datasets[1].source {
            params.insert("bins".to_owned(), ParamBinding::State(StateKey::new("bins")));
        }
        let select = |id: &str, state: &str, options| ComponentSpec {
            id: ComponentId::new(id),
            title: None,
            span: 6,
            kind: ComponentKind::Select {
                label: id.to_owned(),
                state: StateKey::new(state),
                options,
                default: None,
            },
        };
        let column =
            SelectOptions::Column { dataset: DatasetId::new("derived"), column: "x".to_owned() };
        let mut items = vec![select("choice", state, column)];
        if state != "bins" {
            // Something must write `bins`.
            items.push(select("bins", "bins", SelectOptions::Static(vec!["1".to_owned()])));
        }
        spec.pages[0].rows.push(RowSpec { items });
        spec
    }

    #[test]
    fn select_offering_a_dataset_that_reads_its_own_state_is_rejected() {
        let error = rejection(select_on_derived("bins"));
        assert_eq!(error.as_ref().map(AppError::code), Some("select_depends_on_its_state"));
        assert!(matches!(
            &error,
            Some(AppError::SelectDependsOnItsState { state, dataset })
                if state.as_str() == "bins" && dataset.as_str() == "derived"
        ));
        let message = error.map(|error| error.to_string()).unwrap_or_default();
        assert!(message.contains("state `bins`") && message.contains("`derived`"), "{message}");

        assert!(rejection(select_on_derived("other")).is_none(), "another state is fine");
    }

    #[test]
    fn slots_are_ordered_inputs_first_and_depend_on_imported_data_only() -> Result<(), AppError> {
        let mut spec = spec("summarize", &["table"], &["bins"]);
        // `report` is declared first but reads `derived`.
        let report = DatasetSource::Computed {
            function: "summarize".to_owned(),
            inputs: BTreeMap::from([(
                "table".to_owned(),
                DataView::of(&DatasetId::new("derived")),
            )]),
            params: BTreeMap::from([("bins".to_owned(), ParamBinding::Literal(Value::from(2)))]),
        };
        spec.datasets.insert(
            0,
            DatasetSlot {
                id: DatasetId::new("report"),
                label: "Report".to_owned(),
                source: report,
            },
        );
        let slots = derived_slots(&spec, &registry())?;
        let order: Vec<&str> = slots.iter().map(|slot| slot.id.as_str()).collect();
        assert_eq!(order, ["derived", "report"]);
        assert_eq!(slots[1].dependencies, [Dependency::Dataset(DatasetId::new("data"))]);
        Ok(())
    }

    #[test]
    fn computation_runs_without_the_session_lock_and_a_stale_result_is_not_cached()
    -> Result<(), Box<dyn std::error::Error>> {
        let data = DatasetId::new("data");
        let filter = StateKey::new("filter");
        let mut spec = spec("wait", &["table"], &[]);
        if let DatasetSource::Computed { inputs, .. } = &mut spec.datasets[1].source {
            inputs.insert("table".to_owned(), DataView::of(&data).filtered_by(&filter));
        }
        let filter_input = ComponentSpec {
            id: ComponentId::new("filter"),
            title: None,
            span: 12,
            kind: ComponentKind::FilterInput { dataset: data.clone(), state: filter.clone() },
        };
        spec.pages[0].rows.push(RowSpec { items: vec![filter_input] });

        // `wait` announces that it runs, then waits for the test to resume it.
        let (started, on_start) = mpsc::channel();
        let (resume, on_resume) = mpsc::channel::<()>();
        let on_resume = Mutex::new(on_resume);
        let runs = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&runs);
        let mut computations = ComputationRegistry::default();
        computations.register(
            Computation::new("wait", move |ctx| {
                counted.fetch_add(1, Ordering::SeqCst);
                started.send(()).map_err(|_| ComputeError::new("the test stopped listening"))?;
                let on_resume = on_resume.lock().map_err(|_| ComputeError::new("poisoned"))?;
                on_resume
                    .recv_timeout(Duration::from_secs(5))
                    .map_err(|_| ComputeError::new("not resumed: the session stayed locked"))?;
                Ok(ctx.input("table")?.clone())
            })
            .input("table"),
        );
        let app = App::new(spec, AppOptions { computations, ..AppOptions::default() })?;
        let frame = DataFrame::new(3, vec![Column::new("x".into(), [1.0, 2.0, 3.0])])?;
        app.load_dataset(&data, Dataset::from_frame("x.csv", frame)?)?;
        assert_eq!(runs.load(Ordering::SeqCst), 0, "nothing reads `derived` yet");

        let table = ComponentId::new("table");
        let rows = |component: ComponentData| match component {
            ComponentData::DataTable { table } => Some(table.total_rows),
            _ => None,
        };
        let status = |app: &App| {
            let derived = app.datasets().into_iter().find(|info| info.id.as_str() == "derived");
            derived.and_then(|info| info.derived)
        };
        let narrow = json!({ "column": "x", "op": "gt", "value": 1.5 });
        let first = std::thread::scope(|scope| -> Result<_, Box<dyn std::error::Error>> {
            let reader = scope.spawn(|| app.component_data(&table, 0));
            on_start.recv_timeout(Duration::from_secs(5))?;
            // While `wait` runs, a change takes the session lock.
            app.set_state(&filter, narrow)?;
            resume.send(())?;
            Ok(reader.join().map_err(|_| "the reader panicked")??)
        })?;
        assert_eq!(rows(first), Some(3), "computed from the table before the change");
        assert_eq!(status(&app), Some(DerivedStatus::NotComputed), "stale, so not cached");

        resume.send(())?;
        assert_eq!(rows(app.component_data(&table, 0)?), Some(2));
        assert_eq!(rows(app.component_data(&table, 0)?), Some(2));
        assert_eq!(runs.load(Ordering::SeqCst), 2, "the second result is cached");
        assert_eq!(status(&app), Some(DerivedStatus::Computed));
        Ok(())
    }

    #[test]
    fn panic_becomes_an_error_naming_its_message() {
        let computation = Computation::new("boom", |_| panic!("division by zero"));
        let result = run(&computation, &ComputeContext::default());
        let message = result.err().map(|error| error.to_string()).unwrap_or_default();
        assert_eq!(message, "panicked: division by zero");
    }
}
