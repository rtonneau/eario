//! Installs and updates dashboards from GitHub releases (ADR 0013): the
//! blocking work behind the [`Supervisor`](crate::Supervisor)'s catalogs,
//! previews, installs, update checks, updates and rollbacks. Call it through
//! `tokio::task::spawn_blocking`.
//!
//! # A preview, then an install
//!
//! A *preview* reads the latest release of a repository and picks the asset
//! for this machine: `<slug>-<target triple>[.exe]`, the only one with that
//! ending for a first install, the one of the installed slug for an update.
//! It verifies the minisign signature of the release's `SHA256SUMS.txt`
//! (a release without `SHA256SUMS.txt.minisig` is refused), downloads the
//! asset into the staging area of this process under `staging/`, and
//! compares its SHA-256 with the signed list.
//!
//! Only then, and only if the key that signed the release is trusted, is
//! the file run, with `--eario-manifest` alone, no standard input, a bounded
//! output and a timeout: its slug must be the asset name's prefix and its
//! launch protocol this hub's. A release signed with a key that is `unknown`
//! or `changed` stops before that, as a preview without a manifest, and
//! waits for the user to accept the key; the file is then hashed again and
//! run. A signature proves only that the holder of
//! the key published the file, so nothing published under a key the user
//! has not trusted is ever run.
//!
//! The verified file waits in `staging/` until the user confirms the full
//! preview by its SHA-256. It is then moved into `bin/<slug>/` with one
//! rename, under a name of its own, checked again, and recorded in the
//! registry, which is written atomically under the hub directory's lock.
//! Nothing enters `bin/` before every check has passed.
//!
//! # Staging
//!
//! Each supervisor, so each process (the hub, or `eario-hub` given a
//! command), has a staging area of its own: `staging/<process id>-<n>/`,
//! holding a file `lock` that it keeps locked while it lives, and one
//! directory per preview. A hub that starts removes only the areas whose lock
//! it can take, left by a process that ended; an `eario-hub install` waiting
//! for its confirmation keeps its preview.
//!
//! # Keys
//!
//! The key that must have signed a release is the one the release carries
//! in `minisign.pub`, or, without that file, the key the hub already trusts
//! for the repository. How it relates to what the hub trusts is its
//! [`KeyStatus`]: the key pinned in the registry decides for an installed
//! repository; otherwise the key of the first catalog, in the user's order,
//! that lists the repository; with neither, the key is `unknown` and is
//! trusted on first use only once the user accepts it. A key that differs
//! from the pinned or catalog key is `changed` and refused until the user
//! accepts it. The user accepts a key by the SHA-256 of its key line
//! ([`PublicKey::sha256`]), never by its minisign key id, which anyone can
//! copy. An install pins the key the release was verified with.
//!
//! # Versions
//!
//! Versions are compared by SemVer 2.0 precedence, ignoring build metadata
//! and one leading `v`. An update installs a release only when its
//! executable's manifest version is strictly newer than the installed
//! manifest version; the release tag, which nobody signs, only lets the hub
//! refuse an older release before downloading it and report available
//! updates without downloading anything. When either version is not SemVer,
//! or missing, the hub cannot tell an update from a downgrade, so the update
//! is refused: uninstall the dashboard, then install it again (the preview
//! shows the version). An older version is installed only by a rollback to
//! the one previous version the hub keeps.

use std::cmp::Ordering;
use std::ffi::OsStr;
use std::fs::{self, TryLockError};
use std::io::{self, BufWriter, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use eario_core::{
    ApiError, CATALOG_FORMAT_VERSION, Catalog, DashboardUpdate, HubCatalog, HubCatalogEntry,
    HubSettings, InstallPreview, KeyStatus, LAUNCH_PROTOCOL_VERSION, MANIFEST_ARGUMENT, Manifest,
    Source, UpdateCheck, is_repo, is_slug,
};
use eario_runtime::HubDirError;
use eario_runtime::hub_dir::{self, HubDir, PreviousVersion, Registry, RegistryEntry};
use serde::{Deserialize, Serialize};

use crate::github::{Download, GitHub, GitHubError, Release, ReleaseAsset};
use crate::supervisor::{HubError, detach};
use crate::verify::{
    self, KEY_FILE, MAX_KEY_FILE_BYTES, PublicKey, SUMS_FILE, Target, VerifyError, asset_name,
    verify_sums,
};

/// The catalog the hub reads until the user removes it.
pub const DEFAULT_CATALOG: &str = "rtonneau/eario-catalog";

/// Default longest time a downloaded executable has to print its manifest
/// (or, for the hub's own update, its version) and exit. The first run of a
/// new executable can be slow while an antivirus scans it.
pub const MANIFEST_TIMEOUT: Duration = Duration::from_secs(20);

/// Format version of `catalogs.json`.
const CATALOGS_FORMAT_VERSION: u32 = 1;

/// Most of the output of a downloaded executable read, in bytes; a manifest
/// takes a few hundred.
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;

/// Most previews kept at once. A new one drops the oldest, file included.
pub(crate) const MAX_STAGED: usize = 4;

/// Attempts at starting a downloaded executable that the system reports
/// busy.
const SPAWN_ATTEMPTS: u32 = 5;

/// `catalogs.json`: the repositories of the catalogs the hub reads.
#[derive(Debug, Serialize, Deserialize)]
struct CatalogsFile {
    format_version: u32,
    catalogs: Vec<String>,
}

/// The repositories of the catalogs the hub reads, in the user's order:
/// [`DEFAULT_CATALOG`] until the user changes the list.
pub(crate) fn read_catalogs(hub: &HubDir) -> Result<Vec<String>, HubError> {
    let path = hub.catalogs_path();
    let Some(file) = hub_dir::read_json_at::<CatalogsFile>(&path)? else {
        return Ok(vec![DEFAULT_CATALOG.to_owned()]);
    };
    let invalid = |message: String| HubDirError::Invalid { path: path.clone(), message };
    if file.format_version > CATALOGS_FORMAT_VERSION {
        return Err(invalid(format!(
            "format version {} is newer than {CATALOGS_FORMAT_VERSION}, the newest this hub reads; update the hub",
            file.format_version
        ))
        .into());
    }
    if let Some(bad) = file.catalogs.iter().find(|repo| !is_repo(repo)) {
        return Err(invalid(format!("`{bad}` is not a GitHub repository; remove it")).into());
    }
    Ok(file.catalogs)
}

/// Writes the list of catalogs atomically. A caller that read the list to
/// change it holds the hub directory's lock.
pub(crate) fn write_catalogs(hub: &HubDir, catalogs: &[String]) -> Result<(), HubError> {
    let file =
        CatalogsFile { format_version: CATALOGS_FORMAT_VERSION, catalogs: catalogs.to_vec() };
    Ok(hub_dir::write_json_at(&hub.catalogs_path(), &file)?)
}

/// The hub's settings; the defaults when the user never changed them.
pub(crate) fn read_settings(hub: &HubDir) -> Result<HubSettings, HubError> {
    Ok(hub_dir::read_json_at(&hub.settings_path())?.unwrap_or_default())
}

/// Writes the hub's settings atomically. A caller that read them to change
/// them holds the hub directory's lock.
pub(crate) fn write_settings(hub: &HubDir, settings: &HubSettings) -> Result<(), HubError> {
    Ok(hub_dir::write_json_at(&hub.settings_path(), settings)?)
}

/// What the last update check found; `None` before the first one.
pub(crate) fn read_last_check(hub: &HubDir) -> Result<Option<UpdateCheck>, HubError> {
    Ok(hub_dir::read_json_at(&hub.updates_path())?)
}

/// Stores what an update check found, atomically.
pub(crate) fn write_last_check(hub: &HubDir, check: &UpdateCheck) -> Result<(), HubError> {
    Ok(hub_dir::write_json_at(&hub.updates_path(), check)?)
}

/// Whether two repositories are the same one: GitHub ignores the case of
/// their names.
pub(crate) fn same_repo(first: &str, second: &str) -> bool {
    first.eq_ignore_ascii_case(second)
}

/// The entry installed from `repo`, if any.
pub(crate) fn installed_from<'r>(registry: &'r Registry, repo: &str) -> Option<&'r RegistryEntry> {
    registry.dashboards.iter().find(|entry| match &entry.source {
        Source::Github { repo: installed } => same_repo(installed, repo),
        Source::Local => false,
    })
}

