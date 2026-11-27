//! Checks a GitHub release before anything from it is installed (ADR 0013).
//!
//! A release carries one executable per target, named by [`asset_name`], and
//! a [`SUMS_FILE`] listing their SHA-256, signed with minisign in
//! [`SIGNATURE_FILE`]; it may carry the signer's public key in [`KEY_FILE`].
//! [`verify_sums`] checks that signature against a [`PublicKey`] and only
//! then reads the list, so a [`Checksums`] exists only for a list the key
//! signed. [`key_status`] says how a key relates to the one pinned for the
//! repository. A key is identified by [`PublicKey::sha256`], never by its
//! minisign key id.
//!
//! Nothing here touches the network or the disk: callers download the files,
//! then compare the SHA-256 of the asset with [`Checksums::sha256`]. Every
//! malformed input is refused; nothing is repaired or guessed.

use std::collections::BTreeMap;

use eario_core::{KeyStatus, is_slug};
use sha2::{Digest, Sha256};

/// Name of the checksum list in a release.
pub const SUMS_FILE: &str = "SHA256SUMS.txt";

/// Name of the minisign signature of [`SUMS_FILE`] in a release.
pub const SIGNATURE_FILE: &str = "SHA256SUMS.txt.minisig";

/// Name of the minisign public key file in a release: the `.pub` file
/// minisign writes. What a repository that no catalog lists is trusted with
/// on first use, once the user has accepted it.
pub const KEY_FILE: &str = "minisign.pub";

/// Largest [`KEY_FILE`] read, in bytes. A minisign `.pub` file takes about
/// 100.
pub const MAX_KEY_FILE_BYTES: usize = 4 * 1024;

/// Largest [`SUMS_FILE`] read, in bytes. The list of four assets takes about
/// 400.
pub const MAX_SUMS_BYTES: usize = 64 * 1024;

/// Largest [`SIGNATURE_FILE`] read, in bytes. A minisign signature takes
/// about 300 plus its trusted comment.
pub const MAX_SIGNATURE_BYTES: usize = 16 * 1024;

/// Bytes of a minisign public key: algorithm (2), key id (8), Ed25519 key
/// (32).
const KEY_BYTES: usize = 42;

/// What starts the first line of a minisign signature file.
const UNTRUSTED_COMMENT: &str = "untrusted comment: ";

/// Why a release, or a key for it, was refused. Every variant names the
/// repository and, where there is one, the file.
#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    /// The text given as a key is not a minisign public key.
    #[error(
        "the public key given for `{repo}` is not a minisign public key; give the key line of its minisign `.pub` file: 56 base64 characters starting with `RW`, with nothing around them"
    )]
    InvalidKey {
        /// The repository the key is for.
        repo: String,
    },
    /// The release's key file is not a minisign public key file.
    #[error(
        "`minisign.pub` of `{repo}` is not a minisign public key file (an untrusted comment line, then the key line); its author must attach the `.pub` file minisign wrote"
    )]
    InvalidKeyFile {
        /// The repository.
        repo: String,
    },
    /// Nothing tells which key the release must be signed with.
    #[error(
        "no catalog lists a key for `{repo}` and its release has no `minisign.pub`, so nothing can verify it; ask its author to attach the public key to the release, or add a catalog that lists the repository"
    )]
    MissingKey {
        /// The repository.
        repo: String,
    },
    /// A downloaded asset does not have the SHA-256 the signed list gives.
    #[error(
        "the downloaded `{asset}` of `{repo}` does not have the SHA-256 its signed `SHA256SUMS.txt` lists: it was changed or damaged on the way; nothing was installed"
    )]
    HashMismatch {
        /// The repository.
        repo: String,
        /// The asset.
        asset: String,
    },
    /// The release has no signature of its checksum list.
    #[error(
        "the release of `{repo}` has no `SHA256SUMS.txt.minisig`, so its `SHA256SUMS.txt` cannot be checked; its author must sign the checksum list with minisign"
    )]
    MissingSignature {
        /// The repository.
        repo: String,
    },
    /// The signature file is not a minisign signature.
    #[error(
        "`SHA256SUMS.txt.minisig` of `{repo}` is not a minisign signature (an untrusted comment, the signature, a trusted comment and a global signature, one per line); its author must sign `SHA256SUMS.txt` again"
    )]
    MalformedSignature {
        /// The repository.
        repo: String,
    },
    /// The signature is of a kind this hub refuses: legacy (not pre-hashed)
    /// or an unknown algorithm.
    #[error(
        "`SHA256SUMS.txt.minisig` of `{repo}` is a legacy or unknown kind of minisign signature; its author must sign `SHA256SUMS.txt` again with a current minisign, which pre-hashes the file"
    )]
    UnsupportedSignature {
        /// The repository.
        repo: String,
    },
    /// The signature names another key than the one it was checked with.
    #[error(
        "`SHA256SUMS.txt` of `{repo}` was not signed with the key trusted for it (fingerprint {fingerprint}); if its author changed keys, the new key must be accepted before installing from this release"
    )]
    WrongKey {
        /// The repository.
        repo: String,
        /// Fingerprint of the key the signature was checked with.
        fingerprint: String,
    },
    /// The signature does not match the checksum list.
    #[error(
        "the signature of `SHA256SUMS.txt` of `{repo}` does not verify: the file or its signature was changed after signing; nothing from this release can be installed"
    )]
    BadSignature {
        /// The repository.
        repo: String,
    },
    /// A file is larger than a release may have.
    #[error("`{file}` of `{repo}` has {size} bytes, more than the {limit} a release may have")]
    TooLarge {
        /// The repository.
        repo: String,
        /// The file, [`SUMS_FILE`] or [`SIGNATURE_FILE`].
        file: &'static str,
        /// Its size in bytes.
        size: usize,
        /// The largest size accepted.
        limit: usize,
    },
    /// A line of the checksum list is not a checksum line.
    #[error(
        "line {line} of `SHA256SUMS.txt` of `{repo}` is not `<64 hexadecimal digits>  <asset name>` as `sha256sum` writes it; asset names have ASCII letters, digits, `.`, `_`, `-` and `+` only"
    )]
    MalformedSums {
        /// The repository.
        repo: String,
        /// Number of the line, from 1.
        line: usize,
    },
    /// The checksum list names an asset twice.
    #[error("line {line} of `SHA256SUMS.txt` of `{repo}` lists `{asset}` a second time")]
    DuplicateAsset {
        /// The repository.
        repo: String,
        /// Number of the second line naming it, from 1.
        line: usize,
        /// The asset.
        asset: String,
    },
    /// The checksum list names no asset.
    #[error("`SHA256SUMS.txt` of `{repo}` lists no asset")]
    EmptySums {
        /// The repository.
        repo: String,
    },
    /// The signed checksum list does not name the asset asked for.
    #[error(
        "`SHA256SUMS.txt` of `{repo}` does not list `{asset}`: this release has no signed asset of that name"
    )]
    AssetNotListed {
        /// The repository.
        repo: String,
        /// The asset asked for.
        asset: String,
    },
    /// An asset name was asked for something that is not a slug.
    #[error(
        "`{0}` is not a dashboard identifier; identifiers have lower-case letters, digits and dashes only"
    )]
    InvalidSlug(String),
}

