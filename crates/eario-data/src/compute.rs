//! Extension point for custom logic.
//!
//! A [`Computation`] is a named Rust function that turns input tables and
//! parameters into one table. A derived dataset of the dashboard
//! specification names the computation and binds each of its inputs and
//! parameters; the runtime evaluates it with a [`ComputeContext`] holding
//! their current values. An application registers its computations in a
//! [`ComputationRegistry`].

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use polars::error::PolarsError;
use polars::frame::DataFrame;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::DataError;

/// The function of a computation.
type ComputeFn = dyn Fn(&ComputeContext) -> Result<DataFrame, ComputeError> + Send + Sync;

/// A named function turning input tables and parameters into one table.
///
/// It declares the names of the inputs and parameters it reads. A derived
/// dataset binds every one of them, and the runtime rejects a dashboard that
/// binds a name the computation does not declare or leaves a declared one
/// unbound.
///
/// ```
/// use eario_data::compute::Computation;
/// use eario_data::polars::prelude::*;
///
/// let above = Computation::new("above", |ctx| {
///     let column: String = ctx.param("column")?;
///     let threshold: f64 = ctx.param("threshold")?;
///     let rows = ctx.input("table")?.clone().lazy();
///     Ok(rows.filter(col(column.as_str()).gt(lit(threshold))).collect()?)
/// })
/// .input("table")
/// .param("column")
/// .param("threshold")
/// .describe("The rows whose value in `column` exceeds `threshold`.");
/// assert_eq!(above.name(), "above");
/// ```
#[derive(Clone)]
pub struct Computation {
    name: String,
    inputs: BTreeSet<String>,
    params: BTreeSet<String>,
    description: Option<String>,
    function: Arc<ComputeFn>,
}

impl Computation {
    /// Creates a computation named `name` that runs `function`. It reads no
    /// input and no parameter until they are declared.
    pub fn new<F>(name: impl Into<String>, function: F) -> Self
    where
        F: Fn(&ComputeContext) -> Result<DataFrame, ComputeError> + Send + Sync + 'static,
    {
        Self {
            name: name.into(),
            inputs: BTreeSet::new(),
            params: BTreeSet::new(),
            description: None,
            function: Arc::new(function),
        }
    }

    /// Declares an input table, read with [`ComputeContext::input`].
    pub fn input(mut self, name: impl Into<String>) -> Self {
        self.inputs.insert(name.into());
        self
    }

    /// Declares a parameter, read with [`ComputeContext::param`].
    pub fn param(mut self, name: impl Into<String>) -> Self {
        self.params.insert(name.into());
        self
    }

    /// Describes what the computation produces, for the people and agents
    /// inspecting the dashboard.
    pub fn describe(mut self, text: impl Into<String>) -> Self {
        self.description = Some(text.into());
        self
    }

    /// Name derived datasets refer to the computation by.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Names of the declared inputs.
    pub fn inputs(&self) -> &BTreeSet<String> {
        &self.inputs
    }

    /// Names of the declared parameters.
    pub fn params(&self) -> &BTreeSet<String> {
        &self.params
    }

    /// What the computation produces, if described.
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// Runs the function on `context`. A panic of the function is not
    /// caught here; the runtime catches it.
    pub fn run(&self, context: &ComputeContext) -> Result<DataFrame, ComputeError> {
        (self.function)(context)
    }
}

impl fmt::Debug for Computation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Computation")
            .field("name", &self.name)
            .field("inputs", &self.inputs)
            .field("params", &self.params)
            .field("description", &self.description)
            .finish_non_exhaustive()
    }
}

/// What a computation receives: its input tables and its parameter values,
/// by the names it declares.
#[derive(Debug, Clone, Default)]
pub struct ComputeContext {
    inputs: BTreeMap<String, DataFrame>,
    params: BTreeMap<String, Value>,
}

impl ComputeContext {
    /// A context holding the given input tables and parameter values.
    ///
    /// The runtime builds one for every evaluation. Build one yourself to
    /// test a computation without running a dashboard.
    pub fn new(inputs: BTreeMap<String, DataFrame>, params: BTreeMap<String, Value>) -> Self {
        Self { inputs, params }
    }

    /// The input table named `name`, with the filter of its binding, if
    /// any, already applied. Cloning it is cheap: columns are shared.
    pub fn input(&self, name: &str) -> Result<&DataFrame, ComputeError> {
        self.inputs.get(name).ok_or_else(|| ComputeError::UnknownInput(name.to_owned()))
    }

    /// The value of the parameter named `name`, read as a `T`.
    ///
    /// A parameter bound to a state entry the user has not set yet holds
    /// `null`. Unless `T` accepts `null`, as `Option` does, this returns
    /// [`ComputeError::UnsetParameter`]; returning that error from the
    /// computation makes the components reading the dataset ask for a value
    /// instead of reporting a failure.
    pub fn param<T: DeserializeOwned>(&self, name: &str) -> Result<T, ComputeError> {
        let value =
            self.params.get(name).ok_or_else(|| ComputeError::UnknownParameter(name.to_owned()))?;
        T::deserialize(value).map_err(|error| {
            if value.is_null() {
                ComputeError::UnsetParameter(name.to_owned())
            } else {
                ComputeError::InvalidParameter { name: name.to_owned(), message: error.to_string() }
            }
        })
    }
}

