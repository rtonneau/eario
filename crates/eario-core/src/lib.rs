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
//! - [`launch`]: the contract between a dashboard executable and a hub.
//!
//! It has no dependency on a dataframe engine, a web framework or a browser
//! API, so a specification can be produced by Rust code, read from a file or
//! edited by a tool.

pub mod filter;
pub mod id;
pub mod launch;
pub mod payload;
pub mod spec;

pub use filter::{Filter, FilterOp, FilterValue};
pub use id::{ComponentId, DatasetId, StateKey};
pub use launch::{
    DashboardState, HUB_LOG_LINES, HubDashboard, HubInfo, LAUNCH_PROTOCOL_VERSION, Manifest,
    RunFile, is_slug, slug,
};
pub use payload::{
    ApiError, AppInfo, ChartData, ColumnInfo, ColumnKind, ColumnStats, ComponentData, DatasetInfo,
    DerivedStatus, Preferences, StateMap, StateUpdate, StatsTable, TablePage, Theme,
};
pub use spec::{
    ChartMark, ComponentKind, ComponentSpec, DashboardSpec, DataView, DatasetSlot, DatasetSource,
    Dependency, NumberConstraints, PageSpec, ParamBinding, RowSpec, SPEC_VERSION, SelectOptions,
    SpecError,
};