impl VerifyError {
    /// Stable, machine-readable code of the error.
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidKey { .. } => "release_invalid_key",
            Self::InvalidKeyFile { .. } => "release_invalid_key_file",
            Self::MissingKey { .. } => "release_missing_key",
            Self::HashMismatch { .. } => "release_hash_mismatch",
            Self::MissingSignature { .. } => "release_missing_signature",
            Self::MalformedSignature { .. } => "release_malformed_signature",
            Self::UnsupportedSignature { .. } => "release_unsupported_signature",
            Self::WrongKey { .. } => "release_wrong_key",
            Self::BadSignature { .. } => "release_bad_signature",
            Self::TooLarge { .. } => "release_file_too_large",
            Self::MalformedSums { .. } => "release_malformed_sums",
            Self::DuplicateAsset { .. } => "release_duplicate_asset",
            Self::EmptySums { .. } => "release_empty_sums",
            Self::AssetNotListed { .. } => "release_asset_not_listed",
            Self::InvalidSlug(_) => "hub_invalid_slug",
        }
    }
}

/// A minisign public key: the key a release's checksum list must be signed
/// with.
///
/// Two keys are equal when their algorithm, key id and Ed25519 key all are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicKey {
    key: minisign_verify::PublicKey,
    text: String,
    fingerprint: String,
}

impl PublicKey {
    /// Reads the key line of a minisign `.pub` file (its second line: 56
    /// base64 characters starting with `RW`), as catalogs and the registry
    /// store it. `repo` is the repository the key is for; it only names it in
    /// errors.
    ///
    /// Nothing around the key is accepted: no comment line, space or newline.
    pub fn from_base64(repo: &str, text: &str) -> Result<Self, VerifyError> {
        let invalid = || VerifyError::InvalidKey { repo: repo.to_owned() };
        let bytes = decode_key_line(text).ok_or_else(invalid)?;
        // minisign public keys name the algorithm `Ed`; minisign-verify also
        // takes `ED`, which only signatures use.
        if bytes[..2] != *b"Ed" {
            return Err(invalid());
        }
        // Every decoding failure means the same to the user: this is not a key.
        let key = minisign_verify::PublicKey::from_base64(text).map_err(|_| invalid())?;
        // minisign shows the key id (bytes 2 to 9) as a little-endian number.
        let fingerprint = bytes[2..10].iter().rev().map(|byte| format!("{byte:02X}")).collect();
        Ok(Self { key, text: text.to_owned(), fingerprint })
    }

    /// Reads a minisign `.pub` file, as a release carries it in
    /// [`KEY_FILE`]: an untrusted comment line, then the key line (see
    /// [`PublicKey::from_base64`]), each ended by `\n` or `\r\n`, the last one
    /// optionally. Nothing else is accepted.
    pub fn from_pub_file(repo: &str, bytes: &[u8]) -> Result<Self, VerifyError> {
        let invalid = || VerifyError::InvalidKeyFile { repo: repo.to_owned() };
        check_size(repo, KEY_FILE, bytes.len(), MAX_KEY_FILE_BYTES)?;
        let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
        let body = text.strip_suffix('\n').unwrap_or(text);
        let mut lines = body.split('\n').map(|line| line.strip_suffix('\r').unwrap_or(line));
        let (Some(comment), Some(key), None) = (lines.next(), lines.next(), lines.next()) else {
            return Err(invalid());
        };
        if !comment.starts_with(UNTRUSTED_COMMENT) {
            return Err(invalid());
        }
        Self::from_base64(repo, key).map_err(|_| invalid())
    }

    /// The key line, as read: what the registry stores as the pinned key.
    pub fn as_base64(&self) -> &str {
        &self.text
    }

    /// SHA-256 of the key line, in lower-case hexadecimal: what identifies the
    /// key when the user accepts it. Two keys have the same hash only if they
    /// are the same key.
    pub fn sha256(&self) -> String {
        sha256_hex(self.text.as_bytes())
    }

    /// Fingerprint of the key, for the user to compare: its minisign key id
    /// in 16 upper-case hexadecimal digits, as minisign prints it
    /// (`minisign public key: 027D40E88081F840`).
    ///
    /// A key id names a key but does not prove it: anyone can make a key with
    /// the same id. Trust decisions compare whole keys, as [`key_status`]
    /// does, never fingerprints.
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
}

