//! The hub directory: where a hub and the dashboards it starts meet.
//!
//! ```text
//! <hub dir>/
//! ├── registry.json     installed dashboards
//! ├── hub.json          address and token of the running hub
//! ├── catalogs.json     catalogs the hub reads (written once the user changes them)
//! ├── settings.json     the hub's settings (written once the user changes them)
//! ├── updates.json      result of the last update check
//! ├── self-update.log   a line per self-update after which the new hub could not start
//! ├── hub.lock          locked while a process changes the files above
//! ├── bin/<slug>/       the hub's copy of each installed executable
//! ├── staging/          release assets being verified, one directory per process
//! ├── run/<slug>.json   one per running dashboard, written by the dashboard
//! └── logs/<slug>.log   output of each dashboard
//! ```
//!
//! The directory is `<config dir>/eario/hub/`, or the one named by the
//! `EARIO_HUB_DIR` environment variable. Every file is written atomically: a
//! temporary file in the same directory is renamed over the destination, so a
//! reader sees the old or the new content, never a mixture. A process that
//! reads a file, changes it and writes it back holds [`HubDir::lock`]
//! meanwhile, so two processes never lose each other's change. On Unix the
//! directories this module creates are readable by the user only (`0700`),
//! and so are the files it writes (`0600`): run files and `hub.json` hold
//! session tokens.
//!
//! Liveness of a dashboard is decided by [`probe`]: the server answers
//! `GET /api/app` with the token recorded in its run file. The process id is
//! not consulted. [`request_shutdown`] asks such a server to stop.

use std::fs;
use std::io::{self, Read, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use directories::ProjectDirs;
use eario_core::{Manifest, RunFile, Source, is_slug};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::error::HubDirError;

/// Name of the environment variable that overrides the hub directory.
pub const HUB_DIR_VARIABLE: &str = "EARIO_HUB_DIR";

/// Newest registry format version this build reads and writes.
///
/// Format 2 added `source`, `pinned_key` and `previous` to the entries. A
/// format-1 file is read as local entries with no key and no previous
/// version, and is written back as format 2.
pub const REGISTRY_FORMAT_VERSION: u32 = 2;

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
    /// Where the dashboard was installed from. Absent in a format-1 file,
    /// which reads as [`Source::Local`].
    #[serde(default)]
    pub source: Source,
    /// Minisign public key pinned for a GitHub install, as the base64 text of
    /// its key line; the key every later release of this repository must be
    /// signed with. `None` for a local install.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_key: Option<String>,
    /// The version this one replaced, kept for rollback; `None` when there is
    /// none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous: Option<PreviousVersion>,
}

