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
//!     let x = StateKey::new("x");
//!     let y = StateKey::new("y");
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
//! see [`Dashboard::spec`]. The crates behind this facade are re-exported as
//! [`core`], [`data`] and [`runtime`].

mod dashboard;
mod error;
pub mod widgets;

pub use dashboard::{Dashboard, Page};
pub use eario_core as core;
pub use eario_data as data;
pub use eario_runtime as runtime;
pub use error::{Error, Result};
pub use widgets::Widget;

/// Everything an ordinary dashboard needs, in one import.
pub mod prelude {
    pub use eario_core::{DataView, DatasetId, StateKey};

    pub use crate::widgets::{
        Widget, column_select, data_table, dataset_info, file_import, filter_input, line_chart,
        numeric_column_select, scatter_chart, statistics, text,
    };
    pub use crate::{Dashboard, Page, Result};
}