/// The error body of `error`, as the HTTP API would answer it.
pub(crate) fn api_error(error: &HubError) -> ApiError {
    ApiError { code: error.code().to_owned(), message: error.to_string() }
}

/// A SemVer 2.0 version, for precedence only: build metadata is checked,
/// then left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Version {
    core: [u64; 3],
    pre_release: Vec<Identifier>,
}

/// One dot-separated identifier of a pre-release. A numeric one has lower
/// precedence than one with letters, as the derived order says.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Identifier {
    Numeric(u64),
    Text(String),
}

impl Version {
    /// Reads `MAJOR.MINOR.PATCH[-PRE-RELEASE][+BUILD]` as SemVer 2.0 writes
    /// it, after one optional leading `v`; `None` for anything else.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let text = text.strip_prefix('v').unwrap_or(text);
        let text = match text.split_once('+') {
            Some((version, build)) => {
                let valid = build.split('.').all(|part| {
                    !part.is_empty()
                        && part.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                });
                if !valid {
                    return None;
                }
                version
            }
            None => text,
        };
        let (core, pre_release) = match text.split_once('-') {
            Some((core, pre_release)) => (core, Some(pre_release)),
            None => (text, None),
        };
        let mut numbers = core.split('.').map(numeric);
        let core = [numbers.next()??, numbers.next()??, numbers.next()??];
        if numbers.next().is_some() {
            return None;
        }
        let pre_release = match pre_release {
            Some(text) => text.split('.').map(identifier).collect::<Option<Vec<_>>>()?,
            None => Vec::new(),
        };
        Some(Self { core, pre_release })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.core.cmp(&other.core).then_with(|| {
            match (self.pre_release.is_empty(), other.pre_release.is_empty()) {
                (true, true) => Ordering::Equal,
                // A pre-release comes before its release.
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                // Identifier by identifier; a shorter list that is a prefix
                // of the other comes first.
                (false, false) => self.pre_release.cmp(&other.pre_release),
            }
        })
    }
}

/// A number of a version: digits, without a leading zero.
fn numeric(text: &str) -> Option<u64> {
    let digits = !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    if !digits || (text.len() > 1 && text.starts_with('0')) {
        return None;
    }
    text.parse().ok()
}

fn identifier(text: &str) -> Option<Identifier> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-') {
        return None;
    }
    if text.bytes().all(|byte| byte.is_ascii_digit()) {
        numeric(text).map(Identifier::Numeric)
    } else {
        Some(Identifier::Text(text.to_owned()))
    }
}

/// How version `candidate` compares with `installed`; `None` when either is
/// missing or not SemVer, so the hub cannot tell.
pub(crate) fn compare_versions(
    candidate: Option<&str>,
    installed: Option<&str>,
) -> Option<Ordering> {
    let candidate = Version::parse(candidate?)?;
    let installed = Version::parse(installed?)?;
    Some(candidate.cmp(&installed))
}

/// The version a release tag names: the tag without one leading `v`, when
/// that is SemVer.
pub(crate) fn tag_version(tag: &str) -> Option<String> {
    let version = tag.strip_prefix('v').unwrap_or(tag);
    Version::parse(version).map(|_| version.to_owned())
}

/// A version as shown in a message.
pub(crate) fn version_text(version: Option<&str>) -> String {
    version.unwrap_or("unknown").to_owned()
}

/// What the hub trusts for a repository when it previews a release.
#[derive(Debug, Clone, Default)]
pub(crate) struct Trust {
    /// The key pinned in the registry: the repository is installed.
    pub(crate) pinned: Option<PublicKey>,
    /// The key of the first catalog that lists the repository.
    pub(crate) catalog: Option<PublicKey>,
    /// A key the user accepted for the repository in this hub.
    pub(crate) accepted: Option<PublicKey>,
}

impl Trust {
    /// How `key` relates to what is trusted; see the module documentation.
    pub(crate) fn status(&self, key: &PublicKey) -> KeyStatus {
        match verify::key_status(key, self.pinned.as_ref(), self.catalog.as_ref()) {
            KeyStatus::Unknown | KeyStatus::Changed if self.accepted.as_ref() == Some(key) => {
                KeyStatus::Accepted
            }
            status => status,
        }
    }

    /// The key to verify a release with when it carries none.
    fn known_key(&self) -> Option<&PublicKey> {
        self.pinned.as_ref().or(self.catalog.as_ref()).or(self.accepted.as_ref())
    }
}

/// Whether a release signed by a key of this status may be installed.
pub(crate) fn is_trusted(status: KeyStatus) -> bool {
    matches!(status, KeyStatus::Catalog | KeyStatus::Pinned | KeyStatus::Accepted)
}

/// Name of the locked file in a staging area.
const STAGING_LOCK: &str = "lock";

/// Distinguishes the staging areas, and the previews in them, of one
/// process.
static STAGING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// The staging area of one supervisor: `staging/<process id>-<n>/`, holding
/// the file `lock`, locked for as long as the area exists, and one
/// directory per preview. Removed with what it holds when dropped; left
/// behind only by a process that ends without dropping it, for the next hub
/// start to remove (see [`clean_staging`]).
#[derive(Debug)]
pub(crate) struct StagingArea {
    path: PathBuf,
    /// The locked file, closed (so unlocked) before the area is removed.
    lock: Option<fs::File>,
}

impl StagingArea {
    /// A new staging area in the hub's staging directory, created and
    /// locked while the hub directory's lock is held, so a hub cleaning the
    /// staging directory meanwhile never takes it for a stale one.
    pub(crate) fn create(hub: &HubDir) -> Result<Self, HubError> {
        let sequence = STAGING_SEQUENCE.fetch_add(1, AtomicOrdering::Relaxed);
        let path = hub.staging_dir().join(format!("{}-{sequence}", std::process::id()));
        let hub_lock = hub.lock()?;
        // Only an earlier process with the same id, which has ended, can
        // have left a directory of this name.
        remove_dir(&path);
        hub_dir::create_private_dir(&path)?;
        let lock_path = path.join(STAGING_LOCK);
        let io_error = |source| HubError::from(HubDirError::Io { path: lock_path.clone(), source });
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&lock_path)
            .map_err(io_error)?;
        lock.try_lock().map_err(|error| io_error(error.into()))?;
        drop(hub_lock);
        tracing::debug!(directory = %path.display(), "staging area created");
        Ok(Self { path, lock: Some(lock) })
    }
}

impl Drop for StagingArea {
    fn drop(&mut self) {
        // Closed first, so nothing in the directory is open when it is
        // removed. A hub that starts meanwhile may remove it too.
        drop(self.lock.take());
        remove_dir(&self.path);
    }
}

/// The directory of one preview in a staging area, removed with what it
/// holds when dropped.
#[derive(Debug)]
pub(crate) struct StagingDir {
    path: PathBuf,
    /// Its area, kept while the directory exists.
    _area: Arc<StagingArea>,
}

impl StagingDir {
    /// A new, empty directory in `area`, named after a counter.
    fn create(area: &Arc<StagingArea>) -> Result<Self, HubError> {
        let sequence = STAGING_SEQUENCE.fetch_add(1, AtomicOrdering::Relaxed);
        let path = area.path.join(sequence.to_string());
        hub_dir::create_private_dir(&path)?;
        Ok(Self { path, _area: Arc::clone(area) })
    }

    /// Where the directory is.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for StagingDir {
    fn drop(&mut self) {
        remove_dir(&self.path);
    }
}

/// Removes a directory and what it holds. A failure is logged: the next hub
/// start removes what is left of a staging area nobody holds.
fn remove_dir(path: &Path) {
    match fs::remove_dir_all(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            tracing::warn!(directory = %path.display(), %error, "could not remove a staging directory");
        }
    }
}