/// The copy kept of the version an update replaced, in the same `bin/<slug>/`
/// directory as the current one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviousVersion {
    /// File name of the copy inside `bin/<slug>/`. Never a path: it is joined
    /// to that directory by the hub.
    pub file_name: String,
    /// Size of the copy in bytes.
    pub size: u64,
    /// SHA-256 of the copy, in lower-case hexadecimal.
    pub sha256: String,
    /// Manifest of that version.
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

    /// Path of `hub.json`, where the running hub records its address and
    /// token in the [`RunFile`] format, so a second start finds it.
    pub fn hub_file_path(&self) -> PathBuf {
        self.root.join("hub.json")
    }

    /// Path of `catalogs.json`: the repositories of the catalogs the hub
    /// reads.
    pub fn catalogs_path(&self) -> PathBuf {
        self.root.join("catalogs.json")
    }

    /// Path of `settings.json`: the hub's settings.
    pub fn settings_path(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    /// Path of `updates.json`: what the last update check found.
    pub fn updates_path(&self) -> PathBuf {
        self.root.join("updates.json")
    }

    /// Path of `self-update.log`, where a hub that replaced its own
    /// executable but could not start the new one says so, since it may have
    /// no terminal. Never a dashboard's log: those are in `logs/`.
    pub fn self_update_log_path(&self) -> PathBuf {
        self.root.join("self-update.log")
    }

    /// Directory where release assets are downloaded and verified before
    /// they are moved into `bin/<slug>/`. On the same volume as `bin/`, so
    /// the move is a rename.
    pub fn staging_dir(&self) -> PathBuf {
        self.root.join("staging")
    }

    /// Path of `hub.lock`, the file [`HubDir::lock`] locks.
    pub fn lock_path(&self) -> PathBuf {
        self.root.join("hub.lock")
    }

    /// Takes the lock of the hub directory, waiting while another holder,
    /// in this process or another, has it; see [`HubLock`]. Creates the
    /// directory and `hub.lock` when missing.
    ///
    /// Blocking. The lock is not reentrant: a thread holding it must not ask
    /// for it again.
    pub fn lock(&self) -> Result<HubLock, HubDirError> {
        create_private_dir_all(&self.root)
            .map_err(|source| HubDirError::Io { path: self.root.clone(), source })?;
        let path = self.lock_path();
        let io_error = |source| HubDirError::Io { path: path.clone(), source };
        let file = private_file_options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(io_error)?;
        file.lock().map_err(io_error)?;
        Ok(HubLock { _file: file })
    }

    /// Directory of the run files.
    pub fn run_dir(&self) -> PathBuf {
        self.root.join("run")
    }

    /// Directory of the log files.
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// Directory holding the hub's copy of an installed executable. Fails
    /// unless `slug` is a slug.
    pub fn bin_dir(&self, slug: &str) -> Result<PathBuf, HubDirError> {
        check_slug(slug)?;
        Ok(self.root.join("bin").join(slug))
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

    /// Reads the registry. A missing file is an empty registry; a format-1
    /// file is read as local entries (see [`REGISTRY_FORMAT_VERSION`]).
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
        let header: Header = parse(&path, &text).map_err(invalid_registry)?;
        if header.format_version > REGISTRY_FORMAT_VERSION {
            return Err(HubDirError::UnknownRegistryVersion {
                path,
                found: header.format_version,
                supported: REGISTRY_FORMAT_VERSION,
            });
        }
        let mut registry: Registry = parse(&path, &text).map_err(invalid_registry)?;
        // An older file is upgraded in memory; the next write stores format 2.
        registry.format_version = REGISTRY_FORMAT_VERSION;
        Ok(registry)
    }

    /// Writes the registry atomically.
    pub fn write_registry(&self, registry: &Registry) -> Result<(), HubDirError> {
        write_json_atomic(&self.registry_path(), registry)
    }

    /// Reads the run file of a dashboard; `None` when there is none.
    pub fn read_run_file(&self, slug: &str) -> Result<Option<RunFile>, HubDirError> {
        read_run_file_at(&self.run_file_path(slug)?)
    }

    /// Writes the run file of a dashboard atomically.
    pub fn write_run_file(&self, slug: &str, run: &RunFile) -> Result<(), HubDirError> {
        write_run_file_at(&self.run_file_path(slug)?, run)
    }

    /// Removes the run file of a dashboard. A missing file is not an error.
    pub fn remove_run_file(&self, slug: &str) -> Result<(), HubDirError> {
        remove_run_file_at(&self.run_file_path(slug)?)
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

/// The lock of a hub directory, taken by [`HubDir::lock`] and released when
/// dropped.
///
/// It is held around every change that reads a file of the hub directory,
/// changes it and writes it back: the registry, with the hub's copies in
/// `bin/`, and `catalogs.json` and `settings.json`. Every Eario process
/// takes it for such a change: a hub, the `eario-hub` command line, a
/// dashboard executable given `--eario-install` or `--eario-uninstall`. A
/// write is atomic without it; what the lock prevents is a change made
/// between another process's read and its write, which that write would
/// undo.
///
/// It is an operating-system lock on `hub.lock` (`flock` on Unix,
/// `LockFileEx` on Windows), so it is released when the process ends, even
/// by a crash. It is never held while waiting for the user or the network.
#[derive(Debug)]
#[must_use = "the lock is released as soon as it is dropped"]
pub struct HubLock {
    _file: fs::File,
}

/// Reads the run file at `path`; `None` when there is none.
pub fn read_run_file_at(path: &Path) -> Result<Option<RunFile>, HubDirError> {
    read_json_at(path)
}

/// Reads the JSON file at `path`; `None` when it does not exist. A file that
/// does not parse as `T` is [`HubDirError::Invalid`], naming it.
pub fn read_json_at<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Option<T>, HubDirError> {
    read_optional(path)?.map(|text| parse(path, &text)).transpose()
}

/// Writes `value` as JSON at `path` atomically, like every file of the hub
/// directory: through a temporary file in the same directory, then a
/// rename. Creates the directory when missing; on Unix the file is readable
/// by the user only.
pub fn write_json_at<T: Serialize>(path: &Path, value: &T) -> Result<(), HubDirError> {
    write_json_atomic(path, value)
}

/// Creates the directory `path` and its missing parents, readable by the
/// user only on Unix (`0700`), as the hub directory's own.
pub fn create_private_dir(path: &Path) -> Result<(), HubDirError> {
    create_private_dir_all(path).map_err(|source| HubDirError::Io { path: path.to_owned(), source })
}

/// Writes a run file atomically at `path`, creating its directory when
/// missing.
///
/// A dashboard started with `--eario-child <run file>` writes there the path
/// its hub chose, normally [`HubDir::run_file_path`].
pub fn write_run_file_at(path: &Path, run: &RunFile) -> Result<(), HubDirError> {
    write_json_atomic(path, run)
}

/// Removes the run file at `path`. A missing file is not an error.
pub fn remove_run_file_at(path: &Path) -> Result<(), HubDirError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(HubDirError::Io { path: path.to_owned(), source }),
    }
}

