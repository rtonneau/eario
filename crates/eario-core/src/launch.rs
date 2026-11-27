//! The launch contract between a dashboard executable and a hub.
//!
//! A dashboard executable describes itself with a [`Manifest`] and, when a hub
//! starts it, announces where it listens with a [`RunFile`]. The hub shows
//! the dashboards it manages with the [`HubInfo`] payload of its HTTP API.
//! The types are versioned by [`LAUNCH_PROTOCOL_VERSION`].

use serde::{Deserialize, Serialize};

use crate::payload::ApiError;

/// Version of the launch contract: the manifest, the run file and the
/// command-line flags of a dashboard executable.
pub const LAUNCH_PROTOCOL_VERSION: u32 = 1;

/// Number of log lines the hub reports for a dashboard that failed to start.
pub const HUB_LOG_LINES: usize = 20;

/// Argument that starts a dashboard executable for a hub; the path of the run
/// file follows it.
pub const CHILD_ARGUMENT: &str = "--eario-child";

/// Argument that makes a dashboard executable print its [`Manifest`] as one
/// line of JSON, then exit.
pub const MANIFEST_ARGUMENT: &str = "--eario-manifest";

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
    /// The last update check, `None` until one has run in this hub
    /// directory.
    pub last_check: Option<UpdateCheck>,
    /// A newer version of the hub itself that the last check found, while
    /// it is still newer than this hub: what a self-update would install, or
    /// a newer one. `None` otherwise.
    pub available_version: Option<String>,
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
    /// Slug of the dashboard among those the repository's releases carry,
    /// see [`is_slug`]: the hub installs the asset `<slug>-<target
    /// triple>[.exe]`. `None` for a repository whose releases carry one
    /// dashboard. A repository may be listed once per slug.
    #[serde(default)]
    pub slug: Option<String>,
    /// Name of the dashboard, shown to the user.
    pub name: String,
    /// One-line description, empty when the catalog gives none.
    #[serde(default)]
    pub description: String,
    /// Minisign public key (the base64 text of a `.pub` file's key line) the
    /// repository's releases are signed with. The catalog's author vouches for
    /// it. The key belongs to the repository: when a catalog lists it under
    /// several slugs, the first entry's key is the one used.
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
    /// No key is known for this repository: the user must compare the key
    /// and accept it by its SHA-256 to trust it (trust on first use). Until
    /// then the release's executable is not run.
    Unknown,
    /// The key differs from the one pinned for this repository, or from the
    /// one its catalog lists. Refused, and the release's executable not run,
    /// until the user accepts it by its SHA-256.
    Changed,
    /// A key that was `unknown` or `changed`, which the user accepted by its
    /// SHA-256 in this hub; it becomes the pinned key when the release is
    /// installed.
    Accepted,
}

/// What the hub found for an install or an update, shown to the user before
/// anything is written. The hub has downloaded the asset and verified its
/// signature and SHA-256.
///
/// A preview comes in two steps when the key is not trusted yet. While
/// `key_status` is `unknown` or `changed`, the hub has not run the
/// downloaded executable, so `manifest` is `None`: the user first compares
/// the key and accepts it by `key_sha256`. The hub then runs the executable
/// for its manifest and answers the full preview, with `manifest` set and
/// `key_status` `accepted`. Only a full preview can be installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct InstallPreview {
    /// The repository, as `owner/repo`.
    pub repo: String,
    /// Slug the dashboard is installed as: the prefix of the asset's name
    /// for a first install, the installed slug for an update. The manifest
    /// must give the same.
    pub slug: String,
    /// What the verified executable printed for `--eario-manifest`: its
    /// name, version and description. Updates compare its version, not the
    /// tag. `None` while the key is not trusted, as the hub runs no
    /// executable before.
    pub manifest: Option<Manifest>,
    /// Tag of the release, for example `v1.2.0`.
    pub tag: String,
    /// Name of the release asset for this machine.
    pub asset: String,
    /// Size of the asset in bytes.
    pub size: u64,
    /// SHA-256 of the asset from the signed checksum list, in lower-case
    /// hexadecimal. The install request must repeat it.
    pub sha256: String,
    /// The minisign public key the release is signed with: the key line of
    /// its `.pub` file, for the user to compare with the one its author
    /// publishes.
    pub public_key: String,
    /// SHA-256 of `public_key`, in lower-case hexadecimal: what identifies the
    /// key when the user accepts it. The minisign key id does not, as anyone
    /// can make a key with a given id.
    pub key_sha256: String,
    /// How the key relates to what the hub trusts.
    pub key_status: KeyStatus,
}

