//! The hub updates itself (ADR 0013) from the latest release of
//! [`HUB_REPO`], verified with the Eario release key compiled into it. Call
//! the blocking functions through `tokio::task::spawn_blocking`.
//!
//! # The key
//!
//! A release of the hub is verified with one key only: the minisign public
//! key file `crates/eario-hub/eario-release.pub`, embedded when the hub is
//! compiled. The release's own `minisign.pub` is never read, no catalog is
//! consulted and no key is trusted on first use: a key that the hub does not
//! hold already proves nothing about a new hub. While that file is the
//! placeholder committed with the source, the hub does not update itself
//! (`self_update_not_configured`), and its update check reports so without a
//! request.
//!
//! # Verification
//!
//! A self-update takes the path of a dashboard's preview, with that key:
//!
//! 1. The latest release's tag must name a newer version than this hub's;
//!    otherwise it is refused before anything is downloaded.
//! 2. Its `SHA256SUMS.txt` must carry a valid signature by the key and list
//!    `eario-hub-<target triple>[.exe]`, which is downloaded into this
//!    process's staging area, bounded by the size the release lists; its
//!    SHA-256 must be the signed one.
//! 3. Only then is it run, with `--version` alone (no standard input, bounded
//!    output, a timeout, no console window), and the version it prints must
//!    be strictly newer than this hub's, by SemVer precedence. The tag is not
//!    signed and the executable is, so an older signed release served under
//!    a newer tag cannot downgrade the hub. The run also shows that the new
//!    executable starts on this machine before it replaces the hub.
//!
//! # The swap
//!
//! The verified file is copied beside the running executable as
//! `<name>.new`, with the executable's permissions on Unix, and hashed again,
//! then swapped in; the running executable is kept as `<name>.old`. On Unix
//! `<name>.old` is a hard link to it, and `<name>.new` is renamed over it,
//! which replaces it at once. On Windows, which lets a
//! running executable be renamed but not deleted or replaced, and on a Unix
//! file system without hard links, the running executable is renamed to
//! `<name>.old`, then `<name>.new` takes its name; if that second rename
//! fails, the first is undone. Either way, on failure `<name>.new` is removed
//! and the hub is left as it was. Between the two renames there is no file at
//! the executable's name: a crash or a power loss at that instant leaves
//! `<name>.old` and `<name>.new` for the user to rename. The next start of
//! the hub removes `<name>.old`. A hub that runs from a folder it cannot
//! write to refuses before downloading anything (`self_update_not_writable`).
//!
//! # The restart
//!
//! After a swap, a hub serving its API ([`crate::start`]) stops its server,
//! as Quit does, even if the request that asked for the update is gone;
//! [`crate::run`] then removes `hub.json`, waits for a self-update still in
//! progress, starts the new executable with no arguments, no standard
//! streams, no console window and a process group of its own, and exits.
//! The new hub finds no hub answering and starts. Dashboards are separate
//! processes: they keep running, and the new hub re-attaches to them.
//!
//! While the system reports the new executable busy (an antivirus scanning
//! it, on Windows), the start is tried again for about 10 s. If it still
//! fails, the old hub exits all the same, and may have no terminal to say
//! so: it appends a line to `self-update.log` in the hub directory, naming
//! the executable, and the user starts it again. The new version is in
//! place either way.

use std::cmp::Ordering;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eario_core::{HubUpdate, UpdateCheck};
use eario_runtime::hub_dir::{self, HubDir};

use crate::github::GitHub;
use crate::remote::{
    self, StagingArea, Verified, api_error, compare_versions, tag_version, version_text,
};
use crate::supervisor::{HubError, detach};
use crate::verify::{PublicKey, Target, VerifyError, asset_name};

/// The repository the hub's own releases are published in.
pub const HUB_REPO: &str = "rtonneau/eario";

/// Slug of the hub in the names of its release assets:
/// `eario-hub-<target triple>[.exe]`.
pub const HUB_SLUG: &str = "eario-hub";

/// The argument that makes `eario-hub` print `eario-hub <version>` and exit.
pub const VERSION_ARGUMENT: &str = "--version";

/// The Eario release public key file, compiled into the hub.
const COMPILED_KEY_FILE: &str = include_str!("../eario-release.pub");

/// The key line of the placeholder key file, which is no key.
const PLACEHOLDER_KEY_LINE: &str = "PLACEHOLDER-NOT-A-KEY";

/// Longest version, in bytes, read from a new executable's `--version`.
const MAX_VERSION_BYTES: usize = 64;

/// Longest a starting hub keeps trying to remove the executable the last
/// self-update replaced, while the hub that ran from it is still exiting.
const OLD_EXECUTABLE_PATIENCE: Duration = Duration::from_secs(10);

/// Attempts at starting the new hub after a self-update, 50 ms apart, while
/// the system reports its executable busy: about 10 s in all.
const RESTART_ATTEMPTS: u32 = 200;