/// Removes from the hub's staging directory what processes that ended left
/// there: the staging areas whose lock nobody holds, and anything that is no
/// area. The area of a process that runs, such as an `eario-hub install`
/// waiting for its confirmation, is kept. What a hub does when it starts.
///
/// Runs under the hub directory's lock, under which areas are created, so an
/// area without its lock file is a stale one.
pub(crate) fn clean_staging(hub: &HubDir) -> Result<(), HubError> {
    let staging = hub.staging_dir();
    let _hub_lock = hub.lock()?;
    let entries = match fs::read_dir(&staging) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(source) => return Err(HubDirError::Io { path: staging, source }.into()),
    };
    for entry in entries {
        let entry = entry.map_err(|source| HubDirError::Io { path: staging.clone(), source })?;
        let path = entry.path();
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            match fs::remove_file(&path) {
                Ok(()) => tracing::info!(file = %path.display(), "removed a stray staging file"),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    tracing::warn!(file = %path.display(), %error, "could not remove a stray staging file");
                }
            }
        } else if is_in_use(&path) {
            tracing::debug!(directory = %path.display(), "kept the staging area of a running process");
        } else {
            remove_dir(&path);
            tracing::info!(directory = %path.display(), "removed a stale staging area");
        }
    }
    Ok(())
}

/// Whether a supervisor, in this process or another, holds the lock of the
/// staging area `directory`. An area that cannot be checked is taken as in
/// use: removing it could break a preview in progress.
fn is_in_use(directory: &Path) -> bool {
    let path = directory.join(STAGING_LOCK);
    let file = match fs::OpenOptions::new().read(true).write(true).open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return false,
        Err(error) => {
            tracing::warn!(file = %path.display(), %error, "could not open a staging lock; its area is kept");
            return true;
        }
    };
    match file.try_lock() {
        // Released when `file` is closed, on return.
        Ok(()) => false,
        Err(TryLockError::WouldBlock) => true,
        Err(TryLockError::Error(error)) => {
            tracing::warn!(file = %path.display(), %error, "could not check a staging lock; its area is kept");
            true
        }
    }
}

/// A release downloaded into the staging directory and verified, waiting to
/// be installed. Dropping it removes its file.
#[derive(Debug)]
pub(crate) struct Staged {
    /// What the user is shown. Its manifest is `None` until the key is
    /// trusted and the verified file has been run.
    pub(crate) preview: InstallPreview,
    /// The key the release was verified with.
    pub(crate) key: PublicKey,
    /// For an update, the registry entry it replaces, as it was when the
    /// preview was made.
    pub(crate) replaces: Option<RegistryEntry>,
    /// The verified file.
    file: PathBuf,
    /// Its directory, removed with the file when the release is dropped.
    directory: StagingDir,
}

impl Staged {
    /// The manifest of a release ready to be installed: its key is trusted
    /// and the verified file printed it. Refused otherwise.
    pub(crate) fn manifest(&self) -> Result<&Manifest, HubError> {
        match &self.preview.manifest {
            Some(manifest) if is_trusted(self.preview.key_status) => Ok(manifest),
            _ => Err(untrusted_key(&self.preview)),
        }
    }
}

/// The refusal of a release whose key is not trusted yet.
pub(crate) fn untrusted_key(preview: &InstallPreview) -> HubError {
    let (repo, key_sha256) = (preview.repo.clone(), preview.key_sha256.clone());
    match preview.key_status {
        KeyStatus::Changed => HubError::KeyChanged { repo, key_sha256 },
        _ => HubError::KeyNotAccepted { repo, key_sha256 },
    }
}

/// What a preview is for.
#[derive(Debug)]
pub(crate) enum Purpose {
    /// A first install. The slug comes from the asset name; it, and the
    /// repository, must not be installed.
    Install {
        /// The registry as read before the preview.
        registry: Registry,
        /// The key of the first catalog that lists the repository.
        catalog_key: Option<PublicKey>,
    },
    /// An update of this entry, installed from the repository.
    Update {
        /// The entry as read before the preview.
        entry: Box<RegistryEntry>,
    },
}

/// Everything a preview needs.
#[derive(Debug)]
pub(crate) struct StageRequest {
    /// The repository, which passed [`is_repo`].
    pub(crate) repo: String,
    /// What the preview is for.
    pub(crate) purpose: Purpose,
    /// A key the user accepted for the repository in this hub.
    pub(crate) accepted: Option<PublicKey>,
    /// This machine's target; `None` refuses the preview.
    pub(crate) target: Option<Target>,
    /// See [`MANIFEST_TIMEOUT`].
    pub(crate) manifest_timeout: Duration,
}

/// Previews the latest release of a repository: downloads its asset for this
/// machine into `area` and verifies it, as the module documentation
/// describes. Nothing is written outside `staging/`.
///
/// When the key is trusted, the verified file is run for its manifest and
/// the preview is complete. Otherwise the preview stops after the signature
/// and the SHA-256, without a manifest: the file is not run before the user
/// accepts its key ([`accept_staged`]).
pub(crate) fn stage(
    area: &Arc<StagingArea>,
    github: &dyn GitHub,
    request: StageRequest,
) -> Result<Staged, HubError> {
    let StageRequest { repo, purpose, accepted, target, manifest_timeout } = request;
    let repo = repo.as_str();
    let target = target.ok_or(HubError::UnsupportedPlatform)?;
    let release = github.latest_release(repo)?;

    let (asset, slug, trust) = match &purpose {
        Purpose::Install { registry, catalog_key } => {
            let (asset, slug) = find_asset(repo, &release, target)?;
            refuse_installed(registry, repo, &slug)?;
            (asset, slug, Trust { pinned: None, catalog: catalog_key.clone(), accepted })
        }
        Purpose::Update { entry } => {
            let name = asset_name(&entry.slug, target)?;
            let asset = release
                .asset(&name)
                .ok_or_else(|| HubError::NoAsset { repo: repo.to_owned(), asset: name.clone() })?;
            // The tag is not signed, but an older one is refused before
            // anything is downloaded.
            let installed = entry.manifest.version.as_deref();
            let tagged = tag_version(&release.tag);
            if matches!(
                compare_versions(tagged.as_deref(), installed),
                Some(Ordering::Less | Ordering::Equal)
            ) {
                return Err(HubError::NotNewer {
                    slug: entry.slug.clone(),
                    installed: version_text(installed),
                    found: version_text(tagged.as_deref()),
                });
            }
            let pinned = entry
                .pinned_key
                .as_deref()
                .map(|text| PublicKey::from_base64(repo, text))
                .transpose()?;
            (asset, entry.slug.clone(), Trust { pinned, catalog: None, accepted })
        }
    };

    let sums = signed_sums(github, repo, &release)?;
    let key_file = github.release_file(repo, &release, KEY_FILE, MAX_KEY_FILE_BYTES as u64)?;
    let key = match key_file {
        Some(bytes) => PublicKey::from_pub_file(repo, &bytes)?,
        None => trust
            .known_key()
            .cloned()
            .ok_or_else(|| VerifyError::MissingKey { repo: repo.to_owned() })?,
    };
    let key_status = trust.status(&key);
    let verified = download_verified(area, github, repo, asset, &key, &sums)?;
    let replaces = match purpose {
        Purpose::Update { entry } => Some(*entry),
        Purpose::Install { .. } => None,
    };
    let mut staged = Staged {
        preview: InstallPreview {
            repo: repo.to_owned(),
            slug,
            manifest: None,
            tag: release.tag.clone(),
            asset: asset.name.clone(),
            size: verified.size,
            sha256: verified.sha256,
            public_key: key.as_base64().to_owned(),
            key_sha256: key.sha256(),
            key_status,
        },
        key,
        replaces,
        file: verified.file,
        directory: verified.directory,
    };
    let preview = &staged.preview;
    if !is_trusted(key_status) {
        tracing::info!(
            repo,
            asset = %preview.asset,
            sha256 = %preview.sha256,
            key_sha256 = %preview.key_sha256,
            ?key_status,
            "release verified and staged; it is not run until its key is accepted"
        );
        return Ok(staged);
    }
    run_manifest(&mut staged, manifest_timeout)?;
    let preview = &staged.preview;
    tracing::info!(
        repo,
        asset = %preview.asset,
        sha256 = %preview.sha256,
        key_sha256 = %preview.key_sha256,
        ?key_status,
        "release verified and staged"
    );
    Ok(staged)
}

