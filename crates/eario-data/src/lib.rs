//! Data layer of the Eario framework.
//!
//! - [`reader`]: the [`TableReader`] interface a data format implements, and
//!   the [`ReaderRegistry`] that picks a reader from a file name. CSV, TSV and
//!   XLSX readers are built in.
//! - [`dataset`]: a loaded [`Dataset`], which keeps the imported table apart
//!   from anything derived from it.
//! - [`query`]: the operations dashboards run on a table: filtering, paging,
//!   chart preparation and descriptive statistics.
//!
//! Tables are [Polars](https://pola.rs) data frames. The functions of
//! [`query`] return the plain contract types of `eario-core`, so nothing
//! above this crate depends on Polars.

pub mod dataset;
mod delimited;
pub mod error;
pub mod query;
pub mod reader;
mod xlsx;

/// The Polars crate this layer is built on, re-exported so that code written
/// against Eario uses the same version and features (including `lazy`).
pub use polars;

pub use dataset::Dataset;
pub use error::DataError;
pub use polars::frame::DataFrame;
pub use reader::{ImportOptions, ReaderRegistry, TableReader};
