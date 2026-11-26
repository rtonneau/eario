//! Runtime of the Eario framework.
//!
//! - [`app`]: the application services. An [`App`] owns a dashboard, its
//!   datasets and its state, and answers every question a client can ask.
//!   The HTTP API and the MCP server are both thin adapters over it.
//! - [`server`]: the local HTTP server, bound to the loopback interface,
//!   that serves the API and the embedded frontend.
//! - [`preferences`]: user preferences stored on the local machine.
//!
//! Nothing in [`app`] knows about HTTP or a browser, so the same services
//! can later back a desktop shell.

pub mod app;
mod assets;
pub mod error;
mod http;
#[cfg(test)]
mod http_tests;
pub mod preferences;
pub mod server;

pub use app::{App, AppOptions};
pub use error::{AppError, ServeError};
pub use server::{RunningServer, ServerConfig, run, start};