/// The checksum list of a release and its minisign signature, as downloaded:
/// not verified yet.
#[derive(Debug)]
pub(crate) struct SignedSums {
    sums: Vec<u8>,
    signature: Vec<u8>,
}

/// Downloads the `SHA256SUMS.txt` of `release` and its signature. Refused
/// when the release has no signature (`release_missing_signature`), before
/// any asset is downloaded: without one nothing in the release can be
/// trusted.
pub(crate) fn signed_sums(
    github: &dyn GitHub,
    repo: &str,
    release: &Release,
) -> Result<SignedSums, HubError> {
    let files = github.checksum_files(repo, release)?;
    let signature =
        files.signature.ok_or_else(|| VerifyError::MissingSignature { repo: repo.to_owned() })?;
    let sums = files.sums.ok_or_else(|| GitHubError::NotFound {
        repo: repo.to_owned(),
        file: SUMS_FILE.to_owned(),
    })?;
    Ok(SignedSums { sums, signature })
}

/// A release asset downloaded into a staging directory, whose SHA-256 is the
/// one its signed checksum list gives.
#[derive(Debug)]
pub(crate) struct Verified {
    /// The downloaded file.
    pub(crate) file: PathBuf,
    /// Its size in bytes.
    pub(crate) size: u64,
    /// Its SHA-256, in lower-case hexadecimal.
    pub(crate) sha256: String,
    /// Its directory, removed with the file when dropped.
    pub(crate) directory: StagingDir,
}

/// What every release goes through, a dashboard's or the hub's own: checks
/// the signature of `sums` with `key`, then downloads `asset` into a new
/// directory of `area`, bounded by the size the release lists, and compares
/// its SHA-256 with the one the signed list gives. Nothing is downloaded
/// unless the signature verifies and lists the asset.
pub(crate) fn download_verified(
    area: &Arc<StagingArea>,
    github: &dyn GitHub,
    repo: &str,
    asset: &ReleaseAsset,
    key: &PublicKey,
    sums: &SignedSums,
) -> Result<Verified, HubError> {
    let checksums = verify_sums(repo, key, &sums.sums, Some(&sums.signature))?;
    let sha256 = checksums.sha256(&asset.name)?.to_owned();
    let directory = StagingDir::create(area)?;
    let file = directory.path.join(&asset.name);
    let download = download_to(github, repo, asset, &file)?;
    if download.sha256 != sha256 {
        return Err(
            VerifyError::HashMismatch { repo: repo.to_owned(), asset: asset.name.clone() }.into()
        );
    }
    Ok(Verified { file, size: download.size, sha256, directory })
}

/// Completes the preview of a staged release whose key the user has just
/// accepted: hashes its file again, as it waited in the staging directory,
/// then runs it for its manifest, as [`stage`] does for a trusted key.
/// Refused (`hub_preview_gone`) when the file is no longer there.
pub(crate) fn accept_staged(mut staged: Staged, timeout: Duration) -> Result<Staged, HubError> {
    let preview = &staged.preview;
    match hub_dir::file_digest(&staged.file) {
        Ok((size, sha256)) if size == preview.size && sha256 == preview.sha256 => {}
        Ok(_) => {
            return Err(VerifyError::HashMismatch {
                repo: preview.repo.clone(),
                asset: preview.asset.clone(),
            }
            .into());
        }
        Err(HubDirError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
            return Err(HubError::PreviewGone {
                repo: preview.repo.clone(),
                path: staged.file.clone(),
            });
        }
        Err(error) => return Err(error.into()),
    }
    staged.preview.key_status = KeyStatus::Accepted;
    run_manifest(&mut staged, timeout)?;
    Ok(staged)
}

/// Runs the staged file, whose key must be trusted, with `--eario-manifest`
/// and checks what it prints: the slug of the preview, this hub's launch
/// protocol and, for an update, a version newer than the installed one. The
/// manifest then completes the preview.
fn run_manifest(staged: &mut Staged, timeout: Duration) -> Result<(), HubError> {
    let preview = &staged.preview;
    // Never run what a key the user has not trusted signed.
    if !is_trusted(preview.key_status) {
        return Err(untrusted_key(preview));
    }
    let (repo, asset, slug) = (&preview.repo, &preview.asset, &preview.slug);
    let manifest = read_manifest(repo, asset, &staged.file, &staged.directory.path, timeout)?;
    if manifest.slug != *slug {
        return Err(HubError::ManifestMismatch {
            repo: repo.clone(),
            asset: asset.clone(),
            expected: slug.clone(),
            found: manifest.slug,
        });
    }
    if manifest.protocol != LAUNCH_PROTOCOL_VERSION {
        return Err(HubError::UnsupportedProtocol {
            slug: slug.clone(),
            found: manifest.protocol,
            supported: LAUNCH_PROTOCOL_VERSION,
        });
    }
    if let Some(entry) = &staged.replaces {
        let installed = entry.manifest.version.as_deref();
        let found = manifest.version.as_deref();
        match compare_versions(found, installed) {
            Some(Ordering::Greater) => {}
            Some(_) => {
                return Err(HubError::NotNewer {
                    slug: slug.clone(),
                    installed: version_text(installed),
                    found: version_text(found),
                });
            }
            None => {
                return Err(HubError::VersionNotComparable {
                    slug: slug.clone(),
                    installed: version_text(installed),
                    found: version_text(found),
                });
            }
        }
    }
    staged.preview.manifest = Some(manifest);
    Ok(())
}

/// The asset of a first install: the only one whose name is a slug followed
/// by `-<triple>[.exe]` of `target`. Returns it with that slug.
fn find_asset<'r>(
    repo: &str,
    release: &'r Release,
    target: Target,
) -> Result<(&'r ReleaseAsset, String), HubError> {
    let ending = format!("-{}{}", target.triple(), target.exe_suffix());
    let mut matching = release.assets.iter().filter_map(|asset| {
        let slug = asset.name.strip_suffix(&ending).filter(|slug| is_slug(slug))?;
        Some((asset, slug))
    });
    match (matching.next(), matching.next()) {
        (Some((asset, slug)), None) => Ok((asset, slug.to_owned())),
        (None, _) => {
            Err(HubError::NoAsset { repo: repo.to_owned(), asset: format!("<slug>{ending}") })
        }
        (Some(first), Some(second)) => {
            let names: Vec<&str> = [first, second]
                .into_iter()
                .chain(matching)
                .map(|(asset, _)| asset.name.as_str())
                .collect();
            Err(HubError::AmbiguousAsset {
                repo: repo.to_owned(),
                ending,
                assets: names.join(", "),
            })
        }
    }
}

/// Refuses a first install of `repo` as `slug` when the repository, or
/// another dashboard of that slug, is installed already.
fn refuse_installed(registry: &Registry, repo: &str, slug: &str) -> Result<(), HubError> {
    if let Some(entry) = installed_from(registry, repo) {
        return Err(HubError::AlreadyInstalled { repo: repo.to_owned(), slug: entry.slug.clone() });
    }
    if let Some(entry) = registry.get(slug) {
        let installed_from = match &entry.source {
            Source::Local => "an executable on this computer (--eario-install)".to_owned(),
            Source::Github { repo } => format!("`{repo}`"),
        };
        return Err(HubError::SlugTaken {
            slug: slug.to_owned(),
            repo: repo.to_owned(),
            installed_from,
        });
    }
    Ok(())
}