/// SHA-256 of `bytes` in lower-case hexadecimal.
fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Whether `text` is a SHA-256 in lower-case hexadecimal: 64 digits `0-9a-f`,
/// as the hub writes them and asks them back.
pub fn is_sha256(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// How `key`, the key a release would be verified with, relates to what the
/// hub trusts for its repository: `pinned`, the key of its registry entry,
/// and `catalog`, the key of its catalog entry.
///
/// A pinned key decides when there is one: the same key is
/// [`KeyStatus::Pinned`], another is [`KeyStatus::Changed`], whatever the
/// catalog says. Without one, a catalog key decides the same way
/// ([`KeyStatus::Catalog`] or `Changed`). With neither, the key is
/// [`KeyStatus::Unknown`]: trusted on first use, once the user has confirmed
/// its fingerprint. Keys are compared whole, not by fingerprint.
pub fn key_status(
    key: &PublicKey,
    pinned: Option<&PublicKey>,
    catalog: Option<&PublicKey>,
) -> KeyStatus {
    match (pinned, catalog) {
        (Some(pinned), _) if pinned == key => KeyStatus::Pinned,
        (Some(_), _) => KeyStatus::Changed,
        (None, Some(catalog)) if catalog == key => KeyStatus::Catalog,
        (None, Some(_)) => KeyStatus::Changed,
        (None, None) => KeyStatus::Unknown,
    }
}

/// The checksum list of a release, read from a [`SUMS_FILE`] whose
/// signature verified. Only [`verify_sums`] makes one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checksums {
    repo: String,
    /// SHA-256 in lower-case hexadecimal, by asset name.
    entries: BTreeMap<String, String>,
}

impl Checksums {
    /// SHA-256 the signed list gives for `asset`, in lower-case hexadecimal;
    /// refused when the list does not name it.
    pub fn sha256(&self, asset: &str) -> Result<&str, VerifyError> {
        self.entries.get(asset).map(String::as_str).ok_or_else(|| VerifyError::AssetNotListed {
            repo: self.repo.clone(),
            asset: asset.to_owned(),
        })
    }
}

/// Verifies the signature of a release's checksum list, then reads the list.
///
/// `sums` is the content of [`SUMS_FILE`]; `signature` that of
/// [`SIGNATURE_FILE`], or `None` when the release has none; `key` the key the
/// list must be signed with. `repo` only names the repository in errors.
///
/// Refused: a missing, malformed, legacy or foreign signature; a list or
/// signature changed after signing; a file over its size limit; a list with a
/// malformed line, an asset listed twice, or no asset. The list is read only
/// once its signature has verified.
pub fn verify_sums(
    repo: &str,
    key: &PublicKey,
    sums: &[u8],
    signature: Option<&[u8]>,
) -> Result<Checksums, VerifyError> {
    let Some(signature) = signature else {
        return Err(VerifyError::MissingSignature { repo: repo.to_owned() });
    };
    check_size(repo, SUMS_FILE, sums.len(), MAX_SUMS_BYTES)?;
    check_size(repo, SIGNATURE_FILE, signature.len(), MAX_SIGNATURE_BYTES)?;
    let signature = decode_signature(repo, key, signature)?;
    // Legacy signatures (not pre-hashed) are refused.
    key.key.verify(sums, &signature, false).map_err(|error| signature_error(repo, key, error))?;
    parse_sums(repo, sums)
}

fn check_size(
    repo: &str,
    file: &'static str,
    size: usize,
    limit: usize,
) -> Result<(), VerifyError> {
    if size > limit {
        return Err(VerifyError::TooLarge { repo: repo.to_owned(), file, size, limit });
    }
    Ok(())
}

/// Reads a minisign signature file: exactly four lines, the first an
/// untrusted comment.
fn decode_signature(
    repo: &str,
    key: &PublicKey,
    bytes: &[u8],
) -> Result<minisign_verify::Signature, VerifyError> {
    let malformed = || VerifyError::MalformedSignature { repo: repo.to_owned() };
    // A signature that is not UTF-8 is malformed, whatever the position.
    let text = std::str::from_utf8(bytes).map_err(|_| malformed())?;
    if text.lines().count() != 4 || !text.starts_with(UNTRUSTED_COMMENT) {
        return Err(malformed());
    }
    minisign_verify::Signature::decode(text).map_err(|error| signature_error(repo, key, error))
}

fn signature_error(repo: &str, key: &PublicKey, error: minisign_verify::Error) -> VerifyError {
    use minisign_verify::Error;

    let repo = repo.to_owned();
    match error {
        Error::InvalidEncoding => VerifyError::MalformedSignature { repo },
        Error::UnexpectedAlgorithm | Error::UnsupportedAlgorithm | Error::UnsupportedLegacyMode => {
            VerifyError::UnsupportedSignature { repo }
        }
        Error::UnexpectedKeyId => {
            VerifyError::WrongKey { repo, fingerprint: key.fingerprint.clone() }
        }
        // Verifying bytes in memory reads no file, so an I/O error cannot
        // happen; were it to, the release is refused all the same.
        Error::InvalidSignature | Error::IoError(_) => VerifyError::BadSignature { repo },
    }
}