/// Reason a computation produced no table.
///
/// The runtime shows the message to the user, prefixed with the name of
/// the computation, so describe the problem in their terms.
#[derive(Debug, thiserror::Error)]
pub enum ComputeError {
    /// The computation read an input it does not declare.
    #[error("the computation has no input `{0}`")]
    UnknownInput(String),
    /// The computation read a parameter it does not declare.
    #[error("the computation has no parameter `{0}`")]
    UnknownParameter(String),
    /// The parameter has no value yet: the state entry it is bound to is
    /// `null`.
    #[error("parameter `{0}` has no value yet")]
    UnsetParameter(String),
    /// The parameter value cannot be read as the type the computation asked
    /// for.
    #[error("parameter `{name}` has an unexpected value: {message}")]
    InvalidParameter {
        /// The parameter.
        name: String,
        /// Why the value was rejected, as reported by the deserializer.
        message: String,
    },
    /// A query of the data layer failed.
    #[error(transparent)]
    Data(#[from] DataError),
    /// The data engine failed.
    #[error("data engine error: {0}")]
    Engine(#[from] PolarsError),
    /// The computation failed for a reason of its own.
    #[error("{0}")]
    Failed(String),
}

impl ComputeError {
    /// A failure described by the computation, for example
    /// `ComputeError::new("the window is longer than the signal")`.
    pub fn new(message: impl Into<String>) -> Self {
        Self::Failed(message.into())
    }

    /// Stable, machine-readable code of the error.
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnknownInput(_) => "unknown_input",
            Self::UnknownParameter(_) => "unknown_parameter",
            Self::UnsetParameter(_) => "unset_parameter",
            Self::InvalidParameter { .. } => "invalid_parameter",
            Self::Data(error) => error.code(),
            Self::Engine(_) => "engine_error",
            Self::Failed(_) => "computation_failed",
        }
    }
}

/// The computations an application can evaluate, by name.
#[derive(Debug, Clone, Default)]
pub struct ComputationRegistry {
    computations: BTreeMap<String, Computation>,
}

impl ComputationRegistry {
    /// Adds a computation. It replaces, and returns, the one registered
    /// earlier under the same name, if any.
    pub fn register(&mut self, computation: Computation) -> Option<Computation> {
        self.computations.insert(computation.name.clone(), computation)
    }

    /// Finds a computation by name.
    pub fn get(&self, name: &str) -> Option<&Computation> {
        self.computations.get(name)
    }
}

#[cfg(test)]
mod tests {
    use polars::prelude::Column;
    use serde_json::json;

    use super::*;

    fn context() -> Result<ComputeContext, PolarsError> {
        let table = DataFrame::new(3, vec![Column::new("x".into(), [1.0, 2.0, 3.0])])?;
        Ok(ComputeContext::new(
            BTreeMap::from([("table".to_owned(), table)]),
            BTreeMap::from([
                ("rows".to_owned(), json!(2)),
                ("column".to_owned(), Value::Null),
                ("label".to_owned(), json!("total")),
            ]),
        ))
    }

    #[test]
    fn inputs_and_params_are_read_by_name_and_typed_on_access()
    -> Result<(), Box<dyn std::error::Error>> {
        let context = context()?;
        assert_eq!(context.input("table")?.height(), 3);
        assert_eq!(context.param::<usize>("rows")?, 2);
        assert_eq!(context.param::<String>("label")?, "total");
        Ok(())
    }

    #[test]
    fn undeclared_names_are_reported() -> Result<(), PolarsError> {
        let context = context()?;
        assert!(
            matches!(context.input("other"), Err(ComputeError::UnknownInput(name)) if name == "other")
        );
        assert!(matches!(
            context.param::<u32>("other"),
            Err(ComputeError::UnknownParameter(name)) if name == "other"
        ));
        Ok(())
    }

    #[test]
    fn null_parameter_is_unset_unless_the_type_accepts_null()
    -> Result<(), Box<dyn std::error::Error>> {
        let context = context()?;
        let unset = context.param::<String>("column");
        assert!(matches!(&unset, Err(ComputeError::UnsetParameter(name)) if name == "column"));
        assert_eq!(unset.err().map(|error| error.code()), Some("unset_parameter"));
        assert_eq!(context.param::<Option<String>>("column")?, None);
        Ok(())
    }

    #[test]
    fn parameter_of_the_wrong_type_is_invalid() -> Result<(), PolarsError> {
        let context = context()?;
        let error = context.param::<u32>("label").err();
        assert!(
            matches!(&error, Some(ComputeError::InvalidParameter { name, .. }) if name == "label")
        );
        assert!(error.map(|error| error.to_string()).unwrap_or_default().contains("`label`"));
        Ok(())
    }

    #[test]
    fn computation_declares_its_names_and_runs_its_function()
    -> Result<(), Box<dyn std::error::Error>> {
        let head = Computation::new("head", |ctx| {
            let rows: usize = ctx.param("rows")?;
            Ok(ctx.input("table")?.head(Some(rows)))
        })
        .input("table")
        .param("rows")
        .param("rows")
        .describe("The first rows.");
        assert_eq!(head.name(), "head");
        assert_eq!(head.inputs().iter().collect::<Vec<_>>(), ["table"]);
        assert_eq!(head.params().iter().collect::<Vec<_>>(), ["rows"]);
        assert_eq!(head.description(), Some("The first rows."));
        assert_eq!(head.run(&context()?)?.height(), 2);
        Ok(())
    }

    #[test]
    fn registering_a_name_again_replaces_the_computation() {
        let mut registry = ComputationRegistry::default();
        let first = Computation::new("f", |_| Err(ComputeError::new("first")));
        let second = Computation::new("f", |_| Err(ComputeError::new("second"))).input("table");
        assert!(registry.register(first).is_none());
        assert!(registry.register(second).is_some());
        assert_eq!(registry.get("f").map(|found| found.inputs().len()), Some(1));
        assert!(registry.get("g").is_none());
    }
}