/// Downloads `asset` into a new file at `path`, executable by the user only
/// on Unix, and flushes it to disk.
fn download_to(
    github: &dyn GitHub,
    repo: &str,
    asset: &ReleaseAsset,
    path: &Path,
) -> Result<Download, HubError> {
    let io_error = |source| HubError::HubDir(HubDirError::Io { path: path.to_owned(), source });
    let file =
        executable_file_options().write(true).create_new(true).open(path).map_err(io_error)?;
    let mut writer = BufWriter::with_capacity(64 * 1024, file);
    let download = github.download(repo, asset, &mut writer)?;
    let file = writer.into_inner().map_err(|error| io_error(error.into_error()))?;
    // Closed before it is run: on Unix an executable open for writing
    // cannot be started.
    file.sync_all().map_err(io_error)?;
    Ok(download)
}

/// Options that create a file the user alone can read, write and run on
/// Unix. The caller chooses how to open it.
pub(crate) fn executable_file_options() -> fs::OpenOptions {
    let options = fs::OpenOptions::new();
    #[cfg(unix)]
    let options = {
        use std::os::unix::fs::OpenOptionsExt;
        let mut options = options;
        options.mode(0o700);
        options
    };
    options
}

/// Runs the verified executable at `file` with `--eario-manifest` and reads
/// the manifest it prints, as [`probe`] runs it.
fn read_manifest(
    repo: &str,
    asset: &str,
    file: &Path,
    directory: &Path,
    timeout: Duration,
) -> Result<Manifest, HubError> {
    let failed = |detail: String| HubError::ManifestFailed {
        repo: repo.to_owned(),
        asset: asset.to_owned(),
        detail,
    };
    let text = probe(file, MANIFEST_ARGUMENT, directory, timeout).map_err(failed)?;
    serde_json::from_str(text.trim())
        .map_err(|error| failed(format!("it printed no manifest ({error})")))
}

/// Runs the verified executable at `file` with `argument` alone and returns
/// what it printed, once it exited with success: no standard input, standard
/// error discarded, at most [`MAX_MANIFEST_BYTES`] read, no console window
/// (Windows) and a process group of its own, `directory` as working
/// directory, killed after `timeout`. On failure, says what happened, for a
/// message.
pub(crate) fn probe(
    file: &Path,
    argument: &str,
    directory: &Path,
    timeout: Duration,
) -> Result<String, String> {
    let mut command = Command::new(file);
    command
        .arg(argument)
        .current_dir(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    detach(&mut command);
    let started = Instant::now();
    let mut child = spawn_retrying(&mut command, SPAWN_ATTEMPTS)
        .map_err(|error| format!("it could not be started: {error}"))?;
    let Some(stdout) = child.stdout.take() else {
        stop(child);
        return Err("its output could not be read".to_owned());
    };
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let read = stdout.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut bytes).map(|_| bytes);
        // The receiver is gone when it gave up waiting; nobody to tell.
        let _ = sender.send(read);
    });
    let seconds = timeout.as_secs();
    let bytes = match receiver.recv_timeout(timeout) {
        Ok(Ok(bytes)) if bytes.len() as u64 <= MAX_MANIFEST_BYTES => bytes,
        Ok(Ok(_)) => {
            stop(child);
            return Err(format!("it printed more than {MAX_MANIFEST_BYTES} bytes"));
        }
        Ok(Err(error)) => {
            stop(child);
            return Err(format!("its output could not be read: {error}"));
        }
        Err(RecvTimeoutError::Timeout) => {
            stop(child);
            return Err(format!("it did not print its answer within {seconds} s"));
        }
        Err(RecvTimeoutError::Disconnected) => {
            stop(child);
            return Err("its output could not be read".to_owned());
        }
    };
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= timeout => {
                stop(child);
                return Err(format!("it did not exit within {seconds} s"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                stop(child);
                return Err(format!("its exit could not be awaited: {error}"));
            }
        }
    };
    if !status.success() {
        return Err(format!("it exited with {status}"));
    }
    String::from_utf8(bytes).map_err(|_| "its output is not UTF-8".to_owned())
}

/// Starts `command`, trying again 50 ms later, up to `attempts` times in
/// all, while the system reports its executable busy ([`is_busy`]): on Unix
/// a process forked by another thread while the file was still open for
/// writing holds it until that process executes; on Windows an antivirus may
/// hold a file just written. Blocking.
pub(crate) fn spawn_retrying(command: &mut Command, attempts: u32) -> io::Result<Child> {
    let mut attempt = 1;
    loop {
        match command.spawn() {
            Err(error) if is_busy(&error) && attempt < attempts => {
                attempt += 1;
                std::thread::sleep(Duration::from_millis(50));
            }
            result => return result,
        }
    }
}

/// `ETXTBSY`, "text file busy".
#[cfg(unix)]
pub(crate) fn is_busy(error: &io::Error) -> bool {
    error.raw_os_error() == Some(26)
}

/// `ERROR_SHARING_VIOLATION`: another program, such as an antivirus, holds
/// the file.
#[cfg(windows)]
pub(crate) fn is_busy(error: &io::Error) -> bool {
    error.raw_os_error() == Some(32)
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn is_busy(_error: &io::Error) -> bool {
    false
}

/// Kills a process and waits for it to be gone.
fn stop(mut child: Child) {
    // Fails when it exited already, which is the goal.
    if let Err(error) = child.kill() {
        tracing::debug!(%error, "kill failed, the process had exited");
    }
    if let Err(error) = child.wait() {
        tracing::warn!(%error, "could not wait for a stopped process");
    }
}

/// File name of the hub's copy of `slug` with SHA-256 `sha256`: one per
/// content, so a new version never overwrites the one it replaces.
fn copy_name(slug: &str, sha256: &str) -> String {
    let short = sha256.get(..12).unwrap_or(sha256);
    format!("{slug}-{short}{}", std::env::consts::EXE_SUFFIX)
}

/// Moves the staged file into `bin/<slug>/` with one rename and checks that
/// it arrived whole. Returns its path and file name. Refused
/// (`hub_preview_gone`) when the staged file is no longer there.
fn place(hub: &HubDir, staged: &Staged) -> Result<(PathBuf, String), HubError> {
    let preview = &staged.preview;
    let directory = hub.bin_dir(&preview.slug)?;
    hub_dir::create_private_dir(&directory)?;
    let name = copy_name(&preview.slug, &preview.sha256);
    let destination = directory.join(&name);
    if let Err(error) = hub_dir::move_file(&staged.file, &destination) {
        if matches!(staged.file.try_exists(), Ok(false)) {
            return Err(HubError::PreviewGone {
                repo: preview.repo.clone(),
                path: staged.file.clone(),
            });
        }
        return Err(error.into());
    }
    match hub_dir::file_digest(&destination) {
        Ok((size, sha256)) if size == preview.size && sha256 == preview.sha256 => {
            Ok((destination, name))
        }
        other => {
            remove_file(&destination);
            match other {
                Err(error) => Err(error.into()),
                Ok(_) => Err(VerifyError::HashMismatch {
                    repo: preview.repo.clone(),
                    asset: preview.asset.clone(),
                }
                .into()),
            }
        }
    }
}

/// Removes a file of `bin/`, logging a failure.
fn remove_file(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => tracing::info!(file = %path.display(), "removed"),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => tracing::warn!(file = %path.display(), %error, "could not remove a file"),
    }
}

/// Removes the files of `directory` (a `bin/<slug>/`) not named in `keep`:
/// the older previous version after an update, the newer copy after a
/// rollback, a copy left by an interrupted update. A failure is logged and
/// tried again next time.
fn sweep(directory: &Path, keep: &[&str]) {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            tracing::warn!(directory = %directory.display(), %error, "could not list old copies");
            return;
        }
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let kept = keep.iter().any(|kept| name == OsStr::new(kept));
        if !kept && entry.file_type().is_ok_and(|kind| kind.is_file()) {
            remove_file(&entry.path());
        }
    }
}

/// File name of the hub's own copy an entry records, which must be in
/// `directory` and be a plain file name.
fn own_file_name(directory: &Path, entry: &RegistryEntry) -> Result<String, HubError> {
    let name = entry.path.file_name().and_then(OsStr::to_str);
    match name {
        Some(name) if entry.path.parent() == Some(directory) && verify::is_asset_name(name) => {
            Ok(name.to_owned())
        }
        _ => Err(HubError::ForeignExecutable {
            slug: entry.slug.clone(),
            path: entry.path.clone(),
            expected: directory.to_owned(),
        }),
    }
}