/// Why the hub did not update itself. Messages name the file or the version
/// and say what to do.
#[derive(Debug, thiserror::Error)]
pub enum SelfUpdateError {
    /// The hub holds no release key: its key file is the placeholder.
    #[error(
        "this hub was built without the Eario release key (its `eario-release.pub` is the placeholder), so it cannot verify a release of itself and does not update itself; download the new version from https://github.com/rtonneau/eario/releases and replace this executable"
    )]
    NotConfigured,
    /// Eario publishes no hub for the platform this one runs on.
    #[error(
        "Eario publishes no hub for this platform ({os} {arch}), so this hub does not update itself",
        os = std::env::consts::OS,
        arch = std::env::consts::ARCH
    )]
    UnsupportedPlatform,
    /// The hub's own executable could not be found.
    #[error("could not find the hub's own executable, so it cannot replace it: {source}")]
    NoExecutable {
        /// The underlying failure.
        source: io::Error,
    },
    /// The hub cannot write beside its executable.
    #[error(
        "the hub runs from {path}, where it cannot write a file ({source}), so it cannot replace itself; move `eario-hub` to a folder you can write to, or replace it there with the new version yourself"
    )]
    NotWritable {
        /// The executable's directory.
        path: PathBuf,
        /// The underlying failure.
        source: io::Error,
    },
    /// The latest release is not newer than this hub.
    #[error(
        "the latest release of the Eario hub is version {found}, not newer than this hub's version {installed}; nothing to update"
    )]
    NotNewer {
        /// This hub's version.
        installed: String,
        /// Version of the release.
        found: String,
    },
    /// The versions cannot be compared.
    #[error(
        "the version of the latest Eario hub ({found}) or of this hub ({installed}) is not a SemVer version such as 1.2.0, so the hub cannot tell an update from a downgrade and does not update itself"
    )]
    VersionNotComparable {
        /// This hub's version.
        installed: String,
        /// Version of the release.
        found: String,
    },
    /// The verified executable did not print its version.
    #[error(
        "`{asset}` of `rtonneau/eario` was verified, but did not give its version: {detail}; the hub was not replaced"
    )]
    ProbeFailed {
        /// The asset.
        asset: String,
        /// What happened.
        detail: String,
    },
    /// A step of the swap failed; the hub was left as it was.
    #[error(
        "could not replace the hub at {path}: {step} failed ({source}); the hub was left as it was"
    )]
    Replace {
        /// The hub's executable.
        path: PathBuf,
        /// What failed.
        step: &'static str,
        /// The underlying failure.
        source: io::Error,
    },
    /// The new version could not be put in place, nor the old one back.
    #[error(
        "could not put the new hub in place at {path} ({source}), nor rename the previous one back ({restore}); rename {old} (the previous version) or {new} (the new one) to {path} yourself"
    )]
    RestoreFailed {
        /// The hub's executable, which is now missing.
        path: PathBuf,
        /// Where the previous executable is.
        old: PathBuf,
        /// Where the new executable is.
        new: PathBuf,
        /// Why the new one could not be put in place.
        source: io::Error,
        /// Why the previous one could not be renamed back.
        restore: io::Error,
    },
    /// This hub has updated itself already.
    #[error("the hub has updated itself already and restarts; wait for it to open again")]
    Pending,
    /// The command line was asked to update while a hub runs.
    #[error(
        "an Eario hub is running, at {address}; update it from the hub's own page, or quit it first and run this command again"
    )]
    HubRunning {
        /// Address of the running hub.
        address: String,
    },
}

impl SelfUpdateError {
    /// Stable, machine-readable code of the error.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotConfigured => "self_update_not_configured",
            Self::UnsupportedPlatform => "self_update_unsupported_platform",
            Self::NoExecutable { .. } => "self_update_no_executable",
            Self::NotWritable { .. } => "self_update_not_writable",
            Self::NotNewer { .. } => "self_update_not_newer",
            Self::VersionNotComparable { .. } => "self_update_version_not_comparable",
            Self::ProbeFailed { .. } => "self_update_probe_failed",
            Self::Replace { .. } => "self_update_failed",
            Self::RestoreFailed { .. } => "self_update_restore_failed",
            Self::Pending => "self_update_pending",
            Self::HubRunning { .. } => "self_update_hub_running",
        }
    }
}

/// What the hub updates itself with: the key its releases must be signed
/// with, the executable to replace and the version it runs.
#[derive(Debug, Clone)]
pub struct SelfUpdateConfig {
    /// `None` when the hub holds no release key.
    key: Option<PublicKey>,
    /// `None` for the executable of this process.
    executable: Option<PathBuf>,
    version: String,
}

impl SelfUpdateConfig {
    /// The hub's own: the key compiled into it, the executable of this
    /// process and the version it was built as. Every hub and every command
    /// uses it; only tests can choose another.
    pub fn compiled() -> Self {
        let key = compiled_key().unwrap_or_else(|error| {
            // A unit test keeps the committed file the placeholder or a key;
            // were it neither, the hub must not update itself.
            tracing::error!(%error, "the compiled Eario release key is not a key; the hub does not update itself");
            None
        });
        Self { key, executable: None, version: env!("CARGO_PKG_VERSION").to_owned() }
    }

