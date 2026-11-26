//! The hub directory: where a hub and the dashboards it starts meet.
//!
//! ```text
//! <hub dir>/
//! ├── registry.json     installed dashboards
//! ├── run/<slug>.json   one per running dashboard, written by the dashboard
//! └── logs/<slug>.log   output of each dashboard
//! ```
//!
//! The directory is `<config dir>/eario/hub/`, or the one named by the
//! `EARIO_HUB_DIR` environment variable. Every file is written atomically: a
//! temporary file in the same directory is renamed over the destination, so a
//! reader sees the old or the new content, never a mixture.
//!
//! Liveness of a dashboard is decided by [`probe`]: the server answers
//! `GET /api/app` with the token recorded in its run file. The process id is
//! not consulted.

use std::fs;
use std::io::{self, Read};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use directories::ProjectDirs;
use eario_core::{Manifest, RunFile, is_slug};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::error::HubDirError;

/// Name of the environment variable that overrides the hub directory.
pub const HUB_DIR_VARIABLE: &str = "EARIO_HUB_DIR";

/// Newest registry format version this build reads and writes.
pub const REGISTRY_FORMAT_VERSION: u32 = 1;

/// Longest a [`probe`] waits for an answer, connection included.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// One installed dashboard in the registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryEntry {
    /// Identifier of the dashboard; equals `manifest.slug`.
    pub slug: String,
    /// Path of the registered executable.
    pub path: PathBuf,
    /// Size of the executable in bytes, as recorded at install time.
    pub size: u64,
    /// SHA-256 of the executable, in lower-case hexadecimal.
    pub sha256: String,
    /// Manifest the executable printed at install time.
    pub manifest: Manifest,
}

/// The installed dashboards, in install order, with one entry per slug.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registry {
    /// Format version of the file, see [`REGISTRY_FORMAT_VERSION`].
    pub format_version: u32,
    /// Installed dashboards.
    pub dashboards: Vec<RegistryEntry>,
}

impl Default for Registry {
    fn default() -> Self {
        Self { format_version: REGISTRY_FORMAT_VERSION, dashboards: Vec::new() }
    }
}

impl Registry {
    /// Finds an entry by slug.
    pub fn get(&self, slug: &str) -> Option<&RegistryEntry> {
        self.dashboards.iter().find(|entry| entry.slug == slug)
    }

    /// Adds an entry, replacing the one with the same slug in place.
    pub fn insert(&mut self, entry: RegistryEntry) {
        match self.dashboards.iter_mut().find(|existing| existing.slug == entry.slug) {
            Some(existing) => *existing = entry,
            None => self.dashboards.push(entry),
        }
    }

    /// Removes the entry with this slug and returns it.
    pub fn remove(&mut self, slug: &str) -> Option<RegistryEntry> {
        let index = self.dashboards.iter().position(|entry| entry.slug == slug)?;
        Some(self.dashboards.remove(index))
    }
}

/// A hub directory on disk. Creating the value touches nothing; directories
/// are created by the first write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HubDir {
    root: PathBuf,
}

impl HubDir {
    /// A hub directory at a known location.
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The hub directory of this user: `EARIO_HUB_DIR` when set and not
    /// empty, otherwise `hub` in the Eario configuration directory.
    pub fn resolve() -> Result<Self, HubDirError> {
        Self::resolve_from(std::env::var_os(HUB_DIR_VARIABLE))
    }

    fn resolve_from(variable: Option<std::ffi::OsString>) -> Result<Self, HubDirError> {
        match variable.filter(|value| !value.is_empty()) {
            Some(root) => Ok(Self::at(root)),
            None => {
                let directories =
                    ProjectDirs::from("", "", "eario").ok_or(HubDirError::NoConfigDir)?;
                Ok(Self::at(directories.config_dir().join("hub")))
            }
        }
    }

    /// The directory itself.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Path of `registry.json`.
    pub fn registry_path(&self) -> PathBuf {
        self.root.join("registry.json")
    }

