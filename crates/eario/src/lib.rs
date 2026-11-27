//! Eario: bring your data to light.
//!
//! Eario is a local-first framework for building interactive dashboards and
//! data exploration tools. A dashboard is described in Rust, compiled into
//! one executable, and served to the browser from the local machine.
//!
//! ```no_run
//! use eario::prelude::*;
//!
//! fn main() -> eario::Result<()> {
//!     let data = DatasetId::new("data");
//!     let x = State::new("x");
//!     let y = State::new("y");
//!
//!     Dashboard::new("Quick look")
//!         .page("Explore", |page| {
//!             page.row([file_import(&data), dataset_info(&data)])
//!                 .row([
//!                     numeric_column_select("X axis", &data, &x, 0),
//!                     numeric_column_select("Y axis", &data, &y, 1),
//!                 ])
//!                 .row([scatter_chart(&data, &x, &y)])
//!         })
//!         .run()
//! }
//! ```
//!
//! The builder produces a serializable [`DashboardSpec`](core::DashboardSpec);
//! see [`Dashboard::spec`]. Custom Rust logic is a
//! [`Computation`](data::Computation) registered with
//! [`Dashboard::computation`]; a derived dataset declared with
//! [`Dashboard::derived`] holds its table, and components read it like an
//! imported one. The crates behind this facade are re-exported as [`core`],
//! [`data`] and [`runtime`].
//!
//! Every executable built with [`Dashboard::run`] can also be installed in
//! and started by an Eario hub, through its `--eario-*` command-line
//! arguments; see [`Dashboard::run`].

mod dashboard;
mod error;
mod launch;
mod state;
pub mod widgets;

pub use dashboard::{Dashboard, Derived, Page};
pub use eario_core as core;
pub use eario_data as data;
/// Polars, the table library Eario is built on, at the version and with the
/// features Eario uses, including the `lazy` API.
///
/// Write computations against this re-export rather than a separate `polars`
/// dependency, so the tables you build are the ones Eario accepts.
///
/// ```
/// use eario::polars::prelude::*;
///
/// let frame = df!("x" => [1.0, 2.0, 3.0, 4.0])?;
/// let large = frame.lazy().filter(col("x").gt(lit(2.0))).collect()?;
/// assert_eq!(large.height(), 2);
/// # Ok::<(), PolarsError>(())
/// ```
pub use eario_data::polars;
pub use eario_runtime as runtime;
pub use error::{Error, LaunchError, Result};
pub use state::{State, filtered};
pub use widgets::{NumberType, Widget};

/// Everything an ordinary dashboard needs, in one import.
pub mod prelude {
    pub use eario_core::{DataView, DatasetId, Filter, StateKey};
    pub use eario_data::{Computation, ComputeContext, ComputeError};

    pub use crate::widgets::{
        Widget, checkbox, column_select, data_table, dataset_info, file_import, filter_input,
        line_chart, number_input, numeric_column_select, scatter_chart, select, select_from_column,
        slider, statistics, text, text_input,
    };
    pub use crate::{Dashboard, Page, Result, State, filtered};
}