    /// For tests: `key` in place of the compiled key (`None`: the hub holds
    /// none), `executable` in place of this process's, and `version` as the
    /// version of the running hub.
    #[cfg(feature = "fake-github")]
    pub fn for_tests(key: Option<PublicKey>, executable: PathBuf, version: &str) -> Self {
        Self { key, executable: Some(executable), version: version.to_owned() }
    }

    /// Whether the hub can update itself: it holds a release key.
    pub fn is_configured(&self) -> bool {
        self.key.is_some()
    }

    /// The version of the running hub.
    pub fn version(&self) -> &str {
        &self.version
    }

    fn key(&self) -> Result<&PublicKey, SelfUpdateError> {
        self.key.as_ref().ok_or(SelfUpdateError::NotConfigured)
    }

    /// The executable to replace: a file, with a name and a directory.
    pub(crate) fn executable(&self) -> Result<PathBuf, SelfUpdateError> {
        let path = match &self.executable {
            Some(path) => path.clone(),
            None => std::env::current_exe()
                .map_err(|source| SelfUpdateError::NoExecutable { source })?,
        };
        let named = path.file_name().is_some() && path.parent().is_some();
        if !named || !path.is_file() {
            let source = io::Error::new(
                io::ErrorKind::NotFound,
                format!("{} is not a file", path.display()),
            );
            return Err(SelfUpdateError::NoExecutable { source });
        }
        Ok(path)
    }
}

/// The key compiled into the hub; `None` while its file is the placeholder.
fn compiled_key() -> Result<Option<PublicKey>, VerifyError> {
    read_key_file(COMPILED_KEY_FILE)
}

/// The key of a minisign public key file; `None` for the placeholder, whose
/// key line is [`PLACEHOLDER_KEY_LINE`]. Refused when it is neither.
fn read_key_file(text: &str) -> Result<Option<PublicKey>, VerifyError> {
    if text.lines().nth(1) == Some(PLACEHOLDER_KEY_LINE) {
        return Ok(None);
    }
    PublicKey::from_pub_file(HUB_REPO, text.as_bytes()).map(Some)
}

/// What a self-update did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Updated {
    /// Version of the hub replaced.
    pub(crate) previous_version: String,
    /// Version installed.
    pub(crate) version: String,
    /// The executable replaced, now the new version.
    pub(crate) executable: PathBuf,
}

/// Replaces the hub's executable with the latest release of [`HUB_REPO`],
/// once it passed every check of the module documentation; `area` holds the
/// download meanwhile and `timeout` bounds the run of the new executable.
/// Refused before any request when the hub holds no key
/// (`self_update_not_configured`) or cannot write beside its executable.
pub(crate) fn update(
    area: &Arc<StagingArea>,
    github: &dyn GitHub,
    config: &SelfUpdateConfig,
    target: Option<Target>,
    timeout: Duration,
) -> Result<Updated, HubError> {
    let key = config.key()?;
    let target = target.ok_or(SelfUpdateError::UnsupportedPlatform)?;
    let executable = config.executable()?;
    let replacement = sibling(&executable, ".new");
    check_writable(&executable, &replacement)?;

    let release = github.latest_release(HUB_REPO)?;
    let installed = config.version();
    let tagged = tag_version(&release.tag);
    // The tag is not signed, but an older one is refused before anything is
    // downloaded.
    if matches!(
        compare_versions(tagged.as_deref(), Some(installed)),
        Some(Ordering::Less | Ordering::Equal)
    ) {
        return Err(SelfUpdateError::NotNewer {
            installed: installed.to_owned(),
            found: version_text(tagged.as_deref()),
        }
        .into());
    }
    let name = asset_name(HUB_SLUG, target)?;
    let asset = release
        .asset(&name)
        .ok_or_else(|| HubError::NoAsset { repo: HUB_REPO.to_owned(), asset: name.clone() })?;
    let sums = remote::signed_sums(github, HUB_REPO, &release)?;
    // The compiled key, never one the release carries.
    let verified = remote::download_verified(area, github, HUB_REPO, asset, key, &sums)?;

    let version = read_version(&asset.name, &verified, timeout)?;
    match compare_versions(Some(&version), Some(installed)) {
        Some(Ordering::Greater) => {}
        Some(_) => {
            return Err(SelfUpdateError::NotNewer {
                installed: installed.to_owned(),
                found: version,
            }
            .into());
        }
        None => {
            return Err(SelfUpdateError::VersionNotComparable {
                installed: installed.to_owned(),
                found: version,
            }
            .into());
        }
    }
    place(&asset.name, &verified, &executable, &replacement)?;
    swap(&executable, &replacement)?;
    tracing::info!(
        executable = %executable.display(),
        previous = installed,
        version,
        sha256 = %verified.sha256,
        "the hub updated itself"
    );
    Ok(Updated { previous_version: installed.to_owned(), version, executable })
}