    /// Directory of the run files.
    pub fn run_dir(&self) -> PathBuf {
        self.root.join("run")
    }

    /// Directory of the log files.
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// Path of the run file of a dashboard. Fails unless `slug` is a slug, so
    /// a name never escapes the directory.
    pub fn run_file_path(&self, slug: &str) -> Result<PathBuf, HubDirError> {
        check_slug(slug)?;
        Ok(self.run_dir().join(format!("{slug}.json")))
    }

    /// Path of the log file of a dashboard. Fails unless `slug` is a slug.
    pub fn log_path(&self, slug: &str) -> Result<PathBuf, HubDirError> {
        check_slug(slug)?;
        Ok(self.logs_dir().join(format!("{slug}.log")))
    }

    /// Reads the registry. A missing file is an empty registry.
    pub fn read_registry(&self) -> Result<Registry, HubDirError> {
        let path = self.registry_path();
        let Some(text) = read_optional(&path)? else {
            return Ok(Registry::default());
        };
        // The version is read first, so a registry of a newer format is
        // reported as such rather than as a parse error.
        #[derive(Deserialize)]
        struct Header {
            format_version: u32,
        }
        let header: Header = parse(&path, &text)?;
        if header.format_version > REGISTRY_FORMAT_VERSION {
            return Err(HubDirError::UnknownRegistryVersion {
                path,
                found: header.format_version,
                supported: REGISTRY_FORMAT_VERSION,
            });
        }
        parse(&path, &text)
    }

    /// Writes the registry atomically.
    pub fn write_registry(&self, registry: &Registry) -> Result<(), HubDirError> {
        write_json_atomic(&self.registry_path(), registry)
    }

    /// Reads the run file of a dashboard; `None` when there is none.
    pub fn read_run_file(&self, slug: &str) -> Result<Option<RunFile>, HubDirError> {
        let path = self.run_file_path(slug)?;
        read_optional(&path)?.map(|text| parse(&path, &text)).transpose()
    }

    /// Writes the run file of a dashboard atomically.
    pub fn write_run_file(&self, slug: &str, run: &RunFile) -> Result<(), HubDirError> {
        write_json_atomic(&self.run_file_path(slug)?, run)
    }

    /// Removes the run file of a dashboard. A missing file is not an error.
    pub fn remove_run_file(&self, slug: &str) -> Result<(), HubDirError> {
        let path = self.run_file_path(slug)?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(HubDirError::Io { path, source }),
        }
    }

    /// Slugs that have a run file, sorted. Files whose name is not a slug are
    /// not run files and are skipped.
    pub fn run_file_slugs(&self) -> Result<Vec<String>, HubDirError> {
        let directory = self.run_dir();
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => return Err(HubDirError::Io { path: directory, source }),
        };
        let mut slugs = Vec::new();
        for entry in entries {
            let entry =
                entry.map_err(|source| HubDirError::Io { path: directory.clone(), source })?;
            let name = entry.file_name();
            let slug = name.to_str().and_then(|name| name.strip_suffix(".json"));
            if let Some(slug) = slug.filter(|slug| is_slug(slug)) {
                slugs.push(slug.to_owned());
            }
        }
        slugs.sort();
        Ok(slugs)
    }
}

fn check_slug(slug: &str) -> Result<(), HubDirError> {
    if is_slug(slug) { Ok(()) } else { Err(HubDirError::InvalidSlug(slug.to_owned())) }
}

/// Reads a text file; `None` when it does not exist.
fn read_optional(path: &Path) -> Result<Option<String>, HubDirError> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(HubDirError::Io { path: path.to_owned(), source }),
    }
}

fn parse<T: for<'de> Deserialize<'de>>(path: &Path, text: &str) -> Result<T, HubDirError> {
    serde_json::from_str(text)
        .map_err(|error| HubDirError::Invalid { path: path.to_owned(), message: error.to_string() })
}

