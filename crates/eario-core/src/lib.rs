//! Core vocabulary of the Eario framework.
//!
//! This crate holds everything the backend and the generic frontend agree on:
//!
//! - [`spec`]: the serializable dashboard specification (structure, layout,
//!   components, data references).
//! - [`filter`]: the row filter a user can apply to a dataset.
//! - [`payload`]: the data a component receives at runtime, and the envelopes
//!   of the HTTP API.
//! - [`id`]: typed identifiers.
//!
//! It has no dependency on a dataframe engine, a web framework or a browser
//! API, so a specification can be produced by Rust code, read from a file or
//! edited by a tool.

pub mod filter;
pub mod id;
pub mod payload;
pub mod spec;

pub use filter::{Filter, FilterOp, FilterValue};
pub use id::{ComponentId, DatasetId, StateKey};
pub use payload::{
    ApiError, AppInfo, ChartData, ColumnInfo, ColumnKind, ColumnStats, ComponentData, DatasetInfo,
    DerivedStatus, Preferences, StateMap, StateUpdate, StatsTable, TablePage, Theme,
};
pub use spec::{
    ChartMark, ComponentKind, ComponentSpec, DashboardSpec, DataView, DatasetSlot, DatasetSource,
    Dependency, PageSpec, ParamBinding, RowSpec, SPEC_VERSION, SpecError,
};