/// Reads a checksum list as `sha256sum` writes it: one
/// `<64 hexadecimal digits>  <asset>` (text mode) or
/// `<64 hexadecimal digits> *<asset>` (binary mode) per line, lines ended by
/// `\n` or `\r\n`, the last one optionally. Any other line, an empty one
/// included, refuses the whole list.
fn parse_sums(repo: &str, sums: &[u8]) -> Result<Checksums, VerifyError> {
    let body = sums.strip_suffix(b"\n").unwrap_or(sums);
    if body.is_empty() {
        return Err(VerifyError::EmptySums { repo: repo.to_owned() });
    }
    let mut entries = BTreeMap::new();
    for (index, line) in body.split(|&byte| byte == b'\n').enumerate() {
        let line_number = index + 1;
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some((sha256, asset)) = parse_line(line) else {
            return Err(VerifyError::MalformedSums { repo: repo.to_owned(), line: line_number });
        };
        if entries.insert(asset.to_owned(), sha256).is_some() {
            return Err(VerifyError::DuplicateAsset {
                repo: repo.to_owned(),
                line: line_number,
                asset: asset.to_owned(),
            });
        }
    }
    Ok(Checksums { repo: repo.to_owned(), entries })
}

/// The SHA-256, in lower case, and the asset name of one checksum line.
fn parse_line(line: &[u8]) -> Option<(String, &str)> {
    let line = std::str::from_utf8(line).ok()?;
    let (sha256, rest) = line.split_at_checked(64)?;
    let asset = rest.strip_prefix("  ").or_else(|| rest.strip_prefix(" *"))?;
    let valid = sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) && is_asset_name(asset);
    valid.then(|| (sha256.to_ascii_lowercase(), asset))
}

/// Whether `name` can name a release asset: ASCII letters, digits, `.`, `_`,
/// `-` and `+`, and neither `.` nor `..`.
pub(crate) fn is_asset_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+'))
}

/// Decodes the 56 base64 characters of a minisign key line into its 42
/// bytes; `None` for any other length or character. 42 bytes fill 56
/// characters exactly, so the key line has no padding.
fn decode_key_line(text: &str) -> Option<[u8; KEY_BYTES]> {
    // Four characters carry three bytes.
    let (groups, rest) = text.as_bytes().as_chunks::<4>();
    if groups.len() != KEY_BYTES / 3 || !rest.is_empty() {
        return None;
    }
    let mut bytes = [0u8; KEY_BYTES];
    for (group, decoded) in groups.iter().zip(bytes.as_chunks_mut::<3>().0) {
        let mut word = 0u32;
        for &character in group {
            word = (word << 6) | base64_value(character)?;
        }
        let [_, first, second, third] = word.to_be_bytes();
        *decoded = [first, second, third];
    }
    Some(bytes)
}

/// Value of a character of standard base64.
fn base64_value(character: u8) -> Option<u32> {
    let value = match character {
        b'A'..=b'Z' => character - b'A',
        b'a'..=b'z' => character - b'a' + 26,
        b'0'..=b'9' => character - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => return None,
    };
    Some(u32::from(value))
}

/// A platform releases publish executables for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// 64-bit Windows: `x86_64-pc-windows-msvc`.
    WindowsX86_64,
    /// 64-bit Linux with the GNU C library: `x86_64-unknown-linux-gnu`.
    LinuxX86_64,
    /// macOS on Apple silicon: `aarch64-apple-darwin`.
    MacosAarch64,
    /// macOS on Intel: `x86_64-apple-darwin`.
    MacosX86_64,
}

impl Target {
    /// Every target.
    pub const ALL: [Target; 4] =
        [Target::WindowsX86_64, Target::LinuxX86_64, Target::MacosAarch64, Target::MacosX86_64];

    /// The target this executable was built for, decided at compile time;
    /// `None` on any other platform, where nothing can be installed from a
    /// release.
    pub const CURRENT: Option<Target> =
        if cfg!(all(target_os = "windows", target_arch = "x86_64", target_env = "msvc")) {
            Some(Target::WindowsX86_64)
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu")) {
            Some(Target::LinuxX86_64)
        } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            Some(Target::MacosAarch64)
        } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
            Some(Target::MacosX86_64)
        } else {
            None
        };

    /// The target triple, for example `x86_64-pc-windows-msvc`.
    pub const fn triple(self) -> &'static str {
        match self {
            Target::WindowsX86_64 => "x86_64-pc-windows-msvc",
            Target::LinuxX86_64 => "x86_64-unknown-linux-gnu",
            Target::MacosAarch64 => "aarch64-apple-darwin",
            Target::MacosX86_64 => "x86_64-apple-darwin",
        }
    }

    /// What ends the name of an executable for the target: `.exe` on
    /// Windows, nothing elsewhere.
    pub const fn exe_suffix(self) -> &'static str {
        match self {
            Target::WindowsX86_64 => ".exe",
            Target::LinuxX86_64 | Target::MacosAarch64 | Target::MacosX86_64 => "",
        }
    }
}