/// The latest release of the hub, and whether it is newer than this one.
/// Downloads nothing. Refused (`self_update_not_configured`) before any
/// request when the hub holds no key: a release it cannot verify is not
/// offered.
pub(crate) fn latest(
    config: &SelfUpdateConfig,
    github: impl FnOnce() -> Result<Arc<dyn GitHub>, HubError>,
) -> Result<HubUpdate, HubError> {
    config.key()?;
    let release = github()?.latest_release(HUB_REPO)?;
    let latest = tag_version(&release.tag);
    let newer = compare_versions(latest.as_deref(), Some(config.version()));
    let available = latest.clone().filter(|_| newer == Some(Ordering::Greater));
    match &available {
        Some(version) => tracing::info!(version, "a newer version of the hub is available"),
        None => tracing::debug!(tag = %release.tag, "the hub is up to date"),
    }
    Ok(HubUpdate {
        repo: HUB_REPO.to_owned(),
        latest_version: latest,
        available_version: available,
        error: None,
    })
}

/// What the update check stores for the hub: [`latest`], or why it was not
/// read.
pub(crate) fn check(
    config: &SelfUpdateConfig,
    github: impl FnOnce() -> Result<Arc<dyn GitHub>, HubError>,
) -> HubUpdate {
    latest(config, github).unwrap_or_else(|error| {
        if matches!(error, HubError::SelfUpdate(SelfUpdateError::NotConfigured)) {
            tracing::debug!("the hub holds no release key; it does not check for its own updates");
        } else {
            tracing::warn!(%error, "could not check for a newer version of the hub");
        }
        HubUpdate {
            repo: HUB_REPO.to_owned(),
            latest_version: None,
            available_version: None,
            error: Some(api_error(&error)),
        }
    })
}

/// The newer version of the hub that the last check found, if it is still
/// newer than the running hub's and this hub can update itself: a hub
/// without the release key offers no update, even one that a check by
/// another build stored in the hub directory.
pub(crate) fn available_version(
    check: Option<&UpdateCheck>,
    config: &SelfUpdateConfig,
) -> Option<String> {
    if !config.is_configured() {
        return None;
    }
    let available = check?.hub.as_ref()?.available_version.as_deref()?;
    let newer =
        compare_versions(Some(available), Some(config.version())) == Some(Ordering::Greater);
    newer.then(|| available.to_owned())
}

/// Runs the verified executable with `--version` and returns the version it
/// prints as `eario-hub <version>`.
fn read_version(asset: &str, verified: &Verified, timeout: Duration) -> Result<String, HubError> {
    let failed = |detail: String| SelfUpdateError::ProbeFailed { asset: asset.to_owned(), detail };
    let directory = verified.directory.path();
    let text =
        remote::probe(&verified.file, VERSION_ARGUMENT, directory, timeout).map_err(failed)?;
    let version =
        text.trim().strip_prefix(HUB_SLUG).and_then(|rest| rest.strip_prefix(' ')).filter(
            |version| {
                // A version takes a few characters; anything else is no answer.
                (1..=MAX_VERSION_BYTES).contains(&version.len())
                    && !version.contains(char::is_whitespace)
            },
        );
    match version {
        Some(version) => Ok(version.to_owned()),
        None => Err(failed(format!("it did not print `{HUB_SLUG} <version>`")).into()),
    }
}

/// `path` with `suffix` added to its file name: `eario-hub.exe` becomes
/// `eario-hub.exe.old`.
pub(crate) fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().map(OsStr::to_os_string).unwrap_or_default();
    name.push(suffix);
    path.with_file_name(name)
}

/// Removes `path` unless it is not there.
fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// Removes `path`, logging a failure.
fn discard(path: &Path) {
    if let Err(error) = remove_if_present(path) {
        tracing::warn!(file = %path.display(), %error, "could not remove a file the self-update left");
    }
}

/// Refuses (`self_update_not_writable`) unless a file can be created beside
/// `executable`: removes a `replacement` left by an update that was
/// interrupted, then creates it and removes it again.
fn check_writable(executable: &Path, replacement: &Path) -> Result<(), SelfUpdateError> {
    let directory = executable.parent().unwrap_or(executable);
    let not_writable = |source| SelfUpdateError::NotWritable { path: directory.to_owned(), source };
    remove_if_present(replacement).map_err(not_writable)?;
    fs::OpenOptions::new().write(true).create_new(true).open(replacement).map_err(not_writable)?;
    remove_if_present(replacement).map_err(not_writable)
}

/// Copies the verified file to `replacement`, beside `executable`, gives it
/// the executable's permissions (Unix), and checks the copy against the
/// verified size and SHA-256. The copy is removed on any failure.
fn place(
    asset: &str,
    verified: &Verified,
    executable: &Path,
    replacement: &Path,
) -> Result<(), HubError> {
    if let Err(source) = copy_executable(&verified.file, replacement, executable) {
        discard(replacement);
        let step = "copying the new version beside it";
        return Err(SelfUpdateError::Replace { path: executable.to_owned(), step, source }.into());
    }
    match hub_dir::file_digest(replacement) {
        Ok((size, sha256)) if size == verified.size && sha256 == verified.sha256 => Ok(()),
        other => {
            discard(replacement);
            match other {
                Err(error) => Err(error.into()),
                Ok(_) => Err(VerifyError::HashMismatch {
                    repo: HUB_REPO.to_owned(),
                    asset: asset.to_owned(),
                }
                .into()),
            }
        }
    }
}