/// Installs a staged first install: moves its file into `bin/<slug>/` and
/// records it, with its key pinned, under the hub directory's lock. Refused
/// unless the preview is complete (its key trusted, its manifest read), and
/// if the repository or the slug was installed since the preview.
pub(crate) fn install_staged(hub: &HubDir, staged: &Staged) -> Result<RegistryEntry, HubError> {
    let preview = &staged.preview;
    let manifest = staged.manifest()?;
    let _lock = hub.lock()?;
    // Read again: anything may have changed since the preview.
    let mut registry = hub.read_registry()?;
    refuse_installed(&registry, &preview.repo, &preview.slug)?;
    let (path, name) = place(hub, staged)?;
    let entry = RegistryEntry {
        slug: preview.slug.clone(),
        path: path.clone(),
        size: preview.size,
        sha256: preview.sha256.clone(),
        manifest: manifest.clone(),
        source: Source::Github { repo: preview.repo.clone() },
        pinned_key: Some(staged.key.as_base64().to_owned()),
        previous: None,
    };
    registry.insert(entry.clone());
    if let Err(error) = hub.write_registry(&registry) {
        remove_file(&path);
        return Err(error.into());
    }
    if let Some(directory) = path.parent() {
        sweep(directory, &[&name]);
    }
    Ok(entry)
}

/// Installs a staged update: moves its file into `bin/<slug>/` beside the
/// current one, records it with the current one as the previous version
/// and the key it was verified with, then removes the older previous
/// version, all under the hub directory's lock. Refused unless the preview
/// is complete, and if the entry changed since the preview.
pub(crate) fn update_staged(hub: &HubDir, staged: &Staged) -> Result<RegistryEntry, HubError> {
    let preview = &staged.preview;
    let manifest = staged.manifest()?;
    let slug = &preview.slug;
    let _lock = hub.lock()?;
    let mut registry = hub.read_registry()?;
    let current = registry.get(slug).cloned();
    let (Some(before), Some(current)) = (&staged.replaces, current) else {
        return Err(HubError::ChangedMeanwhile { slug: slug.clone() });
    };
    if *before != current {
        return Err(HubError::ChangedMeanwhile { slug: slug.clone() });
    }
    let directory = hub.bin_dir(slug)?;
    let current_name = own_file_name(&directory, &current)?;
    let (path, name) = place(hub, staged)?;
    let entry = RegistryEntry {
        slug: slug.clone(),
        path: path.clone(),
        size: preview.size,
        sha256: preview.sha256.clone(),
        manifest: manifest.clone(),
        source: current.source.clone(),
        pinned_key: Some(staged.key.as_base64().to_owned()),
        previous: Some(PreviousVersion {
            file_name: current_name.clone(),
            size: current.size,
            sha256: current.sha256.clone(),
            manifest: current.manifest.clone(),
        }),
    };
    registry.insert(entry.clone());
    if let Err(error) = hub.write_registry(&registry) {
        if name != current_name {
            remove_file(&path);
        }
        return Err(error.into());
    }
    sweep(&directory, &[&name, &current_name]);
    Ok(entry)
}

/// Restores the previous version of `slug`, after checking its copy against
/// the registry, then removes the copy it replaces, under the hub
/// directory's lock. The restored version has no previous version.
pub(crate) fn rollback_entry(hub: &HubDir, slug: &str) -> Result<RegistryEntry, HubError> {
    let _lock = hub.lock()?;
    let mut registry = hub.read_registry()?;
    let current = registry.get(slug).cloned().ok_or_else(|| HubError::NotInstalled {
        slug: slug.to_owned(),
        registry: hub.registry_path(),
    })?;
    let previous =
        current.previous.clone().ok_or_else(|| HubError::NoPrevious { slug: slug.to_owned() })?;
    let directory = hub.bin_dir(slug)?;
    // The registry names a file, never a path: anything else is refused.
    if !verify::is_asset_name(&previous.file_name) {
        return Err(HubError::ForeignExecutable {
            slug: slug.to_owned(),
            path: PathBuf::from(&previous.file_name),
            expected: directory,
        });
    }
    let path = directory.join(&previous.file_name);
    let usable = matches!(
        hub_dir::file_digest(&path),
        Ok((size, sha256)) if size == previous.size && sha256 == previous.sha256
    );
    if !usable {
        return Err(HubError::PreviousUnusable { slug: slug.to_owned(), path });
    }
    let restored = RegistryEntry {
        slug: slug.to_owned(),
        path,
        size: previous.size,
        sha256: previous.sha256.clone(),
        manifest: previous.manifest.clone(),
        source: current.source.clone(),
        pinned_key: current.pinned_key.clone(),
        previous: None,
    };
    registry.insert(restored.clone());
    hub.write_registry(&registry)?;
    sweep(&directory, &[&previous.file_name]);
    Ok(restored)
}

/// The catalog of `repo`, refused when written in a newer format.
pub(crate) fn fetch_catalog(github: &dyn GitHub, repo: &str) -> Result<Catalog, HubError> {
    let catalog = github.catalog(repo)?;
    if catalog.format_version > CATALOG_FORMAT_VERSION {
        return Err(HubError::CatalogVersion {
            repo: repo.to_owned(),
            found: catalog.format_version,
            supported: CATALOG_FORMAT_VERSION,
        });
    }
    Ok(catalog)
}

/// The catalog of `repo` as the hub lists it: its entries that name a
/// repository, each with the slug installed from it; or why it could not be
/// read. A repository listed twice keeps its first entry, the one
/// [`catalog_key`] takes the key from.
pub(crate) fn list_catalog(github: &dyn GitHub, repo: &str, registry: &Registry) -> HubCatalog {
    match fetch_catalog(github, repo) {
        Ok(catalog) => {
            let mut dashboards: Vec<HubCatalogEntry> = Vec::new();
            for entry in catalog.dashboards {
                if !is_repo(&entry.repo) {
                    tracing::warn!(catalog = repo, entry = %entry.repo, "skipped a catalog entry that names no repository");
                    continue;
                }
                if dashboards.iter().any(|listed| same_repo(&listed.repo, &entry.repo)) {
                    tracing::warn!(catalog = repo, entry = %entry.repo, "skipped a catalog entry that repeats a repository listed before");
                    continue;
                }
                dashboards.push(HubCatalogEntry {
                    installed_slug: installed_from(registry, &entry.repo)
                        .map(|installed| installed.slug.clone()),
                    repo: entry.repo,
                    name: entry.name,
                    description: entry.description,
                });
            }
            HubCatalog { repo: repo.to_owned(), name: Some(catalog.name), dashboards, error: None }
        }
        Err(error) => unreadable_catalog(repo, &error),
    }
}

/// A catalog that could not be read, and why.
pub(crate) fn unreadable_catalog(repo: &str, error: &HubError) -> HubCatalog {
    tracing::warn!(catalog = repo, %error, "could not read a catalog");
    HubCatalog {
        repo: repo.to_owned(),
        name: None,
        dashboards: Vec::new(),
        error: Some(api_error(error)),
    }
}

/// The key the first of `catalogs` that lists `repo` gives for it. A catalog
/// that cannot be read vouches for nothing and is skipped: the key is then
/// `unknown`, and the user must compare it, never trusted silently. A key
/// that is not one refuses the preview.
pub(crate) fn catalog_key(
    github: &dyn GitHub,
    catalogs: &[String],
    repo: &str,
) -> Result<Option<PublicKey>, HubError> {
    for catalog in catalogs {
        match fetch_catalog(github, catalog) {
            Ok(found) => {
                if let Some(entry) =
                    found.dashboards.into_iter().find(|entry| same_repo(&entry.repo, repo))
                {
                    return Ok(Some(PublicKey::from_base64(repo, &entry.public_key)?));
                }
            }
            Err(error) => {
                tracing::warn!(catalog = %catalog, %error, "could not read a catalog; its keys are not used");
            }
        }
    }
    Ok(None)
}