/// Name of the release asset of dashboard `slug` for `target`:
/// `<slug>-<triple>`, with `.exe` on Windows, for example
/// `signal-lab-x86_64-pc-windows-msvc.exe`.
///
/// Refused when `slug` is not a slug, so the name is always a plain file
/// name.
pub fn asset_name(slug: &str, target: Target) -> Result<String, VerifyError> {
    if !is_slug(slug) {
        return Err(VerifyError::InvalidSlug(slug.to_owned()));
    }
    Ok(format!("{slug}-{}{}", target.triple(), target.exe_suffix()))
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    const REPO: &str = "lab/signal-lab";

    // Synthetic vectors, made once with the `minisign` crate 0.10.0 from two
    // fresh key pairs whose secret keys were never stored (session
    // 2026-11-27__dashboard-launcher, ticket 02). Key A signed `SUMS`.
    const KEY_A: &str = "RWRA+IGA6EB9AuIbrrq9pGIpkX+AusRGz1XiVhgRrTfRER9SJa+jD9+x";
    const KEY_B: &str = "RWQrHdp6bVYNJfouycVFGpYdTZQx74XTWGu6GVziED4qg9k3cewojLTb";
    /// Key A's algorithm and key id with key B's Ed25519 key: the
    /// fingerprint of A, another key.
    const KEY_A_ID_B_KEY: &str = "RWRA+IGA6EB9AvouycVFGpYdTZQx74XTWGu6GVziED4qg9k3cewojLTb";
    const SUMS: &str = "\
0f4a6b2c1d3e5f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f8  signal-lab-x86_64-pc-windows-msvc.exe
1e5b7c3d2e4f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f9  signal-lab-x86_64-unknown-linux-gnu
2d6c8d4e3f5061728394a5b6c7d8e9fa0b1c2d3e4f5061728394a5b6c7d8e9fa  signal-lab-aarch64-apple-darwin
3c7d9e5f40617283940a5b6c7d8e9f0a1b2c3d4e5f60718293a4b5c6d7e8f9a0  signal-lab-x86_64-apple-darwin
";
    const SIGNATURE: &str = "\
untrusted comment: signature from minisign secret key
RURA+IGA6EB9AiJtgvVZtLL21DWhHik/tuT7uBHuzN4Qn3bHUYOzqhXVEU19pWthWYjYYXMXF9qxhDbf3qtltFzLalZfWadzIQ8=
trusted comment: timestamp:1764230400\tfile:SHA256SUMS.txt\thashed
CuqdxfXnaMu9rHBgfkKv4KUt4idYL9sYvfIJTcGt2OvkIOBGRU7P35WfqvTmQNHqHg5YHh6AjpJuU8emBLy4AA==
";

    // Signatures of the four bytes `test` in the minisign tool's format,
    // from the tests of the minisign-verify crate 0.3.0: one pre-hashed, one
    // legacy.
    const TOOL_KEY: &str = "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
    const TOOL_PREHASHED: &str = "\
untrusted comment: signature from minisign secret key
RUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=
trusted comment: timestamp:1556193335\tfile:test
y/rUw2y8/hOUYjZU71eHp/Wo1KZ40fGy2VJEDl34XMJM+TX48Ss/17u3IvIfbVR1FkZZSNCisQbuQY+bHwhEBg==
";
    const TOOL_LEGACY: &str = "\
untrusted comment: signature from minisign secret key
RWQf6LRCGA9i59SLOFxz6NxvASXDJeRtuZykwQepbDEGt87ig1BNpWaVWuNrm73YiIiJbq71Wi+dP9eKL8OC351vwIasSSbXxwA=
trusted comment: timestamp:1555779966\tfile:test
QtKMXWyYcwdpZAlPF7tE2ENJkRd1ujvKjlj1m9RtHTBnZPa5WKU5uWRs5GoP5M/VqE81QFuMKI5k/SfNQUaOAA==
";

    fn key(text: &str) -> Result<PublicKey, VerifyError> {
        PublicKey::from_base64(REPO, text)
    }

    /// Verifies `sums` with key A and `signature`.
    fn verify(sums: &[u8], signature: &str) -> Result<Checksums, VerifyError> {
        verify_sums(REPO, &key(KEY_A)?, sums, Some(signature.as_bytes()))
    }

    /// Code of the refusal, `None` when `result` was accepted.
    fn refusal<T>(result: Result<T, VerifyError>) -> Option<&'static str> {
        result.err().map(|error| error.code())
    }

    #[test]
    fn a_signed_list_gives_the_hash_of_each_asset() -> TestResult {
        let sums = verify(SUMS.as_bytes(), SIGNATURE)?;
        assert_eq!(
            sums.sha256("signal-lab-x86_64-pc-windows-msvc.exe")?,
            "0f4a6b2c1d3e5f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f8"
        );
        for target in Target::ALL {
            let sha256 = sums.sha256(&asset_name("signal-lab", target)?)?;
            assert_eq!(sha256.len(), 64, "{target:?}");
        }

        let error = sums.sha256("signal-lab-riscv64gc-unknown-linux-gnu").err();
        let error = error.ok_or("an asset the list does not name must be refused")?;
        assert_eq!(error.code(), "release_asset_not_listed");
        let message = error.to_string();
        assert!(message.contains(REPO) && message.contains("riscv64gc"), "{message}");
        Ok(())
    }

    #[test]
    fn signatures_made_by_the_minisign_tool_verify() -> TestResult {
        let key = key(TOOL_KEY)?;
        let signature = Some(TOOL_PREHASHED.as_bytes());
        // The signature verifies, so the content is read: `test` is no list.
        let read = verify_sums(REPO, &key, b"test", signature);
        assert_eq!(refusal(read), Some("release_malformed_sums"));
        assert_eq!(
            refusal(verify_sums(REPO, &key, b"Test", signature)),
            Some("release_bad_signature")
        );
        Ok(())
    }

    #[test]
    fn a_list_signed_with_another_key_is_refused() -> TestResult {
        let other = key(KEY_B)?;
        let error = verify_sums(REPO, &other, SUMS.as_bytes(), Some(SIGNATURE.as_bytes())).err();
        let error = error.ok_or("a signature by another key must be refused")?;
        assert_eq!(error.code(), "release_wrong_key");
        let message = error.to_string();
        assert!(message.contains(REPO) && message.contains(SUMS_FILE), "{message}");
        assert!(message.contains("250D566D7ADA1D2B"), "{message}");
        Ok(())
    }

    #[test]
    fn a_key_with_the_same_fingerprint_is_still_another_key() -> TestResult {
        let (genuine, impostor) = (key(KEY_A)?, key(KEY_A_ID_B_KEY)?);
        assert_eq!(impostor.fingerprint(), genuine.fingerprint());
        assert_ne!(impostor, genuine);
        let read = verify_sums(REPO, &impostor, SUMS.as_bytes(), Some(SIGNATURE.as_bytes()));
        assert_eq!(refusal(read), Some("release_bad_signature"));
        assert_eq!(key_status(&impostor, Some(&genuine), None), KeyStatus::Changed);
        Ok(())
    }

    #[test]
    fn a_changed_list_is_refused() {
        let changed = [
            SUMS.replacen("0f4a", "0f4b", 1),
            SUMS.replacen("windows-msvc.exe", "windows-msvc.exf", 1),
            SUMS.replace('\n', "\r\n"),
            SUMS.trim_end().to_owned(),
            format!("{SUMS}{}  evil.exe\n", "ab".repeat(32)),
            SUMS.lines().take(3).map(|line| format!("{line}\n")).collect(),
        ];
        for sums in changed {
            assert_eq!(refusal(verify(sums.as_bytes(), SIGNATURE)), Some("release_bad_signature"));
        }
    }

    #[test]
    fn a_changed_signature_or_trusted_comment_is_refused() {
        let changed = [
            SIGNATURE.replacen("timestamp:1764230400", "timestamp:1764230401", 1),
            SIGNATURE.replacen("file:SHA256SUMS.txt", "file:OTHER.txt", 1),
            // Inside the Ed25519 signature, after the algorithm and key id.
            SIGNATURE.replacen("tuT7uBHu", "tuT7uBHv", 1),
            // Inside the global signature.
            SIGNATURE.replacen("CuqdxfXn", "CuqdxfXm", 1),
        ];
        for signature in changed {
            let read = verify(SUMS.as_bytes(), &signature);
            assert_eq!(refusal(read), Some("release_bad_signature"), "{signature}");
        }
    }

    #[test]
    fn a_missing_signature_is_refused() -> TestResult {
        let error = verify_sums(REPO, &key(KEY_A)?, SUMS.as_bytes(), None).err();
        let error = error.ok_or("a list without signature must be refused")?;
        assert_eq!(error.code(), "release_missing_signature");
        let message = error.to_string();
        assert!(message.contains(REPO) && message.contains(SIGNATURE_FILE), "{message}");
        Ok(())
    }

    #[test]
    fn a_malformed_signature_is_refused() -> TestResult {
        let lines: Vec<&str> = SIGNATURE.lines().collect();
        let malformed = [
            String::new(),
            "not a signature".to_owned(),
            lines[..3].join("\n"),
            format!("{SIGNATURE}more\n"),
            format!("{SIGNATURE}\n"),
            SIGNATURE.replacen("untrusted comment: ", "comment: ", 1),
            SIGNATURE.replacen("trusted comment: timestamp", "timestamp", 1),
            SIGNATURE.replacen("RURA+IGA", "RURA IGA", 1),
            SIGNATURE.replacen("CuqdxfXn", "Cuqdx", 1),
        ];
        for signature in malformed {
            let read = verify(SUMS.as_bytes(), &signature);
            assert_eq!(refusal(read), Some("release_malformed_signature"), "{signature:?}");
        }
        let not_utf8 = [SIGNATURE.as_bytes(), &[0xff]].concat();
        let read = verify_sums(REPO, &key(KEY_A)?, SUMS.as_bytes(), Some(&not_utf8));
        assert_eq!(refusal(read), Some("release_malformed_signature"));
        Ok(())
    }

    #[test]
    fn legacy_and_unknown_signature_algorithms_are_refused() -> TestResult {
        let tool = verify_sums(REPO, &key(TOOL_KEY)?, b"test", Some(TOOL_LEGACY.as_bytes()));
        assert_eq!(refusal(tool), Some("release_unsupported_signature"));
        // `RW` encodes the legacy algorithm `Ed`, `QU` the unknown `AD`; the
        // key id is unchanged.
        for prefix in ["RWRA+IGA", "QURA+IGA"] {
            let signature = SIGNATURE.replacen("RURA+IGA", prefix, 1);
            let read = verify(SUMS.as_bytes(), &signature);
            assert_eq!(refusal(read), Some("release_unsupported_signature"), "{prefix}");
        }
        Ok(())
    }

    #[test]
    fn oversized_files_are_refused() -> TestResult {
        let large = vec![b'a'; MAX_SUMS_BYTES + 1];
        let error = verify(&large, SIGNATURE).err().ok_or("an oversized list must be refused")?;
        assert_eq!(error.code(), "release_file_too_large");
        assert!(error.to_string().contains(SUMS_FILE), "{error}");

        let large = format!("{SIGNATURE}{}", " ".repeat(MAX_SIGNATURE_BYTES));
        let error = verify(SUMS.as_bytes(), &large).err();
        let error = error.ok_or("an oversized signature must be refused")?;
        assert_eq!(error.code(), "release_file_too_large");
        assert!(error.to_string().contains(SIGNATURE_FILE), "{error}");
        Ok(())
    }

    #[test]
    fn checksum_lines_of_text_and_binary_mode_are_read() -> TestResult {
        let hash = "ab".repeat(32);
        let upper = hash.to_ascii_uppercase();
        let text = format!("{hash}  a.exe\n{upper} *b_1+x\r\n{hash}  c-2.0.tar.gz");
        let sums = parse_sums(REPO, text.as_bytes())?;
        for asset in ["a.exe", "b_1+x", "c-2.0.tar.gz"] {
            assert_eq!(sums.sha256(asset)?, hash);
        }
        Ok(())
    }

    #[test]
    fn the_list_the_release_workflow_writes_and_signs_is_read() -> TestResult {
        // What `sha256sum --text -- <assets>` printed for these four files,
        // named by `asset_name`, as `.github/workflows/release-dashboard.yml`
        // runs it (GNU coreutils, names in `LC_ALL=C sort` order), and its
        // signature by `minisign -S` 0.12 with a throwaway key whose secret
        // key was deleted after signing (session
        // 2026-11-27__dashboard-launcher, ticket 08).
        let sums = "\
605c2b64fa277b8682bf44b0cc88def63b264f72885aa66c7431dae3c9514d38  signal-lab-aarch64-apple-darwin
caa03297f5e443c638ff1dc72dabe74daf400a27d285582b647dd3e68e034368  signal-lab-x86_64-apple-darwin
51275f0d1a161e1a1bfe2f226348d4227fe90cc9366492b13bdbb04501700a1a  signal-lab-x86_64-pc-windows-msvc.exe
b0a2f490e5e67e7963c10e2dfe53735beb83a83a115b56b61d615e144b0ac5c1  signal-lab-x86_64-unknown-linux-gnu
";
        let pub_file = "\
untrusted comment: minisign public key 276B6B4C3E53962B
RWQrllM+TGtrJ3+q0A+JQpzZxuAwWZjhjCiZROx2paOrqntpu2dOz+y/
";
        let signature = "\
untrusted comment: signature from minisign secret key
RUQrllM+TGtrJ2Ja6qYYw/NcFnitErv07/BjNYC88mCWA7XZgsHjwvgaK1LP3dRAYDAAVpa64mw5Zbqb2ufJkKQol4nARWWYHAM=
trusted comment: timestamp:1795791815\tfile:SHA256SUMS.txt\thashed
CMhqOn6KzLBY2Zhg18LptvSf6ap/3fOGGHIhrkDH1uYBs/6JYIvk/jO7iVtvJaL1FFtF1PFhYfIDc2Xl4LaHDA==
";
        let contents = [
            (Target::WindowsX86_64, "windows build"),
            (Target::LinuxX86_64, "linux build"),
            (Target::MacosAarch64, "apple silicon build"),
            (Target::MacosX86_64, "intel mac build"),
        ];
        let key = PublicKey::from_pub_file(REPO, pub_file.as_bytes())?;
        assert_eq!(key.fingerprint(), "276B6B4C3E53962B");
        let sums = verify_sums(REPO, &key, sums.as_bytes(), Some(signature.as_bytes()))?;
        for (target, content) in contents {
            let asset = asset_name("signal-lab", target)?;
            assert_eq!(sums.sha256(&asset)?, sha256_hex(content.as_bytes()), "{asset}");
        }
        Ok(())
    }

    #[test]
    fn malformed_checksum_lines_are_refused_with_their_number() {
        let hash = "ab".repeat(32);
        let good = format!("{hash}  good\n");
        let cases = [
            (format!("{}  a", &hash[1..]), 1),
            (format!("{hash}0  a"), 1),
            (format!("{}g  a", &hash[1..]), 1),
            (format!("{hash} a"), 1),
            (format!("{hash}\ta"), 1),
            (format!("{hash}   a"), 1),
            (format!("{hash}  "), 1),
            (format!("{hash}  *"), 1),
            (hash.clone(), 1),
            (format!("{hash}  dist/a"), 1),
            (format!("{hash}  a b"), 1),
            (format!("{hash}  a "), 1),
            (format!("{hash}  a\\b"), 1),
            (format!("{hash}  ."), 1),
            (format!("{hash}  .."), 1),
            (format!("{hash}  a\rb"), 1),
            (format!("{hash}  é"), 1),
            (format!("\u{feff}{hash}  a"), 1),
            (format!("\\{hash}  a"), 1),
            (format!("SHA256 (a) = {hash}"), 1),
            (format!("{good}\n{hash}  a"), 2),
            (format!("{good}\n"), 2),
            (format!("{good}{hash}  bad name\n"), 2),
            ("\r\n".to_owned(), 1),
        ];
        for (text, line) in cases {
            match parse_sums(REPO, text.as_bytes()) {
                Err(VerifyError::MalformedSums { line: found, .. }) => {
                    assert_eq!(found, line, "{text:?}");
                }
                other => panic!("{text:?} gave {other:?}"),
            }
        }
        let not_utf8 = [format!("{hash}  a").as_bytes(), &[0xff]].concat();
        assert_eq!(refusal(parse_sums(REPO, &not_utf8)), Some("release_malformed_sums"));
    }

    #[test]
    fn an_asset_listed_twice_is_refused() -> TestResult {
        let (first, second) = ("ab".repeat(32), "cd".repeat(32));
        for text in [format!("{first}  a\n{first}  a\n"), format!("{first}  a\n{second} *a\n")] {
            let error = parse_sums(REPO, text.as_bytes()).err();
            let error = error.ok_or("an asset listed twice must be refused")?;
            assert_eq!(error.code(), "release_duplicate_asset");
            let message = error.to_string();
            assert!(message.contains("line 2") && message.contains("`a`"), "{message}");
        }
        Ok(())
    }

    #[test]
    fn an_empty_list_is_refused() {
        for text in ["", "\n"] {
            assert_eq!(refusal(parse_sums(REPO, text.as_bytes())), Some("release_empty_sums"));
        }
    }

    #[test]
    fn keys_are_read_from_their_key_line_only() -> TestResult {
        assert_eq!(key(KEY_A)?.as_base64(), KEY_A);
        let pub_file = format!("untrusted comment: minisign public key 027D40E88081F840\n{KEY_A}");
        let invalid = [
            String::new(),
            pub_file,
            format!("{KEY_A}\n"),
            format!(" {KEY_A}"),
            format!("{KEY_A}="),
            KEY_A[..55].to_owned(),
            format!("{KEY_A}AAAA"),
            KEY_A.replacen('+', "-", 1),
            // Bytes `AD`, not the algorithm `Ed`.
            KEY_A.replacen("RW", "QU", 1),
            // `ED`, the algorithm of pre-hashed signatures, not of keys.
            KEY_A.replacen("RW", "RU", 1),
            "A".repeat(56),
        ];
        for text in invalid {
            let error = key(&text).err().ok_or("not a key line")?;
            assert_eq!(error.code(), "release_invalid_key", "{text:?}");
            assert!(error.to_string().contains(REPO), "{error}");
        }
        Ok(())
    }

    #[test]
    fn a_pub_file_gives_its_key() -> TestResult {
        let comment = "untrusted comment: minisign public key 027D40E88081F840";
        for text in [
            format!("{comment}\n{KEY_A}\n"),
            format!("{comment}\n{KEY_A}"),
            format!("{comment}\r\n{KEY_A}\r\n"),
        ] {
            assert_eq!(PublicKey::from_pub_file(REPO, text.as_bytes())?, key(KEY_A)?, "{text:?}");
        }
        let invalid = [
            String::new(),
            KEY_A.to_owned(),
            format!("{KEY_A}\n"),
            format!("comment\n{KEY_A}\n"),
            format!("{comment}\n{KEY_A}\n\n"),
            format!("{comment}\n\n{KEY_A}\n"),
            format!("{comment}\n{KEY_A}\n{KEY_B}\n"),
            format!("{comment}\n {KEY_A}\n"),
            format!("{comment}\n{}\n", KEY_A.replacen("RW", "RU", 1)),
        ];
        for text in invalid {
            let error = PublicKey::from_pub_file(REPO, text.as_bytes()).err();
            let error = error.ok_or("not a .pub file")?;
            assert_eq!(error.code(), "release_invalid_key_file", "{text:?}");
            assert!(error.to_string().contains(REPO), "{error}");
        }
        let not_utf8 = [comment.as_bytes(), b"\n\xff"].concat();
        let read = PublicKey::from_pub_file(REPO, &not_utf8);
        assert_eq!(refusal(read), Some("release_invalid_key_file"));
        let large = format!("{comment}{}\n{KEY_A}\n", " ".repeat(MAX_KEY_FILE_BYTES));
        let read = PublicKey::from_pub_file(REPO, large.as_bytes());
        assert_eq!(refusal(read), Some("release_file_too_large"));
        Ok(())
    }

    #[test]
    fn a_key_is_identified_by_the_sha256_of_its_key_line() -> TestResult {
        let (genuine, impostor) = (key(KEY_A)?, key(KEY_A_ID_B_KEY)?);
        // What `printf %s <key line> | sha256sum` prints.
        assert_eq!(
            genuine.sha256(),
            "92552869fa877ed5681c1282c9c47cac63eba1e3f3e5e037041b133fe4187493"
        );
        assert!(is_sha256(&genuine.sha256()));
        // Same key id, another key: another hash.
        assert_eq!(impostor.fingerprint(), genuine.fingerprint());
        assert_ne!(impostor.sha256(), genuine.sha256());
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        Ok(())
    }

    #[test]
    fn only_lower_case_sha256_digests_are_digests() {
        assert!(is_sha256(&"0a".repeat(32)));
        let upper = "0A".repeat(32);
        let long = "0a".repeat(33);
        let not_hex = format!("{}g", "0".repeat(63));
        for text in ["", "0a", &upper, &long, &not_hex] {
            assert!(!is_sha256(text), "{text}");
        }
    }

    #[test]
    fn fingerprints_are_minisign_key_ids() -> TestResult {
        // As minisign prints them in the `.pub` files of these keys.
        assert_eq!(key(TOOL_KEY)?.fingerprint(), "E7620F1842B4E81F");
        assert_eq!(key(KEY_A)?.fingerprint(), "027D40E88081F840");
        assert_eq!(key(KEY_B)?.fingerprint(), "250D566D7ADA1D2B");
        Ok(())
    }

    #[test]
    fn the_pinned_key_decides_then_the_catalog_key() -> TestResult {
        let (a, b) = (key(KEY_A)?, key(KEY_B)?);
        assert_eq!(key(KEY_A)?, a);
        assert_eq!(key_status(&a, None, None), KeyStatus::Unknown);
        assert_eq!(key_status(&a, Some(&a), None), KeyStatus::Pinned);
        assert_eq!(key_status(&a, Some(&b), None), KeyStatus::Changed);
        assert_eq!(key_status(&a, None, Some(&a)), KeyStatus::Catalog);
        assert_eq!(key_status(&a, None, Some(&b)), KeyStatus::Changed);
        assert_eq!(key_status(&a, Some(&a), Some(&b)), KeyStatus::Pinned);
        assert_eq!(key_status(&a, Some(&b), Some(&a)), KeyStatus::Changed);
        Ok(())
    }

    #[test]
    fn asset_names_follow_the_release_convention() -> TestResult {
        let expected = [
            "signal-lab-x86_64-pc-windows-msvc.exe",
            "signal-lab-x86_64-unknown-linux-gnu",
            "signal-lab-aarch64-apple-darwin",
            "signal-lab-x86_64-apple-darwin",
        ];
        for (target, expected) in Target::ALL.into_iter().zip(expected) {
            assert_eq!(asset_name("signal-lab", target)?, expected);
        }
        Ok(())
    }

    #[test]
    fn asset_names_need_a_slug() {
        for slug in ["", "Signal Lab", "../signal-lab", "signal-lab/x", "-a"] {
            assert_eq!(refusal(asset_name(slug, Target::LinuxX86_64)), Some("hub_invalid_slug"));
        }
    }

    #[test]
    fn the_hub_knows_its_own_target() {
        let Some(target) = Target::CURRENT else {
            // Built for a platform releases do not cover: nothing to compare.
            return;
        };
        let os = match target {
            Target::WindowsX86_64 => "windows",
            Target::LinuxX86_64 => "linux",
            Target::MacosAarch64 | Target::MacosX86_64 => "macos",
        };
        assert_eq!(os, std::env::consts::OS);
        assert!(target.triple().starts_with(std::env::consts::ARCH), "{}", target.triple());
        assert_eq!(target.exe_suffix(), std::env::consts::EXE_SUFFIX);
    }
}
