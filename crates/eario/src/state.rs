//! Typed handles of state entries.

use std::fmt;
use std::marker::PhantomData;

use eario_core::{DataView, DatasetId, Filter, StateKey};
use eario_data::compute::{Param, ParamRef};

/// A handle of one entry of the application state, holding values of type
/// `T` once set.
///
/// An input component writes the entry; charts, filtered views and
/// computation parameters read it. The type stops a handle from being given
/// where another kind of value is expected: a column selection is a
/// `State<String>`, a filter a `State<Filter>`. On the wire the entry is
/// named by its [`StateKey`].
///
/// A computation reads a parameter bound to the entry with
/// [`ComputeContext::param`](eario_data::ComputeContext::param), which
/// returns a `T`:
///
/// ```
/// use eario::prelude::*;
///
/// let column = State::<String>::new("column");
/// let reader = column.clone();
/// let selected = Computation::new("selected", move |ctx| {
///     let name = ctx.param(&reader)?;
///     Ok(ctx.input("table")?.select([name])?)
/// })
/// .input("table")
/// .param("column");
/// assert_eq!(column.key().as_str(), "column");
/// ```
///
/// Reading through a handle works only in a derived dataset that binds
/// this very entry to one of the computation's parameters, as
/// `.param("column", &column)` does; elsewhere `ctx.param(&handle)` returns
/// [`ComputeError::UnboundState`](eario_data::ComputeError::UnboundState).
/// A computation reused by derived datasets that bind its parameters to
/// different entries, or to literals, reads them by name instead:
/// `ctx.param::<String>("column")`.
pub struct State<T> {
    key: StateKey,
    // `fn() -> T`: the handle holds no `T`, so it is `Send`, `Sync` and
    // `Clone` whatever `T` is.
    value: PhantomData<fn() -> T>,
}

impl<T> State<T> {
    /// A handle of the state entry named `key`.
    pub fn new(key: impl Into<String>) -> Self {
        Self { key: StateKey::new(key), value: PhantomData }
    }

    /// The name of the entry on the wire.
    pub fn key(&self) -> &StateKey {
        &self.key
    }
}

impl<T> Clone for State<T> {
    fn clone(&self) -> Self {
        Self { key: self.key.clone(), value: PhantomData }
    }
}

impl<T> fmt::Debug for State<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("State").field(&self.key.as_str()).finish()
    }
}

/// Reads the parameter bound to this entry; see the note on [`State`].
impl<T> Param<T> for &State<T> {
    fn param_ref(&self) -> ParamRef<'_> {
        ParamRef::State(&self.key)
    }
}

/// The rows of `dataset` that match the filter held in `filter`, the entry a
/// [`filter_input`](crate::widgets::filter_input) writes.
///
/// ```
/// use eario::prelude::*;
///
/// let data = DatasetId::new("data");
/// let filter = State::<Filter>::new("filter");
/// let view = filtered(&data, &filter);
/// assert_eq!(view.filter.as_ref(), Some(filter.key()));
/// ```
pub fn filtered(dataset: &DatasetId, filter: &State<Filter>) -> DataView {
    DataView::of(dataset).filtered_by(filter.key())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use eario_data::ComputeContext;
    use serde_json::json;

    use super::*;

    #[test]
    fn handle_reads_the_parameter_bound_to_its_entry_typed()
    -> Result<(), Box<dyn std::error::Error>> {
        let window = State::<u32>::new("window");
        let context =
            ComputeContext::new(BTreeMap::new(), BTreeMap::from([("width".to_owned(), json!(5))]))
                .bind_state(window.key().clone(), "width");
        let width = context.param(&window)?;
        assert_eq!(width, 5);
        Ok(())
    }

    #[test]
    fn handle_is_cloned_and_shown_by_its_key() {
        let column = State::<String>::new("column");
        assert_eq!(column.clone().key(), column.key());
        assert_eq!(format!("{column:?}"), r#"State("column")"#);
    }
}
