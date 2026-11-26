//! The launch contract between a dashboard executable and a hub.
//!
//! A dashboard executable describes itself with a [`Manifest`] and, when a hub
//! starts it, announces where it listens with a [`RunFile`]. The hub shows
//! the dashboards it manages with the [`HubInfo`] payload of its HTTP API.
//! The types are versioned by [`LAUNCH_PROTOCOL_VERSION`].

use serde::{Deserialize, Serialize};

/// Version of the launch contract: the manifest, the run file and the
/// command-line flags of a dashboard executable.
pub const LAUNCH_PROTOCOL_VERSION: u32 = 1;

/// Number of log lines the hub reports for a dashboard that failed to start.
pub const HUB_LOG_LINES: usize = 20;

/// What a dashboard executable prints for `--eario-manifest`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Manifest {
    /// Name of the dashboard, shown to the user.
    pub name: String,
    /// Identifier of the dashboard in the hub: the name reduced by [`slug`].
    pub slug: String,
    /// Version of the dashboard, when its author declared one.
    #[serde(default)]
    pub version: Option<String>,
    /// Version of the Eario framework the executable was built with.
    pub eario_version: String,
    /// Launch protocol version the executable speaks, see
    /// [`LAUNCH_PROTOCOL_VERSION`].
    pub protocol: u32,
    /// One-line description, empty when the author gave none.
    #[serde(default)]
    pub description: String,
}

/// What a dashboard started by a hub writes once it is listening.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct RunFile {
    /// Launch protocol version of the executable that wrote the file.
    pub protocol: u32,
    /// Process identifier, for display. Liveness is decided by the token, not
    /// by this number.
    pub pid: u32,
    /// Loopback address the server listens on, for example `127.0.0.1:49152`.
    pub address: String,
    /// Session token of the server.
    pub token: String,
    /// Start time, in seconds since the Unix epoch.
    pub started_at: u64,
}

/// State of a dashboard as the hub reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum DashboardState {
    /// The hub started the process and waits for it to answer.
    Starting,
    /// The dashboard answers on its address.
    Running,
    /// The dashboard is not running.
    Stopped,
    /// The last start did not succeed; see the log lines.
    Failed,
}

/// One installed dashboard in the hub API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct HubDashboard {
    /// Identifier of the dashboard in the registry. The only handle the hub
    /// API accepts.
    pub slug: String,
    /// Name of the dashboard.
    pub name: String,
    /// Version of the dashboard, when declared.
    pub version: Option<String>,
    /// One-line description.
    pub description: String,
    /// Current state.
    pub state: DashboardState,
    /// Last lines of the dashboard's log when `state` is `failed`; empty
    /// otherwise.
    pub log_lines: Vec<String>,
}

/// The hub's answer to a list request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct HubInfo {
    /// Version of the Eario framework serving the hub.
    pub eario_version: String,
    /// Installed dashboards, in registry order.
    pub dashboards: Vec<HubDashboard>,
}

/// Reduces a name to lower-case ASCII letters, digits and dashes.
///
/// Runs of other characters become one dash, leading and trailing dashes are
/// dropped, and a name with nothing left becomes `app`. The result is safe to
/// use as a file name, which is why it identifies dashboards in the hub.
pub fn slug(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() { "app".to_owned() } else { slug.to_owned() }
}

/// Whether `text` is exactly what [`slug`] returns for itself.
pub fn is_slug(text: &str) -> bool {
    slug(text) == text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_keeps_letters_and_digits_and_collapses_the_rest() {
        assert_eq!(slug("Data Explorer"), "data-explorer");
        assert_eq!(slug("  RBS / PIXE (v2) "), "rbs-pixe-v2");
        assert_eq!(slug("../.."), "app");
    }

    #[test]
    fn only_slugs_are_slugs() {
        assert!(is_slug("data-explorer"));
        assert!(!is_slug("Data Explorer"));
        assert!(!is_slug("../x"));
        assert!(!is_slug(""));
    }

    #[test]
    fn manifest_and_run_file_round_trip() -> Result<(), serde_json::Error> {
        let manifest = Manifest {
            name: "Data Explorer".to_owned(),
            slug: slug("Data Explorer"),
            version: Some("1.2.0".to_owned()),
            eario_version: "0.2.0".to_owned(),
            protocol: LAUNCH_PROTOCOL_VERSION,
            description: String::new(),
        };
        let text = serde_json::to_string(&manifest)?;
        assert_eq!(serde_json::from_str::<Manifest>(&text)?, manifest);

        let run = RunFile {
            protocol: LAUNCH_PROTOCOL_VERSION,
            pid: 42,
            address: "127.0.0.1:49152".to_owned(),
            token: "t".to_owned(),
            started_at: 1_700_000_000,
        };
        let text = serde_json::to_string(&run)?;
        assert_eq!(serde_json::from_str::<RunFile>(&text)?, run);
        Ok(())
    }

    #[test]
    fn manifest_without_optional_fields_is_accepted() -> Result<(), serde_json::Error> {
        let text = r#"{"name":"A","slug":"a","eario_version":"0.2.0","protocol":1}"#;
        let manifest: Manifest = serde_json::from_str(text)?;
        assert_eq!(manifest.version, None);
        assert_eq!(manifest.description, "");
        Ok(())
    }

    #[test]
    fn dashboard_states_are_snake_case() -> Result<(), serde_json::Error> {
        assert_eq!(serde_json::to_string(&DashboardState::Starting)?, "\"starting\"");
        assert_eq!(serde_json::to_string(&DashboardState::Failed)?, "\"failed\"");
        Ok(())
    }
}