/// Serializes `value` and writes it to `path` through a temporary file in the
/// same directory, then a rename. Creates the directory when missing.
fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), HubDirError> {
    let io_error = |source| HubDirError::Io { path: path.to_owned(), source };
    let text = serde_json::to_string_pretty(value).map_err(|error| HubDirError::Invalid {
        path: path.to_owned(),
        message: error.to_string(),
    })?;
    if let Some(directory) = path.parent() {
        fs::create_dir_all(directory).map_err(io_error)?;
    }
    // Process id and a per-process counter: two writers, in this process or
    // another, never share a temporary file.
    let sequence = TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let mut temporary_name = path.file_name().unwrap_or_default().to_os_string();
    temporary_name.push(format!(".{}-{sequence}.tmp", std::process::id()));
    let temporary = path.with_file_name(temporary_name);
    if let Err(source) =
        fs::write(&temporary, text).and_then(|()| rename_retrying(&temporary, path))
    {
        // Best effort: the failure to report is the first one.
        let _ = fs::remove_file(&temporary);
        return Err(io_error(source));
    }
    Ok(())
}

/// Distinguishes the temporary files of concurrent writes in one process.
static TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Attempts at renaming a temporary file over its destination.
const RENAME_ATTEMPTS: u32 = 3;

/// Renames `from` over `to`, retrying briefly on a permission error: on
/// Windows a reader, an antivirus or the indexer holding the destination open
/// makes the rename fail for a moment.
fn rename_retrying(from: &Path, to: &Path) -> io::Result<()> {
    let mut attempt = 1;
    loop {
        match fs::rename(from, to) {
            Err(error)
                if error.kind() == io::ErrorKind::PermissionDenied && attempt < RENAME_ATTEMPTS =>
            {
                attempt += 1;
                std::thread::sleep(Duration::from_millis(50));
            }
            result => return result,
        }
    }
}

/// Size in bytes and SHA-256 (lower-case hexadecimal) of a file.
///
/// Reads the file in chunks, so an executable is never held in memory.
pub fn file_digest(path: &Path) -> Result<(u64, String), HubDirError> {
    let io_error = |source| HubDirError::Io { path: path.to_owned(), source };
    let mut file = fs::File::open(path).map_err(io_error)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut size = 0_u64;
    loop {
        let read = file.read(&mut buffer).map_err(io_error)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size += read as u64;
    }
    let hex = hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    Ok((size, hex))
}

/// Whether a dashboard server answers at `address` with `token`.
///
/// Sends `GET /api/app` and is live only on a `200` answer, which the server
/// gives only for its own token. Anything else is not live: a closed port, a
/// wrong token, another program, a timeout (see [`PROBE_TIMEOUT`]). The
/// address must be a loopback socket address; any other is not probed. The
/// request is written by hand over a TCP stream: it is one fixed request to
/// the loopback interface, which does not justify an HTTP client dependency.
pub async fn probe(address: &str, token: &str) -> bool {
    let Ok(address) = address.parse::<SocketAddr>() else {
        return false;
    };
    if !address.ip().is_loopback() || token.contains(['\r', '\n']) {
        return false;
    }
    tokio::time::timeout(PROBE_TIMEOUT, probe_request(address, token)).await.unwrap_or(false)
}

async fn probe_request(address: SocketAddr, token: &str) -> bool {
    let request = format!(
        "GET /api/app HTTP/1.1\r\nHost: {address}\r\nx-eario-token: {token}\r\nConnection: close\r\n\r\n"
    );
    let Ok(mut stream) = TcpStream::connect(address).await else {
        return false;
    };
    if stream.write_all(request.as_bytes()).await.is_err() {
        return false;
    }
    // The status line is all that is read: `HTTP/1.1 200 OK`.
    let mut status = [0_u8; 12];
    stream.read_exact(&mut status).await.is_ok() && status.ends_with(b" 200")
}

#[cfg(test)]
mod tests {
    use eario_core::{LAUNCH_PROTOCOL_VERSION, slug};