/// What the last update check found, stored in the hub directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UpdateCheck {
    /// When the check ran, in seconds since the Unix epoch.
    pub checked_at: u64,
    /// One per dashboard installed from GitHub, in registry order.
    pub dashboards: Vec<DashboardUpdate>,
    /// The hub itself. `None` in a check stored by a hub that did not check
    /// itself.
    #[serde(default)]
    pub hub: Option<HubUpdate>,
}

/// What the last update check found for the hub itself, whose releases are
/// verified with the Eario release key compiled into it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct HubUpdate {
    /// The repository the hub's releases come from, as `owner/repo`.
    pub repo: String,
    /// Version of the latest release: its tag without a leading `v`. `None`
    /// when the release was not read.
    pub latest_version: Option<String>,
    /// `latest_version` when it is newer than the hub that checked: a
    /// self-update to offer. `None` otherwise.
    pub available_version: Option<String>,
    /// Why the release was not read: `self_update_not_configured` when this
    /// build of the hub holds no Eario release key, so it does not update
    /// itself (no request is made); otherwise why the release could not be
    /// read. `None` when it was read.
    pub error: Option<ApiError>,
}

/// The hub's answer to a self-update: it replaced its executable with a
/// verified, newer release. Once it has answered, it stops and starts the new
/// executable, at a new address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct HubSelfUpdate {
    /// Version of the hub that answered, now replaced.
    pub previous_version: String,
    /// Version installed, which the restarted hub runs.
    pub version: String,
}

/// What the last update check found for one dashboard installed from
/// GitHub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DashboardUpdate {
    /// The dashboard.
    pub slug: String,
    /// The repository it was installed from, as `owner/repo`.
    pub repo: String,
    /// Version of the latest release: its tag without a leading `v`. `None`
    /// when the release could not be read.
    pub latest_version: Option<String>,
    /// `latest_version` when it is newer than the installed version: an
    /// update to offer. `None` otherwise.
    pub available_version: Option<String>,
    /// Why the latest release could not be read (the dashboard "could not be
    /// checked"); `None` when it was read.
    pub error: Option<ApiError>,
}

/// A catalog the hub reads, with its dashboards.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct HubCatalog {
    /// Repository of the catalog, as `owner/repo`.
    pub repo: String,
    /// Name the catalog gives itself; `None` when it could not be read.
    pub name: Option<String>,
    /// Its dashboards, in its order; empty when it could not be read.
    pub dashboards: Vec<HubCatalogEntry>,
    /// Why the catalog could not be read; `None` when it was.
    pub error: Option<ApiError>,
}

/// One dashboard of a [`HubCatalog`], with its install state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct HubCatalogEntry {
    /// Repository that publishes the dashboard, as `owner/repo`.
    pub repo: String,
    /// Slug of the dashboard among those the repository publishes, which an
    /// install from this entry asks for; `None` when the catalog names none
    /// (the repository publishes one dashboard).
    pub slug: Option<String>,
    /// Name of the dashboard.
    pub name: String,
    /// One-line description.
    pub description: String,
    /// Slug of the dashboard installed from this entry, when there is one:
    /// `slug` installed from this repository, or, for an entry without a
    /// slug, the first dashboard installed from it.
    pub installed_slug: Option<String>,
}

/// Settings of the hub, stored in the hub directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct HubSettings {
    /// Whether the hub checks GitHub for updates of its dashboards when it
    /// starts. On by default.
    #[serde(default = "default_true")]
    pub check_updates_at_start: bool,
}

impl Default for HubSettings {
    fn default() -> Self {
        Self { check_updates_at_start: true }
    }
}