/// What the latest release of `repo` means for `entry`, installed from it.
pub(crate) fn check_entry(
    github: &dyn GitHub,
    entry: &RegistryEntry,
    repo: &str,
) -> DashboardUpdate {
    let release = match github.latest_release(repo) {
        Ok(release) => release,
        Err(error) => return could_not_check(entry, repo, &error.into()),
    };
    let latest = tag_version(&release.tag);
    let installed = entry.manifest.version.as_deref();
    let available = latest
        .clone()
        .filter(|_| compare_versions(latest.as_deref(), installed) == Some(Ordering::Greater));
    match &available {
        Some(version) => {
            tracing::info!(dashboard = %entry.slug, repo, version, "an update is available");
        }
        None => tracing::debug!(dashboard = %entry.slug, repo, tag = %release.tag, "up to date"),
    }
    DashboardUpdate {
        slug: entry.slug.clone(),
        repo: repo.to_owned(),
        latest_version: latest,
        available_version: available,
        error: None,
    }
}

/// An entry whose latest release could not be read.
pub(crate) fn could_not_check(
    entry: &RegistryEntry,
    repo: &str,
    error: &HubError,
) -> DashboardUpdate {
    tracing::warn!(dashboard = %entry.slug, repo, %error, "could not check for an update");
    DashboardUpdate {
        slug: entry.slug.clone(),
        repo: repo.to_owned(),
        latest_version: None,
        available_version: None,
        error: Some(api_error(error)),
    }
}