    use super::*;
    use crate::app::{App, AppOptions};
    use crate::server::{ServerConfig, start};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn manifest(name: &str) -> Manifest {
        Manifest {
            name: name.to_owned(),
            slug: slug(name),
            version: None,
            eario_version: "0.2.0".to_owned(),
            protocol: LAUNCH_PROTOCOL_VERSION,
            description: "A dashboard".to_owned(),
        }
    }

    fn run_file() -> RunFile {
        RunFile {
            protocol: LAUNCH_PROTOCOL_VERSION,
            pid: 7,
            address: "127.0.0.1:49152".to_owned(),
            token: "secret".to_owned(),
            started_at: 1_700_000_000,
        }
    }

    #[test]
    fn registry_round_trips_and_replaces_by_slug() -> TestResult {
        let directory = tempfile::tempdir()?;
        let hub = HubDir::at(directory.path().join("hub"));
        assert_eq!(hub.read_registry()?, Registry::default());

        let entry = |name: &str, size| RegistryEntry {
            slug: slug(name),
            path: PathBuf::from("bin").join(name),
            size,
            sha256: "ab".repeat(32),
            manifest: manifest(name),
        };
        let mut registry = Registry::default();
        registry.insert(entry("Alpha", 1));
        registry.insert(entry("Beta", 2));
        registry.insert(entry("Alpha", 3));
        hub.write_registry(&registry)?;

        let read = hub.read_registry()?;
        assert_eq!(read, registry);
        assert_eq!(read.dashboards.len(), 2);
        assert_eq!(read.get("alpha").map(|entry| entry.size), Some(3));
        assert!(registry.remove("beta").is_some());
        assert!(registry.remove("beta").is_none());
        Ok(())
    }

