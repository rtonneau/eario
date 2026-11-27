//! Runtime of the Eario framework.
//!
//! - [`app`]: the application services. An [`App`] owns a dashboard, its
//!   datasets and its state, and answers every question a client can ask.
//!   The HTTP API and the MCP server are both thin adapters over it. It
//!   evaluates the derived datasets with the computations registered in
//!   [`AppOptions::computations`], and caches their tables.
//! - [`server`]: the local HTTP server, bound to the loopback interface,
//!   that serves the API and the embedded frontend, standalone ([`run`]) or
//!   for a hub ([`run_child`]).
//! - [`preferences`]: user preferences stored on the local machine.
//! - [`hub_dir`]: the hub directory (registry, run files) and the liveness probe
//!   of a dashboard started by a hub.
//!
//! Nothing in [`app`] knows about HTTP or a browser, so the same services
//! can later back a desktop shell.

pub mod app;
mod assets;
mod compute;
pub mod error;
mod http;
#[cfg(test)]
mod http_tests;
pub mod hub_dir;
mod log_file;
pub mod preferences;
pub mod server;

pub use app::{App, AppOptions};
pub use error::{AppError, HubDirError, ServeError};
pub use server::{
    RunningServer, ServerConfig, ShutdownHandle, init_file_logging, run, run_child, start,
    start_routes,
};