/// The version the last check found for `entry`, if it is still newer than
/// the installed one.
pub(crate) fn available_version(
    check: Option<&UpdateCheck>,
    entry: &RegistryEntry,
) -> Option<String> {
    let Source::Github { repo } = &entry.source else {
        return None;
    };
    let found = check?
        .dashboards
        .iter()
        .find(|update| update.slug == entry.slug && same_repo(&update.repo, repo))?;
    let available = found.available_version.as_deref();
    let newer = compare_versions(available, entry.manifest.version.as_deref());
    available.filter(|_| newer == Some(Ordering::Greater)).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn release(names: &[&str]) -> Release {
        Release {
            tag: "v1.0.0".to_owned(),
            assets: names
                .iter()
                .map(|name| ReleaseAsset {
                    name: (*name).to_owned(),
                    size: 1,
                    download_url: String::new(),
                })
                .collect(),
        }
    }

    fn order(first: &str, second: &str) -> Option<Ordering> {
        compare_versions(Some(first), Some(second))
    }

    #[test]
    fn versions_follow_semver_precedence() {
        // The precedence example of SemVer 2.0, item 11.
        let ordered = [
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
            "1.0.1",
            "1.1.0",
            "1.10.0",
            "2.0.0",
        ];
        for pair in ordered.windows(2) {
            assert_eq!(order(pair[1], pair[0]), Some(Ordering::Greater), "{pair:?}");
            assert_eq!(order(pair[0], pair[1]), Some(Ordering::Less), "{pair:?}");
        }
        assert_eq!(order("1.2.3", "v1.2.3"), Some(Ordering::Equal));
        assert_eq!(order("1.2.3+build.5", "1.2.3"), Some(Ordering::Equal), "build is ignored");
        assert_eq!(order("0.0.0", "0.0.0-test.1"), Some(Ordering::Greater));
    }

    #[test]
    fn anything_but_semver_cannot_be_compared() {
        for text in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "01.2.3",
            "1.02.3",
            "1.2.3-",
            "1.2.3-01",
            "1.2.3-a..b",
            "1.2.3+",
            "1.2.3+a..b",
            "1.2.3-a b",
            "vv1.2.3",
            "V1.2.3",
            "latest",
            "1.2.x",
            "-1.2.3",
            "1.2.3-é",
        ] {
            assert_eq!(order(text, "1.0.0"), None, "{text:?}");
            assert_eq!(order("1.0.0", text), None, "{text:?}");
        }
        assert_eq!(compare_versions(None, Some("1.0.0")), None);
        assert_eq!(compare_versions(Some("1.0.0"), None), None);
        assert_eq!(tag_version("v2.1.0"), Some("2.1.0".to_owned()));
        assert_eq!(tag_version("2.1.0-rc.1"), Some("2.1.0-rc.1".to_owned()));
        assert_eq!(tag_version("release-2"), None);
    }

    #[test]
    fn a_first_install_takes_the_one_asset_for_this_target() -> TestResult {
        let target = Target::LinuxX86_64;
        let found = release(&[
            "SHA256SUMS.txt",
            "signal-lab-x86_64-pc-windows-msvc.exe",
            "signal-lab-x86_64-unknown-linux-gnu",
            "signal-lab-x86_64-unknown-linux-gnu.tar.gz",
            "Not A Slug-x86_64-unknown-linux-gnu",
        ]);
        let (asset, slug) = find_asset("o/r", &found, target)?;
        assert_eq!(asset.name, "signal-lab-x86_64-unknown-linux-gnu");
        assert_eq!(slug, "signal-lab");

        let other_target = release(&["signal-lab-aarch64-apple-darwin"]);
        let error = find_asset("o/r", &other_target, target).err().ok_or("no asset")?;
        assert_eq!(error.code(), "hub_no_asset");
        assert!(error.to_string().contains("x86_64-unknown-linux-gnu"), "{error}");

        let two = release(&["a-x86_64-unknown-linux-gnu", "b-x86_64-unknown-linux-gnu"]);
        let error = find_asset("o/r", &two, target).err().ok_or("two assets")?;
        assert_eq!(error.code(), "hub_ambiguous_asset");
        let message = error.to_string();
        assert!(message.contains("a-x86_64") && message.contains("b-x86_64"), "{message}");
        Ok(())
    }

    #[test]
    fn keys_are_trusted_pinned_first_then_catalog_then_accepted() -> TestResult {
        let key = |text: &str| PublicKey::from_base64("o/r", text);
        let a = key("RWRA+IGA6EB9AuIbrrq9pGIpkX+AusRGz1XiVhgRrTfRER9SJa+jD9+x")?;
        let b = key("RWQrHdp6bVYNJfouycVFGpYdTZQx74XTWGu6GVziED4qg9k3cewojLTb")?;
        let none = Trust::default();
        assert_eq!(none.status(&a), KeyStatus::Unknown);
        let accepted = Trust { accepted: Some(a.clone()), ..Trust::default() };
        assert_eq!(accepted.status(&a), KeyStatus::Accepted);
        assert_eq!(accepted.status(&b), KeyStatus::Unknown);
        let pinned =
            Trust { pinned: Some(a.clone()), accepted: Some(b.clone()), ..Trust::default() };
        assert_eq!(pinned.status(&a), KeyStatus::Pinned);
        assert_eq!(pinned.status(&b), KeyStatus::Accepted, "a change the user accepted");
        let catalog = Trust { catalog: Some(a.clone()), ..Trust::default() };
        assert_eq!(catalog.status(&a), KeyStatus::Catalog);
        assert_eq!(catalog.status(&b), KeyStatus::Changed);
        assert_eq!(catalog.known_key(), Some(&a));
        for status in [KeyStatus::Unknown, KeyStatus::Changed] {
            assert!(!is_trusted(status), "{status:?}");
        }
        Ok(())
    }

    /// A release staged in `area` with `content`, signed by a key nobody
    /// trusts yet.
    fn pending(
        area: &Arc<StagingArea>,
        content: &[u8],
    ) -> Result<Staged, Box<dyn std::error::Error>> {
        let directory = StagingDir::create(area)?;
        let asset = "data-explorer-x86_64-unknown-linux-gnu".to_owned();
        let file = directory.path.join(&asset);
        fs::write(&file, content)?;
        let (size, sha256) = hub_dir::file_digest(&file)?;
        let key = PublicKey::from_base64(
            "o/r",
            "RWRA+IGA6EB9AuIbrrq9pGIpkX+AusRGz1XiVhgRrTfRER9SJa+jD9+x",
        )?;
        let preview = InstallPreview {
            repo: "o/r".to_owned(),
            slug: "data-explorer".to_owned(),
            manifest: None,
            tag: "v1.0.0".to_owned(),
            asset,
            size,
            sha256,
            public_key: key.as_base64().to_owned(),
            key_sha256: key.sha256(),
            key_status: KeyStatus::Unknown,
        };
        Ok(Staged { preview, key, replaces: None, file, directory })
    }

    #[test]
    fn a_release_whose_key_is_not_trusted_is_neither_run_nor_installed() -> TestResult {
        let directory = tempfile::tempdir()?;
        let hub = HubDir::at(directory.path());
        let area = Arc::new(StagingArea::create(&hub)?);
        let timeout = Duration::from_secs(5);

        // Running this file would fail with `hub_manifest_failed`.
        let mut staged = pending(&area, b"not an executable")?;
        fn code<T>(result: Result<T, HubError>) -> Option<&'static str> {
            result.err().map(|error| error.code())
        }
        assert_eq!(code(run_manifest(&mut staged, timeout)), Some("hub_key_not_accepted"));
        assert_eq!(code(install_staged(&hub, &staged)), Some("hub_key_not_accepted"));
        staged.preview.key_status = KeyStatus::Changed;
        assert_eq!(code(update_staged(&hub, &staged)), Some("hub_key_changed"));
        assert_eq!(hub.read_registry()?, Registry::default(), "nothing was recorded");
        assert!(!hub.root().join("bin").exists(), "nothing was placed");
        assert!(staged.file.is_file(), "the verified file still waits");

        // Accepted, it is hashed again before it runs: gone, or changed, it
        // is refused.
        fs::remove_file(&staged.file)?;
        let error = accept_staged(staged, timeout).err().ok_or("the file is gone")?;
        assert_eq!(error.code(), "hub_preview_gone");
        assert!(error.to_string().contains("preview it again"), "{error}");
        let staged = pending(&area, b"as verified")?;
        fs::write(&staged.file, b"changed meanwhile")?;
        assert_eq!(code(accept_staged(staged, timeout)), Some("release_hash_mismatch"));
        Ok(())
    }

    #[test]
    fn a_hub_start_removes_only_the_staging_areas_nobody_holds() -> TestResult {
        let directory = tempfile::tempdir()?;
        let hub = HubDir::at(directory.path());
        clean_staging(&hub)?;

        let held = Arc::new(StagingArea::create(&hub)?);
        let preview = StagingDir::create(&held)?;
        fs::write(preview.path.join("asset"), "verified")?;
        let unlocked = hub.staging_dir().join("1-0");
        fs::create_dir_all(unlocked.join("2"))?;
        fs::write(unlocked.join(STAGING_LOCK), "")?;
        let lockless = hub.staging_dir().join("2-0");
        fs::create_dir_all(&lockless)?;
        fs::write(hub.staging_dir().join("stray"), "")?;

        clean_staging(&hub)?;
        assert!(preview.path.join("asset").is_file(), "an area in use is kept");
        assert!(!unlocked.exists() && !lockless.exists(), "stale areas are removed");
        assert!(!hub.staging_dir().join("stray").exists());

        let area = held.path.clone();
        drop(preview);
        assert!(area.is_dir(), "the area outlives its previews");
        drop(held);
        assert!(!area.exists(), "an area is removed when its supervisor is dropped");
        Ok(())
    }

    #[test]
    fn catalogs_default_to_eario_s_until_changed() -> TestResult {
        let directory = tempfile::tempdir()?;
        let hub = HubDir::at(directory.path());
        assert_eq!(read_catalogs(&hub)?, [DEFAULT_CATALOG]);
        write_catalogs(&hub, &[])?;
        assert!(read_catalogs(&hub)?.is_empty(), "removing the default sticks");
        write_catalogs(&hub, &["lab/catalog".to_owned()])?;
        assert_eq!(read_catalogs(&hub)?, ["lab/catalog"]);

        fs::write(hub.catalogs_path(), r#"{"format_version":1,"catalogs":["../x"]}"#)?;
        let error = read_catalogs(&hub).err().ok_or("not a repository")?;
        assert_eq!(error.code(), "hub_file_invalid");
        assert!(error.to_string().contains("catalogs.json"), "{error}");
        fs::write(hub.catalogs_path(), r#"{"format_version":2,"catalogs":[]}"#)?;
        assert_eq!(read_catalogs(&hub).err().map(|error| error.code()), Some("hub_file_invalid"));
        Ok(())
    }

    #[test]
    fn settings_and_checks_are_stored_in_the_hub_directory() -> TestResult {
        let directory = tempfile::tempdir()?;
        let hub = HubDir::at(directory.path());
        assert!(read_settings(&hub)?.check_updates_at_start);
        write_settings(&hub, &HubSettings { check_updates_at_start: false })?;
        assert!(!read_settings(&hub)?.check_updates_at_start);

        assert_eq!(read_last_check(&hub)?, None);
        let check = UpdateCheck { checked_at: 5, dashboards: Vec::new(), hub: None };
        write_last_check(&hub, &check)?;
        assert_eq!(read_last_check(&hub)?, Some(check));
        Ok(())
    }

    #[test]
    fn copies_have_one_name_per_content() {
        let name = copy_name("signal-lab", &"ab".repeat(32));
        assert_eq!(name, format!("signal-lab-abababababab{}", std::env::consts::EXE_SUFFIX));
        assert!(verify::is_asset_name(&name));
        assert_ne!(name, copy_name("signal-lab", &"cd".repeat(32)));
    }

    #[test]
    fn a_sweep_keeps_only_what_it_is_told_to() -> TestResult {
        let directory = tempfile::tempdir()?;
        for name in ["current", "previous", "older"] {
            fs::write(directory.path().join(name), name)?;
        }
        fs::create_dir(directory.path().join("a-directory"))?;
        sweep(directory.path(), &["current", "previous"]);
        let mut names: Vec<String> = fs::read_dir(directory.path())?
            .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
            .collect::<Result<_, _>>()?;
        names.sort();
        assert_eq!(names, ["a-directory", "current", "previous"]);
        Ok(())
    }

    /// A GitHub that has one catalog and no release.
    struct OneCatalog(Catalog);

    impl GitHub for OneCatalog {
        fn latest_release(&self, repo: &str) -> Result<Release, GitHubError> {
            Err(GitHubError::NoRelease { repo: repo.to_owned() })
        }

        fn catalog(&self, _repo: &str) -> Result<Catalog, GitHubError> {
            Ok(self.0.clone())
        }

        fn release_file(
            &self,
            _repo: &str,
            _release: &Release,
            _name: &str,
            _limit: u64,
        ) -> Result<Option<Vec<u8>>, GitHubError> {
            Ok(None)
        }

        fn download(
            &self,
            repo: &str,
            _asset: &ReleaseAsset,
            _out: &mut dyn std::io::Write,
        ) -> Result<Download, GitHubError> {
            Err(GitHubError::NoRelease { repo: repo.to_owned() })
        }
    }

    #[test]
    fn a_catalog_lists_each_repository_once_keeping_its_first_entry() {
        let entry = |repo: &str, name: &str| eario_core::CatalogEntry {
            repo: repo.to_owned(),
            name: name.to_owned(),
            description: String::new(),
            public_key: String::new(),
        };
        let github = OneCatalog(Catalog {
            format_version: CATALOG_FORMAT_VERSION,
            name: "Lab".to_owned(),
            dashboards: vec![
                entry("lab/signal-lab", "Signal lab"),
                entry("lab/data-explorer", "Data explorer"),
                entry("Lab/Signal-Lab", "Signal lab, again"),
                entry("lab/signal-lab", "Signal lab, a third time"),
                entry("not a repository", "Nothing"),
            ],
        });
        let listed = list_catalog(&github, "lab/catalog", &Registry::default());
        assert_eq!(listed.error, None);
        let entries: Vec<(&str, &str)> = listed
            .dashboards
            .iter()
            .map(|entry| (entry.repo.as_str(), entry.name.as_str()))
            .collect();
        assert_eq!(
            entries,
            [("lab/signal-lab", "Signal lab"), ("lab/data-explorer", "Data explorer")]
        );
    }
}