/// Removes the run file at `path` if it still records `token`: when the
/// dashboard was started again meanwhile, the file is the new process's and
/// is kept. A file that cannot be parsed is removed too; a missing file is
/// not an error.
pub fn remove_own_run_file_at(path: &Path, token: &str) -> Result<(), HubDirError> {
    match read_run_file_at(path) {
        Ok(Some(run)) if run.token != token => Ok(()),
        Ok(_) | Err(HubDirError::Invalid { .. }) => remove_run_file_at(path),
        Err(error) => Err(error),
    }
}

/// Copies the file `from` to `to` through a temporary file beside `to`, then
/// a rename, so `to` is never left half-written. Creates the directory of
/// `to` when missing. An error names `to`, or `from` when it cannot be read.
pub fn copy_atomic(from: &Path, to: &Path) -> Result<(), HubDirError> {
    if let Some(directory) = to.parent() {
        create_private_dir_all(directory)
            .map_err(|source| HubDirError::Io { path: directory.to_owned(), source })?;
    }
    // Checked first, so an unreadable source is named as such.
    fs::metadata(from).map_err(|source| HubDirError::Io { path: from.to_owned(), source })?;
    let temporary = temporary_path(to);
    // `fs::copy` keeps the permissions, so an executable stays executable.
    if let Err(source) = fs::copy(from, &temporary).and_then(|_| rename_retrying(&temporary, to)) {
        // Best effort: the failure to report is the first one.
        let _ = fs::remove_file(&temporary);
        return Err(HubDirError::Io { path: to.to_owned(), source });
    }
    Ok(())
}

/// Appends `line` and a line break to the file at `path`, created when
/// missing with its directory, readable by the user only on Unix. For a log
/// that must outlive the process writing it; not atomic.
pub fn append_line_at(path: &Path, line: &str) -> Result<(), HubDirError> {
    let io_error = |source| HubDirError::Io { path: path.to_owned(), source };
    if let Some(directory) = path.parent() {
        create_private_dir_all(directory)
            .map_err(|source| HubDirError::Io { path: directory.to_owned(), source })?;
    }
    let mut file = private_file_options().append(true).create(true).open(path).map_err(io_error)?;
    writeln!(file, "{line}").map_err(io_error)?;
    file.sync_all().map_err(io_error)
}