/// Copies `from` to the new file `to`, flushed to disk, with the
/// permissions of `executable`.
fn copy_executable(from: &Path, to: &Path, executable: &Path) -> io::Result<()> {
    let mut source = fs::File::open(from)?;
    let mut target = remote::executable_file_options().write(true).create_new(true).open(to)?;
    io::copy(&mut source, &mut target)?;
    target.sync_all()?;
    // Closed before it is renamed and run.
    drop(target);
    copy_permissions(executable, to)
}

/// Gives `to` the permissions of `from`: the executable bit and the rest.
#[cfg(unix)]
fn copy_permissions(from: &Path, to: &Path) -> io::Result<()> {
    fs::set_permissions(to, fs::metadata(from)?.permissions())
}

/// Windows has no executable bit; the new file keeps the folder's defaults.
#[cfg(not(unix))]
fn copy_permissions(_from: &Path, _to: &Path) -> io::Result<()> {
    Ok(())
}

/// Replaces `executable` by `replacement`, a file in the same directory,
/// keeping the original as `<name>.old`, after removing one left by an
/// earlier update. On failure `replacement` is removed and `executable` is
/// what it was.
///
/// On Unix, `<name>.old` is first made a second name of the executable (a
/// hard link), then `replacement` is renamed over `executable`, which
/// replaces it at once: there is no moment without an executable. Where the
/// link is refused (a file system without hard links, such as FAT or some
/// network shares), and always on Windows, which neither renames over nor
/// deletes a running executable, the executable is renamed aside to
/// `<name>.old` and `replacement` takes its name; if that second rename
/// fails, the first is undone. Between the two renames there is no
/// executable at that name, so a crash or a power loss there leaves only
/// `<name>.old` and `<name>.new`.
pub(crate) fn swap(executable: &Path, replacement: &Path) -> Result<(), SelfUpdateError> {
    let old = sibling(executable, ".old");
    let refuse = |step: &'static str, source: io::Error| {
        discard(replacement);
        SelfUpdateError::Replace { path: executable.to_owned(), step, source }
    };
    // Left by the last update when the hub did not start since.
    if let Err(source) = remove_if_present(&old) {
        return Err(refuse("removing the previous version kept as .old", source));
    }
    if link_aside(executable, &old) {
        return match fs::rename(replacement, executable) {
            Ok(()) => Ok(()),
            Err(source) => {
                // The executable was not touched: only its second name goes.
                discard(&old);
                Err(refuse("putting the new version in place", source))
            }
        };
    }
    if let Err(source) = fs::rename(executable, &old) {
        return Err(refuse("renaming the running executable", source));
    }
    let Err(source) = fs::rename(replacement, executable) else {
        return Ok(());
    };
    match fs::rename(&old, executable) {
        Ok(()) => Err(refuse("putting the new version in place", source)),
        Err(restore) => Err(SelfUpdateError::RestoreFailed {
            path: executable.to_owned(),
            old,
            new: replacement.to_owned(),
            source,
            restore,
        }),
    }
}

/// Unix: makes `old` a second name of `executable` (a hard link), so that
/// renaming the new version over `executable` replaces it at once. `false`
/// when the file system refuses, so the swap renames instead.
#[cfg(unix)]
fn link_aside(executable: &Path, old: &Path) -> bool {
    match fs::hard_link(executable, old) {
        Ok(()) => true,
        Err(error) => {
            tracing::info!(file = %executable.display(), %error, "could not link the executable as .old; renaming it aside instead");
            false
        }
    }
}

/// Windows renames a running executable aside, but cannot rename another
/// file over it: the swap always takes two renames.
#[cfg(not(unix))]
fn link_aside(_executable: &Path, _old: &Path) -> bool {
    false
}