    #[test]
    fn unknown_registry_version_names_the_file() -> TestResult {
        let directory = tempfile::tempdir()?;
        let hub = HubDir::at(directory.path());
        fs::write(hub.registry_path(), r#"{"format_version": 99, "something": "new"}"#)?;

        let error = hub.read_registry().err().ok_or("a newer registry must be refused")?;
        assert_eq!(error.code(), "hub_registry_unknown_version");
        assert!(error.to_string().contains("registry.json"), "{error}");
        assert!(error.to_string().contains("99"), "{error}");
        Ok(())
    }

    #[test]
    fn invalid_registry_names_the_file() -> TestResult {
        let directory = tempfile::tempdir()?;
        let hub = HubDir::at(directory.path());
        fs::write(hub.registry_path(), "not json")?;

        let error = hub.read_registry().err().ok_or("garbage must be refused")?;
        assert_eq!(error.code(), "hub_file_invalid");
        assert!(error.to_string().contains("registry.json"), "{error}");
        Ok(())
    }

    #[test]
    fn run_file_round_trips_and_is_removed() -> TestResult {
        let directory = tempfile::tempdir()?;
        let hub = HubDir::at(directory.path());
        assert_eq!(hub.read_run_file("alpha")?, None);
        assert!(hub.run_file_slugs()?.is_empty());

        hub.write_run_file("alpha", &run_file())?;
        hub.write_run_file("beta", &run_file())?;
        fs::write(hub.run_dir().join("Not A Slug.json"), "{}")?;
        assert_eq!(hub.read_run_file("alpha")?, Some(run_file()));
        assert_eq!(hub.run_file_slugs()?, ["alpha", "beta"]);

        hub.remove_run_file("alpha")?;
        hub.remove_run_file("alpha")?;
        assert_eq!(hub.read_run_file("alpha")?, None);
        Ok(())
    }

    #[test]
    fn a_name_that_is_not_a_slug_is_never_a_path() {
        let hub = HubDir::at("hub");
        for name in ["../x", "a/b", "", "Upper", "a b"] {
            let error = hub.run_file_path(name).err();
            assert_eq!(error.map(|error| error.code()), Some("hub_invalid_slug"), "{name}");
            assert!(hub.log_path(name).is_err(), "{name}");
        }
    }

    #[test]
    fn atomic_write_replaces_the_file_and_leaves_no_temporary() -> TestResult {
        let directory = tempfile::tempdir()?;
        let hub = HubDir::at(directory.path());
        hub.write_run_file("alpha", &run_file())?;
        let mut newer = run_file();
        newer.pid = 8;
        hub.write_run_file("alpha", &newer)?;

        assert_eq!(hub.read_run_file("alpha")?, Some(newer));
        let names: Vec<_> = fs::read_dir(hub.run_dir())?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<Result<_, _>>()?;
        assert_eq!(names, ["alpha.json"], "no temporary file may remain");
        Ok(())
    }

    #[test]
    fn a_failed_write_keeps_the_previous_file() -> TestResult {
        let directory = tempfile::tempdir()?;
        let hub = HubDir::at(directory.path());
        hub.write_run_file("alpha", &run_file())?;
        // A directory at the destination of the temporary file's rename makes
        // the rename fail on every platform.
        let blocked = hub.run_dir().join("beta.json");
        fs::create_dir_all(&blocked)?;

        let error = hub.write_run_file("beta", &run_file()).err().ok_or("rename must fail")?;
        assert_eq!(error.code(), "hub_file_io");
        assert!(error.to_string().contains("beta.json"), "{error}");
        assert_eq!(hub.read_run_file("alpha")?, Some(run_file()));
        let temporaries = fs::read_dir(hub.run_dir())?
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(temporaries, 0);
        Ok(())
    }

    #[test]
    fn the_environment_override_wins_over_the_default() -> TestResult {
        let hub = HubDir::resolve_from(Some("somewhere".into()))?;
        assert_eq!(hub.root(), Path::new("somewhere"));
        assert_eq!(hub.registry_path(), Path::new("somewhere").join("registry.json"));

        let default = HubDir::resolve_from(None)?;
        assert!(default.root().ends_with("hub"));
        assert!(HubDir::resolve_from(Some("".into()))?.root().ends_with("hub"));
        Ok(())
    }

    #[test]
    fn digest_gives_size_and_sha256() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("file.bin");
        fs::write(&path, b"abc")?;

        let (size, hash) = file_digest(&path)?;
        assert_eq!(size, 3);
        assert_eq!(hash, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");

        let error = file_digest(&directory.path().join("missing")).err().ok_or("missing file")?;
        assert!(error.to_string().contains("missing"), "{error}");
        Ok(())
    }

    #[tokio::test]
    async fn probe_tells_a_live_server_from_the_rest() -> TestResult {
        use eario_core::{ComponentId, ComponentKind, ComponentSpec, DashboardSpec, PageSpec};
        use eario_core::{RowSpec, SPEC_VERSION};

        let spec = DashboardSpec {
            spec_version: SPEC_VERSION,
            title: "Probe".to_owned(),
            datasets: Vec::new(),
            pages: vec![PageSpec {
                id: "main".to_owned(),
                title: "Main".to_owned(),
                rows: vec![RowSpec {
                    items: vec![ComponentSpec {
                        id: ComponentId::new("note"),
                        title: None,
                        span: 12,
                        kind: ComponentKind::Text { text: "hello".to_owned() },
                    }],
                }],
            }],
        };
        let app = App::new(spec, AppOptions::default())?;
        let server =
            start(app, ServerConfig { open_browser: false, ..ServerConfig::default() }).await?;
        let address = server.base_url().trim_start_matches("http://").to_owned();

        assert!(probe(&address, server.token()).await, "right token");
        assert!(!probe(&address, "wrong-token").await, "wrong token");
        assert!(!probe("example.com:80", server.token()).await, "not a loopback address");
        assert!(!probe("not an address", server.token()).await, "not an address");

        server.shutdown();
        server.wait().await?;
        assert!(!probe(&address, "anything").await, "closed port");
        Ok(())
    }
}