/// Moves the file `from` to `to`, which must be on the same volume, with one
/// rename: `to` is never seen half-written. Retries briefly while another
/// program (an antivirus, on Windows) holds either file. An error names
/// `to`.
pub fn move_file(from: &Path, to: &Path) -> Result<(), HubDirError> {
    rename_retrying(from, to).map_err(|source| HubDirError::Io { path: to.to_owned(), source })
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

/// The registry's own error for a file that does not parse, which says how
/// to recover.
fn invalid_registry(error: HubDirError) -> HubDirError {
    match error {
        HubDirError::Invalid { path, message } => HubDirError::InvalidRegistry { path, message },
        other => other,
    }
}

/// Creates `path` and its missing parents; on Unix, readable by the user
/// only. Directories that exist already are left as they are.
pub(crate) fn create_private_dir_all(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

/// Options that create a file readable and writable by the user only on
/// Unix. The caller chooses how to open it.
pub(crate) fn private_file_options() -> fs::OpenOptions {
    let options = fs::OpenOptions::new();
    #[cfg(unix)]
    let options = {
        use std::os::unix::fs::OpenOptionsExt;
        let mut options = options;
        options.mode(0o600);
        options
    };
    options
}

/// Writes `bytes` to a new or truncated file at `path`, readable by the user
/// only on Unix.
fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = private_file_options().write(true).create(true).truncate(true).open(path)?;
    file.write_all(bytes)
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
        create_private_dir_all(directory).map_err(io_error)?;
    }
    let temporary = temporary_path(path);
    if let Err(source) =
        write_private(&temporary, text.as_bytes()).and_then(|()| rename_retrying(&temporary, path))
    {
        // Best effort: the failure to report is the first one.
        let _ = fs::remove_file(&temporary);
        return Err(io_error(source));
    }
    Ok(())
}

/// A temporary file beside `path`, named after it, the process id and a
/// per-process counter: two writers, in this process or another, never share
/// a temporary file.
fn temporary_path(path: &Path) -> PathBuf {
    let sequence = TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let mut temporary_name = path.file_name().unwrap_or_default().to_os_string();
    temporary_name.push(format!(".{}-{sequence}.tmp", std::process::id()));
    path.with_file_name(temporary_name)
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
    request_status(address, "GET", "/api/app", token).await == Some(200)
}

/// Asks the server at `address` to stop, with `POST /api/shutdown` and
/// `token`. True when it accepted (`204`); the server then stops on its own
/// within a few seconds. Same rules as [`probe`]: loopback addresses only,
/// [`PROBE_TIMEOUT`] for the whole exchange.
pub async fn request_shutdown(address: &str, token: &str) -> bool {
    request_status(address, "POST", "/api/shutdown", token).await == Some(204)
}

/// [`probe`] for a caller outside an asynchronous runtime, such as the
/// command line of a dashboard executable. Runs the probe on a runtime of its
/// own, so it must not be called from asynchronous code. Fails only when that
/// runtime cannot be created.
pub fn probe_blocking(address: &str, token: &str) -> io::Result<bool> {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    Ok(runtime.block_on(probe(address, token)))
}

/// Status code of one request without a body, carrying `token`, to the
/// server at `address`; what [`probe`] and [`request_shutdown`] send. `None`
/// when the address is not a loopback socket address, the method, path or
/// token could split the request, or no answer came within
/// [`PROBE_TIMEOUT`].
pub async fn request_status(address: &str, method: &str, path: &str, token: &str) -> Option<u16> {
    let address = address.parse::<SocketAddr>().ok()?;
    let splits = |text: &str| text.contains(['\r', '\n', ' ']);
    if !address.ip().is_loopback() || splits(method) || splits(path) || token.contains(['\r', '\n'])
    {
        return None;
    }
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nx-eario-token: {token}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    tokio::time::timeout(PROBE_TIMEOUT, exchange(address, request)).await.ok().flatten()
}

