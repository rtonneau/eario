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

/// Argument that starts a dashboard executable for a hub; the path of the run
/// file follows it.
pub const CHILD_ARGUMENT: &str = "--eario-child";

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

/// Where an installed dashboard came from, and so how it is updated.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    /// Installed from an executable on this machine with `--eario-install`.
    /// Never updated from GitHub.
    #[default]
    Local,
    /// Installed from a release of a GitHub repository.
    Github {
        /// The repository, as `owner/repo`; see [`is_repo`].
        repo: String,
    },
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
    /// Where the dashboard was installed from.
    pub source: Source,
    /// Newer version found by the last update check, when there is one.
    pub available_version: Option<String>,
    /// Whether an update left a previous version that can be restored.
    pub has_previous: bool,
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

/// The hub's answer to an open request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct HubOpen {
    /// Address to open in a new tab: the dashboard's own address, with its
    /// session token in the fragment.
    pub url: String,
}

/// Version of the catalog file format; see [`Catalog`].
pub const CATALOG_FORMAT_VERSION: u32 = 1;

/// A list of dashboards published on GitHub: the `catalog.json` file of a
/// repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Catalog {
    /// Format version of the file, see [`CATALOG_FORMAT_VERSION`].
    pub format_version: u32,
    /// Name of the catalog, shown to the user.
    pub name: String,
    /// The dashboards it lists.
    pub dashboards: Vec<CatalogEntry>,
}

/// One dashboard in a [`Catalog`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CatalogEntry {
    /// Repository that publishes the dashboard, as `owner/repo`; see
    /// [`is_repo`].
    pub repo: String,
    /// Name of the dashboard, shown to the user.
    pub name: String,
    /// One-line description, empty when the catalog gives none.
    #[serde(default)]
    pub description: String,
    /// Minisign public key (the base64 text of a `.pub` file's key line) the
    /// repository's releases are signed with. The catalog's author vouches for
    /// it.
    pub public_key: String,
}

/// How the key that signed a release relates to what the hub trusts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum KeyStatus {
    /// The key comes from a catalog entry.
    Catalog,
    /// The key is the one already pinned for this repository.
    Pinned,
    /// No key is known for this repository: the user must confirm the
    /// fingerprint to trust it (trust on first use).
    Unknown,
    /// The key differs from the one pinned for this repository. Refused.
    Changed,
}

/// What the hub found for an install, shown to the user before anything is
/// written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct InstallPreview {
    /// The repository, as `owner/repo`.
    pub repo: String,
    /// Version of the release that would be installed.
    pub version: String,
    /// Name of the release asset for this machine.
    pub asset: String,
    /// Size of the asset in bytes.
    pub size: u64,
    /// SHA-256 of the asset from the signed checksum list, in lower-case
    /// hexadecimal. The install request must repeat it.
    pub sha256: String,
    /// Fingerprint of the signing key, for the user to compare.
    pub key_fingerprint: String,
    /// How the key relates to what the hub trusts.
    pub key_status: KeyStatus,
}

/// Whether `text` is a GitHub repository, as `owner/repo`.
///
/// The owner has 1 to 39 characters among ASCII letters, digits and `-`; the
/// repository 1 to 100 among ASCII letters, digits, `.`, `_` and `-`, and is
/// neither `.` nor `..`. Nothing else is accepted: no URL, scheme, path,
/// query or space. Such a text is safe in a URL path and never a path on disk
/// by itself.
pub fn is_repo(text: &str) -> bool {
    let Some((owner, repo)) = text.split_once('/') else {
        return false;
    };
    (1..=39).contains(&owner.len())
        && owner.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        && (1..=100).contains(&repo.len())
        && repo
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        && repo != "."
        && repo != ".."
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
    fn repositories_are_strict() {
        for good in ["rtonneau/eario", "a/b", "Some-Owner/my.repo_1-x", "o/.hidden", "o/a..b"] {
            assert!(is_repo(good), "{good}");
        }
        let long_owner = format!("{}/r", "a".repeat(40));
        let long_repo = format!("o/{}", "r".repeat(101));
        let bad = [
            "",
            "/",
            "owner",
            "owner/",
            "/repo",
            "a/b/c",
            "o/.",
            "o/..",
            "own_er/repo",
            "owner/re po",
            "owner/rép",
            "ow ner/repo",
            "owner/repo?x=1",
            "owner/repo#x",
            "owner/repo\n",
            "https://github.com/owner/repo",
            "github.com/owner/repo",
            "../repo",
            "owner/..%2f",
            "owner\\repo",
            &long_owner,
            &long_repo,
        ];
        for text in bad {
            assert!(!is_repo(text), "{text:?}");
        }
        assert!(is_repo(&format!("{}/{}", "a".repeat(39), "r".repeat(100))));
    }

    #[test]
    fn sources_are_tagged_and_local_by_default() -> Result<(), serde_json::Error> {
        assert_eq!(Source::default(), Source::Local);
        assert_eq!(serde_json::to_string(&Source::Local)?, r#"{"kind":"local"}"#);
        let github = Source::Github { repo: "o/r".to_owned() };
        let text = serde_json::to_string(&github)?;
        assert_eq!(text, r#"{"kind":"github","repo":"o/r"}"#);
        assert_eq!(serde_json::from_str::<Source>(&text)?, github);
        Ok(())
    }

    #[test]
    fn catalogs_and_previews_round_trip() -> Result<(), serde_json::Error> {
        let text = r#"{"format_version":1,"name":"Lab","dashboards":[
            {"repo":"o/r","name":"Signal Lab","public_key":"RWQabc"}]}"#;
        let catalog: Catalog = serde_json::from_str(text)?;
        assert_eq!(catalog.dashboards[0].description, "");
        assert_eq!(serde_json::from_str::<Catalog>(&serde_json::to_string(&catalog)?)?, catalog);

        let preview = InstallPreview {
            repo: "o/r".to_owned(),
            version: "1.0.0".to_owned(),
            asset: "r-x86_64-pc-windows-msvc.exe".to_owned(),
            size: 10,
            sha256: "ab".repeat(32),
            key_fingerprint: "AB12".to_owned(),
            key_status: KeyStatus::Unknown,
        };
        let text = serde_json::to_string(&preview)?;
        assert!(text.contains(r#""key_status":"unknown""#), "{text}");
        assert_eq!(serde_json::from_str::<InstallPreview>(&text)?, preview);
        Ok(())
    }

    #[test]
    fn dashboard_states_are_snake_case() -> Result<(), serde_json::Error> {
        assert_eq!(serde_json::to_string(&DashboardState::Starting)?, "\"starting\"");
        assert_eq!(serde_json::to_string(&DashboardState::Failed)?, "\"failed\"");
        Ok(())
    }
}