/// Removes `<executable>.old`, the executable the last self-update
/// replaced, if it is there. While the system reports it in use, as it does
/// until the hub that started this one has exited, tries again for up to
/// `patience`. A failure is logged; the next start tries again.
pub(crate) fn remove_old(executable: &Path, patience: Duration) {
    let old = sibling(executable, ".old");
    let started = Instant::now();
    loop {
        match fs::remove_file(&old) {
            Ok(()) => {
                tracing::info!(file = %old.display(), "removed the executable the last self-update replaced");
                return;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => return,
            Err(error) if in_use(&error) && started.elapsed() < patience => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(error) => {
                tracing::warn!(file = %old.display(), %error, "could not remove the executable the last self-update replaced; the next start tries again");
                return;
            }
        }
    }
}

/// [`remove_old`] for the executable of `config`, with the patience a hub
/// start gives it.
pub(crate) fn remove_old_executable(config: &SelfUpdateConfig) {
    match config.executable() {
        Ok(executable) => remove_old(&executable, OLD_EXECUTABLE_PATIENCE),
        Err(error) => tracing::debug!(%error, "no executable to clean after a self-update"),
    }
}

/// Whether removing a file failed because a process still runs from it, or
/// holds it open.
fn in_use(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::PermissionDenied || remote::is_busy(error)
}

/// The command that starts the hub's new executable after a self-update:
/// no arguments, no standard input or output, no console window (Windows)
/// and a process group of its own, in the executable's directory. It
/// inherits this hub's environment, so it serves the same hub directory.
pub(crate) fn restart_command(executable: &Path) -> Command {
    let mut command = Command::new(executable);
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    if let Some(directory) = executable.parent() {
        command.current_dir(directory);
    }
    detach(&mut command);
    command
}

/// Starts the new hub that a self-update installed at `executable`, with
/// [`restart_command`], trying again while the system reports the file busy
/// ([`remote::spawn_retrying`]). Blocking.
///
/// The hub calling this exits next, and may have no terminal: if the new
/// hub cannot be started, a line appended to `self-update.log` in `hub`
/// says so, names the executable, and says that the new version is in
/// place and starts when run again.
pub(crate) fn restart(hub: &HubDir, executable: &Path) -> io::Result<Child> {
    let started = remote::spawn_retrying(&mut restart_command(executable), RESTART_ATTEMPTS);
    if let Err(error) = &started {
        let line = format!(
            "{} (Unix time): the Eario hub updated itself but could not start its new version ({error}). The new version is in place at {}; start it again to open the hub. Dashboards kept running.",
            crate::unix_seconds(),
            executable.display()
        );
        if let Err(log_error) = hub_dir::append_line_at(&hub.self_update_log_path(), &line) {
            tracing::error!(error = %log_error, "could not record in the hub directory that the new hub did not start");
        }
    }
    started
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    /// A minisign public key file.
    const A_KEY_FILE: &str = "untrusted comment: minisign public key\nRWRA+IGA6EB9AuIbrrq9pGIpkX+AusRGz1XiVhgRrTfRER9SJa+jD9+x\n";

    /// A copy of this test executable, as `eario-hub.exe` in a new
    /// directory: an executable that is not the one running.
    fn copied_executable() -> Result<(tempfile::TempDir, PathBuf), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let executable =
            directory.path().join(format!("{HUB_SLUG}{}", std::env::consts::EXE_SUFFIX));
        fs::copy(std::env::current_exe()?, &executable)?;
        Ok((directory, executable))
    }

    fn code(result: Result<(), SelfUpdateError>) -> Option<&'static str> {
        result.err().map(|error| error.code())
    }

    #[test]
    fn the_compiled_key_is_the_placeholder_or_a_key() -> TestResult {
        // A key file that is neither would leave the hub unable to update
        // itself without saying so at build time.
        let compiled = compiled_key()?;
        let placeholder = COMPILED_KEY_FILE.lines().nth(1) == Some(PLACEHOLDER_KEY_LINE);
        assert_eq!(compiled.is_none(), placeholder);
        assert_eq!(SelfUpdateConfig::compiled().is_configured(), !placeholder);
        assert_eq!(SelfUpdateConfig::compiled().version(), env!("CARGO_PKG_VERSION"));
        Ok(())
    }

    #[test]
    fn the_placeholder_verifies_nothing() -> TestResult {
        let placeholder = "untrusted comment: PLACEHOLDER, NOT A KEY\nPLACEHOLDER-NOT-A-KEY\n";
        assert_eq!(read_key_file(placeholder)?, None);
        assert!(PublicKey::from_pub_file(HUB_REPO, placeholder.as_bytes()).is_err());
        let config = SelfUpdateConfig { key: None, executable: None, version: "1.0.0".into() };
        assert_eq!(
            config.key().err().map(|error| error.code()),
            Some("self_update_not_configured")
        );

        assert!(read_key_file(A_KEY_FILE)?.is_some());
        let broken = "untrusted comment: minisign public key\nRWRA+IGA6EB9Au\n";
        assert!(read_key_file(broken).is_err(), "neither the placeholder nor a key");
        Ok(())
    }

    #[test]
    fn a_swap_puts_the_new_version_in_place_and_keeps_the_old_one() -> TestResult {
        let (directory, executable) = copied_executable()?;
        let before = fs::read(&executable)?;
        let replacement = sibling(&executable, ".new");
        fs::write(&replacement, b"the new version")?;
        // Left by an earlier update that was never followed by a start.
        let old = sibling(&executable, ".old");
        fs::write(&old, b"an older version")?;

        swap(&executable, &replacement)?;
        assert_eq!(fs::read(&executable)?, b"the new version");
        assert_eq!(fs::read(&old)?, before, "the replaced executable is kept as .old");
        assert!(!replacement.exists());

        // The next start removes it.
        remove_old(&executable, Duration::ZERO);
        assert!(!old.exists());
        remove_old(&executable, Duration::ZERO);
        let mut names: Vec<_> = fs::read_dir(directory.path())?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<Result<_, _>>()?;
        names.sort();
        assert_eq!(names, [executable.file_name().unwrap_or_default().to_owned()]);
        Ok(())
    }

    #[test]
    fn a_failed_placement_restores_the_original() -> TestResult {
        let (_directory, executable) = copied_executable()?;
        let before = fs::read(&executable)?;
        let old = sibling(&executable, ".old");

        // The new version is gone when it is to take the executable's name:
        // the executable, renamed aside already, is renamed back.
        let replacement = sibling(&executable, ".new");
        assert_eq!(code(swap(&executable, &replacement)), Some("self_update_failed"));
        assert_eq!(fs::read(&executable)?, before);
        assert!(!old.exists() && !replacement.exists());

        // An `.old` that cannot be removed (here a directory) stops the swap
        // before anything is renamed, and the new version is removed.
        fs::create_dir(&old)?;
        fs::write(old.join("file"), b"")?;
        fs::write(&replacement, b"the new version")?;
        let error = swap(&executable, &replacement).err().ok_or("refused")?;
        assert_eq!(error.code(), "self_update_failed");
        assert!(error.to_string().contains("left as it was"), "{error}");
        assert_eq!(fs::read(&executable)?, before);
        assert!(!replacement.exists(), "the new version is removed");
        Ok(())
    }

    /// Windows: a file held open without sharing its deletion cannot be
    /// renamed, so a locked executable or new version fails the swap.
    #[cfg(windows)]
    #[test]
    fn a_locked_file_fails_the_swap_and_changes_nothing() -> TestResult {
        use std::os::windows::fs::OpenOptionsExt;
        let open_exclusively =
            |path: &Path| fs::OpenOptions::new().read(true).share_mode(0).open(path);
        let (_directory, executable) = copied_executable()?;
        let before = fs::read(&executable)?;
        let old = sibling(&executable, ".old");
        let replacement = sibling(&executable, ".new");

        // The executable is locked: it is not renamed.
        fs::write(&replacement, b"the new version")?;
        let locked = open_exclusively(&executable)?;
        assert_eq!(code(swap(&executable, &replacement)), Some("self_update_failed"));
        drop(locked);
        assert_eq!(fs::read(&executable)?, before);
        assert!(!old.exists() && !replacement.exists());

        // The new version is locked: the executable, renamed aside, is
        // renamed back.
        fs::write(&replacement, b"the new version")?;
        let locked = open_exclusively(&replacement)?;
        let error = swap(&executable, &replacement).err().ok_or("refused")?;
        drop(locked);
        assert_eq!(error.code(), "self_update_failed");
        assert!(error.to_string().contains("putting the new version in place"), "{error}");
        assert_eq!(fs::read(&executable)?, before);
        assert!(!old.exists());
        Ok(())
    }

    /// Windows: the replaced executable cannot be removed while a process
    /// still holds it; a starting hub tries again until it can.
    #[cfg(windows)]
    #[test]
    fn the_old_executable_is_removed_once_it_is_released() -> TestResult {
        use std::os::windows::fs::OpenOptionsExt;
        let (_directory, executable) = copied_executable()?;
        let old = sibling(&executable, ".old");
        fs::write(&old, b"the replaced version")?;
        let held = fs::OpenOptions::new().read(true).share_mode(0).open(&old)?;
        remove_old(&executable, Duration::ZERO);
        assert!(old.exists(), "held, and no patience");

        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            drop(held);
        });
        remove_old(&executable, Duration::from_secs(10));
        assert!(!old.exists());
        release.join().map_err(|_| "the thread panicked")?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn the_new_version_gets_the_permissions_of_the_executable() -> TestResult {
        use std::os::unix::fs::PermissionsExt;
        let (directory, executable) = copied_executable()?;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o751))?;
        let downloaded = directory.path().join("downloaded");
        fs::write(&downloaded, b"the new version")?;
        let replacement = sibling(&executable, ".new");
        copy_executable(&downloaded, &replacement, &executable)?;
        swap(&executable, &replacement)?;
        let mode = fs::metadata(&executable)?.permissions().mode() & 0o777;
        assert_eq!(mode, 0o751);
        Ok(())
    }

    /// Unix: the executable gets a second name before the new version is
    /// renamed over it, so its name never lacks an executable.
    #[cfg(unix)]
    #[test]
    fn the_executable_is_linked_aside_before_it_is_replaced() -> TestResult {
        use std::os::unix::fs::MetadataExt;
        let (_directory, executable) = copied_executable()?;
        let old = sibling(&executable, ".old");
        assert!(link_aside(&executable, &old), "a temporary directory takes hard links");
        let (running, kept) = (fs::metadata(&executable)?, fs::metadata(&old)?);
        assert_eq!((running.ino(), running.nlink()), (kept.ino(), 2), "one file, two names");
        Ok(())
    }

    #[test]
    fn a_folder_the_hub_cannot_write_to_is_refused_by_name() -> TestResult {
        let directory = tempfile::tempdir()?;
        let gone = directory.path().join("gone");
        let executable = gone.join("eario-hub.exe");
        let error = check_writable(&executable, &sibling(&executable, ".new")).err();
        let error = error.ok_or("refused")?;
        assert_eq!(error.code(), "self_update_not_writable");
        assert!(error.to_string().contains(&gone.display().to_string()), "{error}");

        // A writable folder passes, and is left as it was.
        let (directory, executable) = copied_executable()?;
        let replacement = sibling(&executable, ".new");
        fs::write(&replacement, b"left by an interrupted update")?;
        check_writable(&executable, &replacement)?;
        assert!(!replacement.exists());
        assert_eq!(fs::read_dir(directory.path())?.count(), 1);
        Ok(())
    }

    #[test]
    fn the_new_hub_is_started_alone_in_its_directory() -> TestResult {
        let (directory, executable) = copied_executable()?;
        let command = restart_command(&executable);
        assert_eq!(command.get_program(), executable.as_os_str());
        assert_eq!(command.get_args().count(), 0, "no arguments");
        assert_eq!(command.get_current_dir(), Some(directory.path()));
        assert_eq!(command.get_envs().count(), 0, "this hub's environment, unchanged");
        Ok(())
    }

    /// A configuration with a key, for `version`.
    fn configured(version: &str) -> Result<SelfUpdateConfig, VerifyError> {
        let key = read_key_file(A_KEY_FILE)?;
        Ok(SelfUpdateConfig { key, executable: None, version: version.to_owned() })
    }

    #[test]
    fn a_new_hub_that_does_not_start_is_recorded_in_the_hub_directory() -> TestResult {
        let directory = tempfile::tempdir()?;
        let hub = HubDir::at(directory.path().join("hub"));
        let missing = directory.path().join(format!("{HUB_SLUG}{}", std::env::consts::EXE_SUFFIX));
        let started = Instant::now();
        let error = restart(&hub, &missing).err().ok_or("a missing file does not start")?;
        assert_eq!(error.kind(), io::ErrorKind::NotFound, "{error}");
        assert!(started.elapsed() < Duration::from_secs(5), "not busy, so not tried again");
        let log = fs::read_to_string(hub.self_update_log_path())?;
        assert_eq!(log.lines().count(), 1, "{log}");
        assert!(log.contains(&missing.display().to_string()), "{log}");
        assert!(log.contains("The new version is in place"), "{log}");

        restart(&hub, &missing).err().ok_or("a missing file does not start")?;
        let log = fs::read_to_string(hub.self_update_log_path())?;
        assert_eq!(log.lines().count(), 2, "appended: {log}");
        Ok(())
    }

    /// Windows: a new hub that another program holds for a moment, as an
    /// antivirus scanning a new file does, starts once it is released.
    #[cfg(windows)]
    #[test]
    fn a_new_hub_held_for_a_moment_still_starts() -> TestResult {
        use std::os::windows::fs::OpenOptionsExt;
        // A program that exits at once without arguments stands in for the
        // new hub: a copy of this test executable would run every test.
        let system = std::env::var_os("SystemRoot").ok_or("SystemRoot is not set")?;
        let directory = tempfile::tempdir()?;
        let executable = directory.path().join(format!("{HUB_SLUG}.exe"));
        fs::copy(Path::new(&system).join("System32").join("hostname.exe"), &executable)?;
        let hub = HubDir::at(directory.path().join("hub"));
        let held = fs::OpenOptions::new().read(true).share_mode(0).open(&executable)?;
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            drop(held);
        });
        let started = Instant::now();
        let mut child = restart(&hub, &executable)?;
        release.join().map_err(|_| "the thread panicked")?;
        assert!(started.elapsed() >= Duration::from_millis(250), "it waited for the file");
        assert!(child.wait()?.success());
        assert!(!hub.self_update_log_path().exists(), "nothing to record");
        Ok(())
    }

    #[test]
    fn a_check_reports_a_newer_hub_only_while_it_is_newer() -> TestResult {
        let check = |available: Option<&str>| UpdateCheck {
            checked_at: 1,
            dashboards: Vec::new(),
            hub: Some(HubUpdate {
                repo: HUB_REPO.to_owned(),
                latest_version: available.map(str::to_owned),
                available_version: available.map(str::to_owned),
                error: None,
            }),
        };
        let running = configured("0.2.0")?;
        assert_eq!(available_version(Some(&check(Some("0.3.0"))), &running), Some("0.3.0".into()));
        let updated = configured("0.3.0")?;
        assert_eq!(available_version(Some(&check(Some("0.3.0"))), &updated), None, "updated since");
        assert_eq!(available_version(Some(&check(None)), &running), None);
        let older = UpdateCheck { hub: None, ..check(None) };
        assert_eq!(available_version(Some(&older), &running), None);
        assert_eq!(available_version(None, &running), None);

        // A hub without the key offers nothing, whatever a check found.
        let keyless = SelfUpdateConfig { key: None, ..running };
        assert_eq!(available_version(Some(&check(Some("0.3.0"))), &keyless), None);
        Ok(())
    }

    #[test]
    fn a_hub_without_a_key_checks_nothing() {
        let config = SelfUpdateConfig { key: None, executable: None, version: "0.2.0".into() };
        let found = check(&config, || Err(HubError::Task("no request may be made".into())));
        let error = found.error.map(|error| error.code);
        assert_eq!(error.as_deref(), Some("self_update_not_configured"));
        assert_eq!(found.available_version, None);
    }
}