async fn exchange(address: SocketAddr, request: String) -> Option<u16> {
    let mut stream = TcpStream::connect(address).await.ok()?;
    stream.write_all(request.as_bytes()).await.ok()?;
    // The status line is all that is read: `HTTP/1.1 200 OK`.
    let mut status = [0_u8; 12];
    stream.read_exact(&mut status).await.ok()?;
    let code = status.strip_prefix(b"HTTP/1.1 ")?;
    std::str::from_utf8(code).ok()?.parse().ok()
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
            source: Source::Local,
            pinned_key: None,
            previous: None,
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
    fn a_format_1_registry_reads_as_local_entries() -> TestResult {
        let directory = tempfile::tempdir()?;
        let hub = HubDir::at(directory.path());
        let old = format!(
            r#"{{"format_version": 1, "dashboards": [{{
                "slug": "alpha", "path": "bin/alpha/alpha", "size": 5,
                "sha256": "{}", "manifest": {}}}]}}"#,
            "ab".repeat(32),
            serde_json::to_string(&manifest("Alpha"))?,
        );
        fs::write(hub.registry_path(), old)?;

        let registry = hub.read_registry()?;
        assert_eq!(registry.format_version, REGISTRY_FORMAT_VERSION);
        let entry = registry.get("alpha").ok_or("alpha must be read")?;
        assert_eq!(entry.source, Source::Local);
        assert_eq!(entry.pinned_key, None);
        assert_eq!(entry.previous, None);

        // Writing stores format 2, and that reads back unchanged.
        hub.write_registry(&registry)?;
        let text = fs::read_to_string(hub.registry_path())?;
        assert!(text.contains(r#""format_version": 2"#), "{text}");
        assert_eq!(hub.read_registry()?, registry);
        Ok(())
    }

    #[test]
    fn source_key_and_previous_version_round_trip() -> TestResult {
        let directory = tempfile::tempdir()?;
        let hub = HubDir::at(directory.path());
        let mut registry = Registry::default();
        registry.insert(RegistryEntry {
            slug: "alpha".to_owned(),
            path: PathBuf::from("bin/alpha/alpha"),
            size: 9,
            sha256: "cd".repeat(32),
            manifest: manifest("Alpha"),
            source: Source::Github { repo: "owner/alpha".to_owned() },
            pinned_key: Some("RWQabc".to_owned()),
            previous: Some(PreviousVersion {
                file_name: "alpha.previous".to_owned(),
                size: 8,
                sha256: "ef".repeat(32),
                manifest: manifest("Alpha"),
            }),
        });
        hub.write_registry(&registry)?;
        assert_eq!(hub.read_registry()?, registry);
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
    fn invalid_registry_names_the_file_and_how_to_recover() -> TestResult {
        let directory = tempfile::tempdir()?;
        let hub = HubDir::at(directory.path());
        for garbage in ["not json", r#"{"format_version": 1, "dashboards": "none"}"#] {
            fs::write(hub.registry_path(), garbage)?;
            let error = hub.read_registry().err().ok_or("garbage must be refused")?;
            assert_eq!(error.code(), "hub_registry_invalid");
            let message = error.to_string();
            assert!(message.contains("registry.json"), "{message}");
            assert!(message.contains("restore") && message.contains("delete"), "{message}");
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn hub_files_are_readable_by_the_user_only() -> TestResult {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir()?;
        let hub = HubDir::at(directory.path().join("hub"));
        hub.write_registry(&Registry::default())?;
        hub.write_run_file("alpha", &run_file())?;
        write_run_file_at(&hub.hub_file_path(), &run_file())?;
        let source = directory.path().join("source.bin");
        fs::write(&source, b"x")?;
        copy_atomic(&source, &hub.bin_dir("alpha")?.join("alpha"))?;
        crate::log_file::RotatingFile::open(&hub.log_path("alpha")?, 1024)?;
        append_line_at(&hub.self_update_log_path(), "a line")?;

        let mode = |path: &Path| fs::metadata(path).map(|metadata| metadata.permissions().mode());
        let directories =
            [hub.root().to_owned(), hub.run_dir(), hub.logs_dir(), hub.bin_dir("alpha")?];
        for path in directories {
            assert_eq!(mode(&path)? & 0o777, 0o700, "{}", path.display());
        }
        let files = [
            hub.registry_path(),
            hub.run_file_path("alpha")?,
            hub.hub_file_path(),
            hub.self_update_log_path(),
        ];
        for path in files.iter().chain([&hub.log_path("alpha")?]) {
            assert_eq!(mode(path)? & 0o777, 0o600, "{}", path.display());
        }
        Ok(())
    }

    #[test]
    fn appended_lines_are_kept_in_order() -> TestResult {
        let directory = tempfile::tempdir()?;
        let hub = HubDir::at(directory.path().join("not created yet"));
        let log = hub.self_update_log_path();
        assert_eq!(log.parent(), Some(hub.root()), "beside registry.json, not in logs/");
        append_line_at(&log, "first")?;
        append_line_at(&log, "second")?;
        assert_eq!(fs::read_to_string(&log)?, "first\nsecond\n");
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
            assert!(hub.bin_dir(name).is_err(), "{name}");
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
    fn the_hub_lock_excludes_every_other_holder_until_dropped() -> TestResult {
        let directory = tempfile::tempdir()?;
        let hub = HubDir::at(directory.path().join("hub"));
        let held = hub.lock()?;
        assert!(hub.lock_path().is_file(), "the lock file is created with its directory");

        // Any other handle, as another process would open, is refused.
        let other = fs::OpenOptions::new().read(true).write(true).open(hub.lock_path())?;
        assert!(matches!(other.try_lock(), Err(fs::TryLockError::WouldBlock)), "the lock is held");

        // A second writer waits until the first is done.
        let (sender, receiver) = std::sync::mpsc::channel();
        let second = {
            let hub = hub.clone();
            std::thread::spawn(move || -> Result<(), HubDirError> {
                let lock = hub.lock()?;
                // Gone only when the test failed already.
                let _ = sender.send(());
                drop(lock);
                Ok(())
            })
        };
        assert_eq!(receiver.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty));
        drop(held);
        receiver.recv()?;
        second.join().map_err(|_| "the second writer panicked")??;

        // Released by both: anyone can take it.
        other.try_lock().map_err(io::Error::from)?;
        Ok(())
    }

    #[test]
    fn run_file_at_a_given_path_round_trips() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("run").join("alpha.json");
        write_run_file_at(&path, &run_file())?;
        assert_eq!(HubDir::at(directory.path()).read_run_file("alpha")?, Some(run_file()));
        assert_eq!(read_run_file_at(&path)?, Some(run_file()));
        remove_run_file_at(&path)?;
        assert_eq!(read_run_file_at(&path)?, None);

        write_run_file_at(&path, &run_file())?;
        remove_own_run_file_at(&path, "another-token")?;
        assert_eq!(read_run_file_at(&path)?, Some(run_file()), "another server's file is kept");
        remove_own_run_file_at(&path, &run_file().token)?;
        assert_eq!(read_run_file_at(&path)?, None);
        remove_own_run_file_at(&path, &run_file().token)?;
        remove_run_file_at(&path)?;
        assert!(!path.exists());
        Ok(())
    }

    #[test]
    fn copy_replaces_the_destination_and_names_a_missing_source() -> TestResult {
        let directory = tempfile::tempdir()?;
        let hub = HubDir::at(directory.path());
        let source = directory.path().join("source.bin");
        fs::write(&source, b"new")?;
        let destination = hub.bin_dir("alpha")?.join("alpha.exe");
        copy_atomic(&source, &destination)?;
        fs::write(&source, b"newer")?;
        copy_atomic(&source, &destination)?;
        assert_eq!(fs::read(&destination)?, b"newer");
        let names: Vec<_> = fs::read_dir(hub.bin_dir("alpha")?)?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<Result<_, _>>()?;
        assert_eq!(names, ["alpha.exe"], "no temporary file may remain");

        let missing = directory.path().join("missing.bin");
        let error = copy_atomic(&missing, &destination).err().ok_or("missing source")?;
        assert!(error.to_string().contains("missing.bin"), "{error}");
        assert_eq!(fs::read(&destination)?, b"newer");
        Ok(())
    }

    #[test]
    fn the_environment_override_wins_over_the_default() -> TestResult {
        let hub = HubDir::resolve_from(Some("somewhere".into()))?;
        assert_eq!(hub.root(), Path::new("somewhere"));
        assert_eq!(hub.registry_path(), Path::new("somewhere").join("registry.json"));
        assert_eq!(hub.hub_file_path(), Path::new("somewhere").join("hub.json"));

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

        assert!(!request_shutdown(&address, "wrong-token").await, "a wrong token cannot stop it");
        assert!(probe(&address, server.token()).await, "still running");
        assert!(request_shutdown(&address, server.token()).await, "the right token stops it");
        server.wait().await?;
        assert!(!probe(&address, "anything").await, "closed port");
        assert!(!request_shutdown(&address, "anything").await, "nothing to stop");
        Ok(())
    }

    #[test]
    fn blocking_probe_answers_outside_a_runtime() -> TestResult {
        assert!(!probe_blocking("127.0.0.1:1", "token")?);
        assert!(!probe_blocking("not an address", "token")?);
        Ok(())
    }
}