fn default_true() -> bool {
    true
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
        assert_eq!(catalog.dashboards[0].slug, None, "a catalog of format 1 without slugs");
        assert_eq!(serde_json::from_str::<Catalog>(&serde_json::to_string(&catalog)?)?, catalog);
        let text = r#"{"format_version":1,"name":"Lab","dashboards":[
            {"repo":"o/suite","slug":"signal-lab","name":"Signal Lab","public_key":"RWQabc"},
            {"repo":"o/suite","slug":"data-explorer","name":"Data Explorer","public_key":"RWQabc"}]}"#;
        let catalog: Catalog = serde_json::from_str(text)?;
        assert_eq!(catalog.dashboards[1].slug.as_deref(), Some("data-explorer"));
        assert_eq!(serde_json::from_str::<Catalog>(&serde_json::to_string(&catalog)?)?, catalog);

        let pending = InstallPreview {
            repo: "o/r".to_owned(),
            slug: "r".to_owned(),
            manifest: None,
            tag: "v1.0.0".to_owned(),
            asset: "r-x86_64-pc-windows-msvc.exe".to_owned(),
            size: 10,
            sha256: "ab".repeat(32),
            public_key: "RWQabc".to_owned(),
            key_sha256: "cd".repeat(32),
            key_status: KeyStatus::Unknown,
        };
        let text = serde_json::to_string(&pending)?;
        assert!(text.contains(r#""key_status":"unknown""#), "{text}");
        assert!(text.contains(r#""manifest":null"#), "{text}");
        assert_eq!(serde_json::from_str::<InstallPreview>(&text)?, pending);
        let full = InstallPreview {
            manifest: Some(Manifest {
                name: "R".to_owned(),
                slug: "r".to_owned(),
                version: Some("1.0.0".to_owned()),
                eario_version: "0.2.0".to_owned(),
                protocol: LAUNCH_PROTOCOL_VERSION,
                description: String::new(),
            }),
            key_status: KeyStatus::Accepted,
            ..pending
        };
        let text = serde_json::to_string(&full)?;
        assert_eq!(serde_json::from_str::<InstallPreview>(&text)?, full);
        assert_eq!(serde_json::to_string(&KeyStatus::Accepted)?, r#""accepted""#);
        Ok(())
    }

    #[test]
    fn settings_check_for_updates_unless_turned_off() -> Result<(), serde_json::Error> {
        assert!(HubSettings::default().check_updates_at_start);
        assert_eq!(serde_json::from_str::<HubSettings>("{}")?, HubSettings::default());
        let off: HubSettings = serde_json::from_str(r#"{"check_updates_at_start":false}"#)?;
        assert!(!off.check_updates_at_start);
        Ok(())
    }

    #[test]
    fn update_checks_round_trip() -> Result<(), serde_json::Error> {
        let check = UpdateCheck {
            checked_at: 1_700_000_000,
            dashboards: vec![
                DashboardUpdate {
                    slug: "r".to_owned(),
                    repo: "o/r".to_owned(),
                    latest_version: Some("1.1.0".to_owned()),
                    available_version: Some("1.1.0".to_owned()),
                    error: None,
                },
                DashboardUpdate {
                    slug: "s".to_owned(),
                    repo: "o/s".to_owned(),
                    latest_version: None,
                    available_version: None,
                    error: Some(ApiError {
                        code: "github_offline".to_owned(),
                        message: "offline".to_owned(),
                    }),
                },
            ],
            hub: Some(HubUpdate {
                repo: "rtonneau/eario".to_owned(),
                latest_version: Some("0.3.0".to_owned()),
                available_version: Some("0.3.0".to_owned()),
                error: None,
            }),
        };
        let text = serde_json::to_string(&check)?;
        assert_eq!(serde_json::from_str::<UpdateCheck>(&text)?, check);

        // A check stored before the hub checked itself.
        let older: UpdateCheck = serde_json::from_str(r#"{"checked_at":5,"dashboards":[]}"#)?;
        assert_eq!(older.hub, None);

        let updated =
            HubSelfUpdate { previous_version: "0.2.0".to_owned(), version: "0.3.0".to_owned() };
        let text = serde_json::to_string(&updated)?;
        assert_eq!(serde_json::from_str::<HubSelfUpdate>(&text)?, updated);
        Ok(())
    }

    #[test]
    fn dashboard_states_are_snake_case() -> Result<(), serde_json::Error> {
        assert_eq!(serde_json::to_string(&DashboardState::Starting)?, "\"starting\"");
        assert_eq!(serde_json::to_string(&DashboardState::Failed)?, "\"failed\"");
        Ok(())
    }
}
