//! Reads GitHub releases and catalogs (ADR 0013): the only network code of
//! Eario.
//!
//! [`GitHub`] is what the hub needs from GitHub: the latest release of a
//! repository, a small file of that release (its checksum list and
//! signature), the `catalog.json` of a repository, and the download of one
//! release asset. [`GitHubClient`] does it over HTTPS.
//!
//! Every request is for a repository that passes [`eario_core::is_repo`].
//! Every URL, the first and each one a redirect leads to, must be HTTPS to
//! one of [`ALLOWED_HOSTS`] on the default port; redirects are followed by
//! the client itself, at most [`MAX_REDIRECTS`] of them, each one checked
//! before it is requested. Every read is bounded while it streams, whatever
//! the server declares: [`MAX_JSON_BYTES`] for an API answer or a catalog,
//! the size the release lists for an asset, at most [`MAX_ASSET_BYTES`].
//!
//! The client blocks. The hub calls it through `tokio::task::spawn_blocking`,
//! never on the asynchronous runtime.
//!
//! [`API_VARIABLE`] points the client at a fake GitHub on the loopback
//! interface, which then replaces every GitHub host. Only debug builds read
//! it; release builds always talk to GitHub.

use std::io::{self, Read, Write};
use std::sync::Arc;
use std::time::Duration;

use eario_core::{Catalog, is_repo};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use ureq::http::{Response, StatusCode, Uri};
use ureq::tls::{Certificate, RootCerts, TlsConfig, TlsProvider};
use ureq::{Agent, Body};

/// Environment variable that replaces GitHub by a server on the loopback
/// interface, given as `http://127.0.0.1:<port>` or `http://localhost:<port>`.
/// Read by debug builds only; see [`Endpoint::from_env`].
pub const API_VARIABLE: &str = "EARIO_HUB_GITHUB_API";

/// The hosts the client talks to: the API, and the hosts a release asset
/// download is redirected through.
pub const ALLOWED_HOSTS: [&str; 4] = [
    "api.github.com",
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];

/// Name of the catalog file in a repository, read from its default branch.
pub const CATALOG_FILE: &str = "catalog.json";

/// Largest API answer or catalog read, in bytes.
pub const MAX_JSON_BYTES: u64 = 2 * 1024 * 1024;

/// Largest release asset downloaded, in bytes.
pub const MAX_ASSET_BYTES: u64 = 512 * 1024 * 1024;

/// Most redirects followed for one request. A release asset takes one.
pub const MAX_REDIRECTS: usize = 5;

/// Default longest wait for GitHub: to resolve its name, to connect, to send
/// a request, for an answer, and between two reads of its body.
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// Default longest time an API answer, a catalog or a release file may take
/// to arrive whole. A download has no such limit, only [`TIMEOUT`] between
/// two reads.
pub const SMALL_FILE_TIMEOUT: Duration = Duration::from_secs(60);

/// Media type of the GitHub REST API.
const API_MEDIA_TYPE: &str = "application/vnd.github+json";

/// Media type that makes the contents endpoint answer with the file itself.
const RAW_MEDIA_TYPE: &str = "application/vnd.github.raw+json";

/// Media type asked for a release asset.
const BINARY_MEDIA_TYPE: &str = "application/octet-stream";

/// Version of the GitHub REST API the client is written against.
const API_VERSION: &str = "2022-11-28";

/// Size of a read buffer.
const CHUNK_BYTES: usize = 64 * 1024;

/// Why a request to GitHub failed. Every variant about a repository names it.
#[derive(Debug, thiserror::Error)]
pub enum GitHubError {
    /// The request named something that is not a repository.
    #[error(
        "`{0}` is not a GitHub repository; give it as `owner/repo`, with ASCII letters, digits, `-` (and `.`, `_` in the repository name) only"
    )]
    InvalidRepo(String),
    /// GitHub could not be reached.
    #[error(
        "could not reach GitHub for `{repo}` ({detail}); check that this computer is online, then try again"
    )]
    Offline {
        /// The repository.
        repo: String,
        /// What failed, for diagnosis.
        detail: String,
    },
    /// GitHub did not answer in time.
    #[error(
        "GitHub did not answer in time for `{repo}`; check the network connection and try again"
    )]
    Timeout {
        /// The repository.
        repo: String,
    },
    /// The repository has no published release, or is not public.
    #[error(
        "`{repo}` has no published release on GitHub, or is not a public repository; check its name, or ask its author to publish a release that is neither a draft nor a pre-release"
    )]
    NoRelease {
        /// The repository.
        repo: String,
    },
    /// A file was not found.
    #[error(
        "GitHub has no `{file}` for `{repo}`; check the repository name, or ask its author to publish the file"
    )]
    NotFound {
        /// The repository.
        repo: String,
        /// The file asked for.
        file: String,
    },
    /// GitHub refuses more requests for now.
    #[error(
        "GitHub refused the request for `{repo}`: this computer made too many requests to GitHub without signing in; try again in an hour"
    )]
    RateLimited {
        /// The repository.
        repo: String,
    },
    /// GitHub answered with an unexpected status.
    #[error("GitHub answered HTTP {status} for `{file}` of `{repo}`; try again later")]
    Status {
        /// The repository.
        repo: String,
        /// The file asked for.
        file: String,
        /// The HTTP status code.
        status: u16,
    },
    /// A URL, or a redirect, leads outside the allowed hosts or away from
    /// HTTPS. Nothing was requested from it.
    #[error(
        "refused to fetch `{file}` of `{repo}` from {url}: the hub downloads over HTTPS from GitHub's own hosts only"
    )]
    ForbiddenUrl {
        /// The repository.
        repo: String,
        /// The file asked for.
        file: String,
        /// The refused URL.
        url: String,
    },
    /// A redirect gave no usable location, or redirects did not end.
    #[error(
        "GitHub redirected the request for `{file}` of `{repo}` more than {MAX_REDIRECTS} times, or to no location; try again later"
    )]
    BadRedirect {
        /// The repository.
        repo: String,
        /// The file asked for.
        file: String,
    },
    /// A file is larger than the hub accepts.
    #[error("`{file}` of `{repo}` is larger than the {limit} bytes the hub accepts")]
    TooLarge {
        /// The repository.
        repo: String,
        /// The file.
        file: String,
        /// The largest size accepted, in bytes.
        limit: u64,
    },
    /// A release file did not have the size its release lists.
    #[error(
        "`{file}` of `{repo}` did not have the {expected} bytes its release lists; the download was cut or the file was replaced; try again"
    )]
    SizeMismatch {
        /// The repository.
        repo: String,
        /// The file.
        file: String,
        /// The size the release lists, in bytes.
        expected: u64,
    },
    /// An answer is not the JSON expected.
    #[error("`{file}` of `{repo}` is not valid: {detail}")]
    BadJson {
        /// The repository.
        repo: String,
        /// The file: the release description or [`CATALOG_FILE`].
        file: String,
        /// What is wrong with it.
        detail: String,
    },
    /// The downloaded bytes could not be written.
    #[error("could not write `{file}` of `{repo}` to disk: {source}")]
    Write {
        /// The repository.
        repo: String,
        /// The file.
        file: String,
        /// The underlying failure.
        source: io::Error,
    },
    /// Another failure of the connection, such as a TLS error.
    #[error("the connection to GitHub for `{repo}` failed: {detail}")]
    Transport {
        /// The repository.
        repo: String,
        /// What failed.
        detail: String,
    },
    /// The system has no root certificate to check GitHub's with.
    #[error(
        "found no root certificate on this computer to check GitHub's certificate with ({detail}); the system's certificate store must be readable"
    )]
    Certificates {
        /// Why none was loaded.
        detail: String,
    },
}

impl GitHubError {
    /// Stable, machine-readable code of the error.
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidRepo(_) => "github_invalid_repo",
            Self::Offline { .. } => "github_offline",
            Self::Timeout { .. } => "github_timeout",
            Self::NoRelease { .. } => "github_no_release",
            Self::NotFound { .. } => "github_not_found",
            Self::RateLimited { .. } => "github_rate_limited",
            Self::Status { .. } => "github_http_status",
            Self::ForbiddenUrl { .. } => "github_forbidden_url",
            Self::BadRedirect { .. } => "github_bad_redirect",
            Self::TooLarge { .. } => "github_too_large",
            Self::SizeMismatch { .. } => "github_size_mismatch",
            Self::BadJson { .. } => "github_bad_json",
            Self::Write { .. } => "github_write_failed",
            Self::Transport { .. } => "github_transport",
            Self::Certificates { .. } => "github_no_certificates",
        }
    }
}

/// The latest release of a repository: neither a draft nor a pre-release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// Its tag, for example `v1.2.0`.
    pub tag: String,
    /// Its assets, in the order GitHub lists them.
    pub assets: Vec<ReleaseAsset>,
}

impl Release {
    /// The asset named `name`, when the release has one.
    pub fn asset(&self, name: &str) -> Option<&ReleaseAsset> {
        self.assets.iter().find(|asset| asset.name == name)
    }
}

/// A file attached to a release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseAsset {
    /// File name.
    pub name: String,
    /// Size in bytes, as GitHub lists it.
    pub size: u64,
    /// Where to download it. Checked against the allowed hosts when it is
    /// requested, not before.
    pub download_url: String,
}

/// What a download wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Download {
    /// Bytes written.
    pub size: u64,
    /// SHA-256 of those bytes, in lower-case hexadecimal.
    pub sha256: String,
}

/// The two files a release must have for anything in it to be installed.
/// A field is `None` when the release has no asset of that name: the caller
/// refuses such a release, the client only reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChecksumFiles {
    /// Content of [`crate::verify::SUMS_FILE`].
    pub sums: Option<Vec<u8>>,
    /// Content of [`crate::verify::SIGNATURE_FILE`].
    pub signature: Option<Vec<u8>>,
}

/// What the hub needs from GitHub. Every method blocks; call it through
/// `tokio::task::spawn_blocking`.
///
/// `repo` is an `owner/repo` that must pass [`is_repo`]; anything else is
/// refused before any request.
pub trait GitHub: Send + Sync {
    /// The latest release of `repo` that is neither a draft nor a
    /// pre-release.
    fn latest_release(&self, repo: &str) -> Result<Release, GitHubError>;

    /// The [`CATALOG_FILE`] of `repo`, from its default branch. Its entries
    /// are not checked here.
    fn catalog(&self, repo: &str) -> Result<Catalog, GitHubError>;

    /// Content of the asset `name` of `release`, `None` when the release has
    /// none of that name. Refused when it is larger than `limit` bytes.
    fn release_file(
        &self,
        repo: &str,
        release: &Release,
        name: &str,
        limit: u64,
    ) -> Result<Option<Vec<u8>>, GitHubError>;

    /// Downloads `asset` of a release of `repo` into `out`, computing its
    /// SHA-256 on the way. Refused, possibly after writing part of it, when
    /// the asset is larger than [`MAX_ASSET_BYTES`] or than its release
    /// lists, or shorter. The caller gives a temporary file and discards it
    /// on error.
    fn download(
        &self,
        repo: &str,
        asset: &ReleaseAsset,
        out: &mut dyn Write,
    ) -> Result<Download, GitHubError>;

    /// The checksum list of `release` and its signature, each bounded by its
    /// limit in [`crate::verify`].
    fn checksum_files(&self, repo: &str, release: &Release) -> Result<ChecksumFiles, GitHubError> {
        use crate::verify::{MAX_SIGNATURE_BYTES, MAX_SUMS_BYTES, SIGNATURE_FILE, SUMS_FILE};
        Ok(ChecksumFiles {
            sums: self.release_file(repo, release, SUMS_FILE, MAX_SUMS_BYTES as u64)?,
            signature: self.release_file(
                repo,
                release,
                SIGNATURE_FILE,
                MAX_SIGNATURE_BYTES as u64,
            )?,
        })
    }
}

/// Where the client sends its requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    /// GitHub itself, over HTTPS, on [`ALLOWED_HOSTS`].
    GitHub,
    /// A server on the loopback interface, over plain HTTP, in place of every
    /// GitHub host: a fake GitHub for tests.
    Loopback {
        /// `127.0.0.1` or `localhost`.
        host: String,
        /// Its port.
        port: u16,
    },
}

impl Endpoint {
    /// The endpoint of this process: GitHub, unless this is a debug build and
    /// [`API_VARIABLE`] names a loopback server. A value that does not is
    /// ignored, with a warning.
    pub fn from_env() -> Self {
        let value = if cfg!(debug_assertions) { std::env::var(API_VARIABLE).ok() } else { None };
        Self::from_override(value.as_deref())
    }

    /// The endpoint for an override value: GitHub without one.
    fn from_override(value: Option<&str>) -> Self {
        let Some(value) = value else {
            return Self::GitHub;
        };
        match Self::loopback(value) {
            Some(endpoint) => endpoint,
            None => {
                tracing::warn!(
                    value,
                    "ignoring {API_VARIABLE}: not http://127.0.0.1:<port> or http://localhost:<port>"
                );
                Self::GitHub
            }
        }
    }

    /// The loopback endpoint `text` names: exactly `http://127.0.0.1:<port>`
    /// or `http://localhost:<port>`, with an optional final `/`. `None` for
    /// anything else.
    pub fn loopback(text: &str) -> Option<Self> {
        let rest = text.strip_prefix("http://")?;
        let rest = rest.strip_suffix('/').unwrap_or(rest);
        let (host, port) = rest.split_once(':')?;
        if host != "127.0.0.1" && host != "localhost" {
            return None;
        }
        if port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let port: u16 = port.parse().ok().filter(|&port| port != 0)?;
        Some(Self::Loopback { host: host.to_owned(), port })
    }

    /// Base URL of the REST API.
    fn api_base(&self) -> String {
        match self {
            Self::GitHub => "https://api.github.com".to_owned(),
            Self::Loopback { host, port } => format!("http://{host}:{port}"),
        }
    }

    /// Whether the client may request `uri`.
    fn allows(&self, uri: &Uri) -> bool {
        let Some(authority) = uri.authority() else {
            return false;
        };
        // No user information: `https://api.github.com@evil.example/` names
        // `evil.example`, and nothing legitimate needs one.
        if authority.as_str().contains('@') {
            return false;
        }
        let host = authority.host();
        match self {
            Self::GitHub => {
                uri.scheme_str() == Some("https")
                    && matches!(authority.port_u16(), None | Some(443))
                    && ALLOWED_HOSTS.iter().any(|allowed| host.eq_ignore_ascii_case(allowed))
            }
            Self::Loopback { host: own_host, port } => {
                uri.scheme_str() == Some("http")
                    && authority.port_u16() == Some(*port)
                    && host.eq_ignore_ascii_case(own_host)
            }
        }
    }
}

/// Limits of a [`GitHubClient`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientOptions {
    /// See [`MAX_JSON_BYTES`].
    pub max_json_bytes: u64,
    /// See [`MAX_ASSET_BYTES`].
    pub max_asset_bytes: u64,
    /// See [`TIMEOUT`].
    pub timeout: Duration,
    /// See [`SMALL_FILE_TIMEOUT`].
    pub small_file_timeout: Duration,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            max_json_bytes: MAX_JSON_BYTES,
            max_asset_bytes: MAX_ASSET_BYTES,
            timeout: TIMEOUT,
            small_file_timeout: SMALL_FILE_TIMEOUT,
        }
    }
}

/// The [`GitHub`] of the hub: HTTPS with rustls, the system's root
/// certificates, no automatic redirect. Cloning it is cheap and shares the
/// connections.
#[derive(Clone)]
pub struct GitHubClient {
    agent: Agent,
    endpoint: Endpoint,
    options: ClientOptions,
}

/// One GET: what to ask, how much to accept, how to name it in errors.
struct Fetch<'a> {
    repo: &'a str,
    url: String,
    /// Name of what is fetched, for errors.
    file: &'a str,
    accept: &'static str,
    /// Size the release lists, which the body must have exactly.
    expected: Option<u64>,
    /// Largest body accepted.
    limit: u64,
    /// Longest time for the whole exchange, body included.
    total_timeout: Option<Duration>,
    /// Error for a `404`.
    not_found: fn(&str, &str) -> GitHubError,
}

impl GitHubClient {
    /// A client of the endpoint of this process (see [`Endpoint::from_env`])
    /// with the default limits.
    pub fn new() -> Result<Self, GitHubError> {
        Self::with_options(Endpoint::from_env(), ClientOptions::default())
    }

    /// A client of `endpoint` with `options`. Reads the system's root
    /// certificates when the endpoint is GitHub; touches no network.
    pub fn with_options(endpoint: Endpoint, options: ClientOptions) -> Result<Self, GitHubError> {
        let mut config = Agent::config_builder()
            .tls_config(tls_config(&endpoint)?)
            // Redirects are followed by `open`, which checks every hop.
            .max_redirects(0)
            .http_status_as_error(false)
            .https_only(endpoint == Endpoint::GitHub)
            .user_agent(concat!("eario-hub/", env!("CARGO_PKG_VERSION")))
            .timeout_resolve(Some(options.timeout))
            .timeout_connect(Some(options.timeout))
            .timeout_send_request(Some(options.timeout))
            .timeout_recv_response(Some(options.timeout))
            .timeout_per_read(Some(options.timeout));
        // A proxy from the environment serves GitHub (HTTPS goes through it
        // end to end); the loopback server is reached directly.
        if endpoint != Endpoint::GitHub {
            config = config.proxy(None);
        }
        Ok(Self { agent: config.build().new_agent(), endpoint, options })
    }

    /// The endpoint the client talks to.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// Sends `fetch` and passes the body to `sink` as it arrives, within its
    /// limits. Returns the number of bytes read.
    fn fetch(
        &self,
        fetch: &Fetch<'_>,
        sink: &mut dyn FnMut(&[u8]) -> io::Result<()>,
    ) -> Result<u64, GitHubError> {
        let repo = fetch.repo;
        let too_large = || GitHubError::TooLarge {
            repo: repo.to_owned(),
            file: fetch.file.to_owned(),
            limit: fetch.limit,
        };
        let mismatch = |expected| GitHubError::SizeMismatch {
            repo: repo.to_owned(),
            file: fetch.file.to_owned(),
            expected,
        };
        if fetch.expected.is_some_and(|expected| expected > fetch.limit) {
            return Err(too_large());
        }
        let response = self.open(fetch)?;
        if let Some(length) = response.body().content_length() {
            if length > fetch.limit {
                return Err(too_large());
            }
            if let Some(expected) = fetch.expected
                && length != expected
            {
                return Err(mismatch(expected));
            }
        }
        // The declared length is not trusted: the body is counted as it
        // streams, and refused at the first byte over the limit.
        let limit = fetch.expected.unwrap_or(fetch.limit);
        let mut reader = response.into_body().into_reader();
        let mut buffer = vec![0_u8; CHUNK_BYTES];
        let mut total = 0_u64;
        loop {
            let read = match reader.read(&mut buffer) {
                Ok(read) => read,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(transport_error(repo, ureq::Error::from(error))),
            };
            if read == 0 {
                break;
            }
            total += read as u64;
            if total > limit {
                return Err(match fetch.expected {
                    Some(expected) => mismatch(expected),
                    None => too_large(),
                });
            }
            sink(&buffer[..read]).map_err(|source| GitHubError::Write {
                repo: repo.to_owned(),
                file: fetch.file.to_owned(),
                source,
            })?;
        }
        if let Some(expected) = fetch.expected
            && total != expected
        {
            return Err(mismatch(expected));
        }
        Ok(total)
    }

    /// Sends `fetch`, following redirects after checking each location, and
    /// returns the first answer that is not a redirect, if it is a success.
    fn open(&self, fetch: &Fetch<'_>) -> Result<Response<Body>, GitHubError> {
        let repo = fetch.repo;
        let bad_redirect =
            || GitHubError::BadRedirect { repo: repo.to_owned(), file: fetch.file.to_owned() };
        let mut url = fetch.url.clone();
        for _ in 0..=MAX_REDIRECTS {
            let uri = self.checked_uri(fetch, &url)?;
            tracing::debug!(repo, file = fetch.file, url = %uri, "GitHub request");
            let mut request = self.agent.get(uri.clone()).header("accept", fetch.accept);
            if fetch.accept != BINARY_MEDIA_TYPE {
                request = request.header("x-github-api-version", API_VERSION);
            }
            let response = request
                .config()
                .timeout_global(fetch.total_timeout)
                .build()
                .call()
                .map_err(|error| transport_error(repo, error))?;
            let status = response.status();
            if status.is_redirection() {
                let location = response
                    .headers()
                    .get("location")
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(bad_redirect)?;
                url =
                    resolve_location(&uri, location).ok_or_else(|| GitHubError::ForbiddenUrl {
                        repo: repo.to_owned(),
                        file: fetch.file.to_owned(),
                        url: location.to_owned(),
                    })?;
                continue;
            }
            return check_status(fetch, response);
        }
        Err(bad_redirect())
    }

    /// `url` parsed, when the endpoint allows it.
    fn checked_uri(&self, fetch: &Fetch<'_>, url: &str) -> Result<Uri, GitHubError> {
        url.parse::<Uri>().ok().filter(|uri| self.endpoint.allows(uri)).ok_or_else(|| {
            GitHubError::ForbiddenUrl {
                repo: fetch.repo.to_owned(),
                file: fetch.file.to_owned(),
                url: url.to_owned(),
            }
        })
    }

    /// The body of a small answer, read whole within `fetch`'s limits.
    fn read_small(&self, fetch: &Fetch<'_>) -> Result<Vec<u8>, GitHubError> {
        let mut bytes = Vec::new();
        self.fetch(fetch, &mut |chunk| {
            bytes.extend_from_slice(chunk);
            Ok(())
        })?;
        Ok(bytes)
    }
}

impl GitHub for GitHubClient {
    fn latest_release(&self, repo: &str) -> Result<Release, GitHubError> {
        check_repo(repo)?;
        const FILE: &str = "latest release";
        let fetch = Fetch {
            repo,
            url: format!("{}/repos/{repo}/releases/latest", self.endpoint.api_base()),
            file: FILE,
            accept: API_MEDIA_TYPE,
            expected: None,
            limit: self.options.max_json_bytes,
            total_timeout: Some(self.options.small_file_timeout),
            not_found: |repo, _| GitHubError::NoRelease { repo: repo.to_owned() },
        };
        let bytes = self.read_small(&fetch)?;
        let release: ApiRelease =
            serde_json::from_slice(&bytes).map_err(|error| GitHubError::BadJson {
                repo: repo.to_owned(),
                file: FILE.to_owned(),
                detail: error.to_string(),
            })?;
        // `/releases/latest` never gives either, but nothing else is accepted.
        if release.draft || release.prerelease {
            return Err(GitHubError::NoRelease { repo: repo.to_owned() });
        }
        Ok(Release {
            tag: release.tag_name,
            assets: release
                .assets
                .into_iter()
                .map(|asset| ReleaseAsset {
                    name: asset.name,
                    size: asset.size,
                    download_url: asset.browser_download_url,
                })
                .collect(),
        })
    }

    fn catalog(&self, repo: &str) -> Result<Catalog, GitHubError> {
        check_repo(repo)?;
        let fetch = Fetch {
            repo,
            url: format!("{}/repos/{repo}/contents/{CATALOG_FILE}", self.endpoint.api_base()),
            file: CATALOG_FILE,
            accept: RAW_MEDIA_TYPE,
            expected: None,
            limit: self.options.max_json_bytes,
            total_timeout: Some(self.options.small_file_timeout),
            not_found,
        };
        let bytes = self.read_small(&fetch)?;
        serde_json::from_slice(&bytes).map_err(|error| GitHubError::BadJson {
            repo: repo.to_owned(),
            file: CATALOG_FILE.to_owned(),
            detail: error.to_string(),
        })
    }

    fn release_file(
        &self,
        repo: &str,
        release: &Release,
        name: &str,
        limit: u64,
    ) -> Result<Option<Vec<u8>>, GitHubError> {
        check_repo(repo)?;
        let Some(asset) = release.asset(name) else {
            return Ok(None);
        };
        let fetch = Fetch {
            repo,
            url: asset.download_url.clone(),
            file: name,
            accept: BINARY_MEDIA_TYPE,
            expected: Some(asset.size),
            limit,
            total_timeout: Some(self.options.small_file_timeout),
            not_found,
        };
        self.read_small(&fetch).map(Some)
    }

    fn download(
        &self,
        repo: &str,
        asset: &ReleaseAsset,
        out: &mut dyn Write,
    ) -> Result<Download, GitHubError> {
        check_repo(repo)?;
        let fetch = Fetch {
            repo,
            url: asset.download_url.clone(),
            file: &asset.name,
            accept: BINARY_MEDIA_TYPE,
            expected: Some(asset.size),
            limit: self.options.max_asset_bytes,
            total_timeout: None,
            not_found,
        };
        let mut hasher = Sha256::new();
        let size = self.fetch(&fetch, &mut |chunk| {
            hasher.update(chunk);
            out.write_all(chunk)
        })?;
        out.flush().map_err(|source| GitHubError::Write {
            repo: repo.to_owned(),
            file: asset.name.clone(),
            source,
        })?;
        let sha256 = hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
        Ok(Download { size, sha256 })
    }
}

/// A release in the GitHub REST API; only the fields the hub reads.
#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<ApiAsset>,
}

/// A release asset in the GitHub REST API; only the fields the hub reads.
#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    size: u64,
    browser_download_url: String,
}

fn check_repo(repo: &str) -> Result<(), GitHubError> {
    if is_repo(repo) { Ok(()) } else { Err(GitHubError::InvalidRepo(repo.to_owned())) }
}

/// The `404` error of a file.
fn not_found(repo: &str, file: &str) -> GitHubError {
    GitHubError::NotFound { repo: repo.to_owned(), file: file.to_owned() }
}

/// TLS of the client: rustls with ring, and the system's root certificates
/// when the endpoint is GitHub.
fn tls_config(endpoint: &Endpoint) -> Result<TlsConfig, GitHubError> {
    let roots: Vec<Certificate<'static>> = match endpoint {
        Endpoint::GitHub => system_roots()?,
        // Plain HTTP: no certificate is ever checked.
        Endpoint::Loopback { .. } => Vec::new(),
    };
    Ok(TlsConfig::builder()
        .provider(TlsProvider::Rustls)
        .root_certs(RootCerts::Specific(Arc::new(roots)))
        .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .build())
}

/// The root certificates of the system's store. Certificates that fail to
/// load are skipped with a warning; none at all is an error.
fn system_roots() -> Result<Vec<Certificate<'static>>, GitHubError> {
    let loaded = rustls_native_certs::load_native_certs();
    for error in &loaded.errors {
        tracing::warn!(%error, "skipped a root certificate of the system");
    }
    if loaded.certs.is_empty() {
        let detail = loaded
            .errors
            .first()
            .map_or_else(|| "the store is empty".to_owned(), ToString::to_string);
        return Err(GitHubError::Certificates { detail });
    }
    Ok(loaded.certs.iter().map(|der| Certificate::from_der(der).to_owned()).collect())
}

/// The URL a redirect from `from` to `location` leads to: `location` itself
/// when absolute, or joined to the scheme and host of `from` when it starts
/// with one `/`. `None` for any other form; the endpoint checks the result.
fn resolve_location(from: &Uri, location: &str) -> Option<String> {
    if location.starts_with("https://") || location.starts_with("http://") {
        return Some(location.to_owned());
    }
    if location.starts_with('/') && !location.starts_with("//") {
        let scheme = from.scheme_str()?;
        let authority = from.authority()?;
        return Some(format!("{scheme}://{authority}{location}"));
    }
    None
}

/// `response` when it is a success; otherwise the error its status means.
fn check_status(
    fetch: &Fetch<'_>,
    response: Response<Body>,
) -> Result<Response<Body>, GitHubError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let repo = fetch.repo;
    let exhausted = response
        .headers()
        .get("x-ratelimit-remaining")
        .is_some_and(|value| value.as_bytes() == b"0");
    Err(match status {
        StatusCode::NOT_FOUND => (fetch.not_found)(repo, fetch.file),
        StatusCode::TOO_MANY_REQUESTS => GitHubError::RateLimited { repo: repo.to_owned() },
        StatusCode::FORBIDDEN if exhausted => GitHubError::RateLimited { repo: repo.to_owned() },
        _ => GitHubError::Status {
            repo: repo.to_owned(),
            file: fetch.file.to_owned(),
            status: status.as_u16(),
        },
    })
}

/// The error a failed exchange means for `repo`.
fn transport_error(repo: &str, error: ureq::Error) -> GitHubError {
    let repo = repo.to_owned();
    match error {
        ureq::Error::Timeout(_) => GitHubError::Timeout { repo },
        ureq::Error::Io(error)
            if matches!(error.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock) =>
        {
            GitHubError::Timeout { repo }
        }
        error
        @ (ureq::Error::HostNotFound | ureq::Error::ConnectionFailed | ureq::Error::Io(_)) => {
            GitHubError::Offline { repo, detail: error.to_string() }
        }
        error => GitHubError::Transport { repo, detail: error.to_string() },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake_github::{FakeAsset, FakeGitHub, FakeResponse};
    use crate::supervisor::HubError;
    use crate::verify::{SIGNATURE_FILE, SUMS_FILE};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    const REPO: &str = "lab/signal-lab";
    const ASSET: &str = "signal-lab-x86_64-unknown-linux-gnu";
    /// SHA-256 of `hello`.
    const HELLO_SHA256: &str = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
    const CATALOG: &str = r#"{
        "format_version": 1,
        "name": "Lab dashboards",
        "dashboards": [
            {"repo": "lab/signal-lab", "name": "Signal lab",
             "public_key": "RWRA+IGA6EB9AuIbrrq9pGIpkX+AusRGz1XiVhgRrTfRER9SJa+jD9+x"}
        ]
    }"#;

    fn options() -> ClientOptions {
        ClientOptions { timeout: Duration::from_secs(10), ..ClientOptions::default() }
    }

    fn client_with(fake: &FakeGitHub, options: ClientOptions) -> Result<GitHubClient, GitHubError> {
        let endpoint = Endpoint::loopback(&fake.base_url())
            .ok_or_else(|| GitHubError::InvalidRepo(fake.base_url()))?;
        GitHubClient::with_options(endpoint, options)
    }

    fn client(fake: &FakeGitHub) -> Result<GitHubClient, GitHubError> {
        client_with(fake, options())
    }

    /// A fake GitHub with a release of `REPO` holding `ASSET` (`hello`) and
    /// its checksum list.
    fn fake_with_release() -> io::Result<FakeGitHub> {
        let fake = FakeGitHub::start()?;
        fake.release(
            REPO,
            "v1.2.0",
            &[FakeAsset::new(ASSET, "hello"), FakeAsset::new(SUMS_FILE, "sums")],
        );
        Ok(fake)
    }

    /// Code of the error, `None` on success.
    fn code<T>(result: Result<T, GitHubError>) -> Option<&'static str> {
        result.err().map(|error| error.code())
    }

    fn asset_at(url: &str, size: u64) -> ReleaseAsset {
        ReleaseAsset { name: ASSET.to_owned(), size, download_url: url.to_owned() }
    }

    #[test]
    fn the_latest_release_is_read_with_its_assets() -> TestResult {
        let fake = fake_with_release()?;
        let release = client(&fake)?.latest_release(REPO)?;
        assert_eq!(release.tag, "v1.2.0");
        let names: Vec<&str> = release.assets.iter().map(|asset| asset.name.as_str()).collect();
        assert_eq!(names, [ASSET, SUMS_FILE]);
        let asset = release.asset(ASSET).ok_or("asset listed")?;
        assert_eq!(asset.size, 5);
        assert_eq!(
            asset.download_url,
            format!("{}/{REPO}/releases/download/v1.2.0/{ASSET}", fake.base_url())
        );
        assert_eq!(fake.requests(), [format!("/repos/{REPO}/releases/latest")]);
        Ok(())
    }

    #[test]
    fn drafts_and_pre_releases_are_not_releases() -> TestResult {
        let fake = FakeGitHub::start()?;
        let client = client(&fake)?;
        for (draft, prerelease) in [(true, false), (false, true)] {
            let release = serde_json::json!({
                "tag_name": "v2.0.0-rc.1", "draft": draft, "prerelease": prerelease, "assets": [],
            });
            fake.route(
                &format!("/repos/{REPO}/releases/latest"),
                FakeResponse::ok(release.to_string()),
            );
            assert_eq!(code(client.latest_release(REPO)), Some("github_no_release"));
        }
        Ok(())
    }

    #[test]
    fn an_asset_is_downloaded_through_its_redirect_with_its_sha256() -> TestResult {
        let fake = fake_with_release()?;
        let client = client(&fake)?;
        let release = client.latest_release(REPO)?;
        let mut out = Vec::new();
        let download =
            client.download(REPO, release.asset(ASSET).ok_or("asset listed")?, &mut out)?;
        assert_eq!(out, b"hello");
        assert_eq!(download, Download { size: 5, sha256: HELLO_SHA256.to_owned() });
        assert_eq!(
            fake.requests()[1..],
            [
                format!("/{REPO}/releases/download/v1.2.0/{ASSET}"),
                format!("/assets/{REPO}/v1.2.0/{ASSET}"),
            ]
        );
        Ok(())
    }

    #[test]
    fn a_relative_redirect_stays_on_the_same_host() -> TestResult {
        let fake = FakeGitHub::start()?;
        fake.route("/first", FakeResponse::redirect("/second"));
        fake.route("/second", FakeResponse::ok("hello"));
        let mut out = Vec::new();
        let url = format!("{}/first", fake.base_url());
        client(&fake)?.download(REPO, &asset_at(&url, 5), &mut out)?;
        assert_eq!(out, b"hello");
        Ok(())
    }

    #[test]
    fn checksum_files_are_read_and_a_missing_signature_is_reported() -> TestResult {
        let fake = fake_with_release()?;
        let client = client(&fake)?;
        let release = client.latest_release(REPO)?;
        let files = client.checksum_files(REPO, &release)?;
        assert_eq!(files.sums.as_deref(), Some(&b"sums"[..]));
        assert_eq!(files.signature, None, "the release has no signature");

        fake.release(
            REPO,
            "v1.3.0",
            &[FakeAsset::new(SUMS_FILE, "sums"), FakeAsset::new(SIGNATURE_FILE, "signature")],
        );
        let release = client.latest_release(REPO)?;
        let files = client.checksum_files(REPO, &release)?;
        assert_eq!(files.signature.as_deref(), Some(&b"signature"[..]));
        Ok(())
    }

    #[test]
    fn a_catalog_is_read_from_the_contents_endpoint() -> TestResult {
        let fake = FakeGitHub::start()?;
        fake.catalog("lab/catalog", CATALOG);
        let catalog = client(&fake)?.catalog("lab/catalog")?;
        assert_eq!(catalog.name, "Lab dashboards");
        assert_eq!(catalog.dashboards[0].repo, REPO);
        assert_eq!(fake.requests(), ["/repos/lab/catalog/contents/catalog.json"]);
        Ok(())
    }

    #[test]
    fn answers_that_are_not_the_expected_json_are_refused() -> TestResult {
        let fake = FakeGitHub::start()?;
        let client = client(&fake)?;
        fake.catalog("lab/catalog", "{\"name\": \"no format version\"}");
        assert_eq!(code(client.catalog("lab/catalog")), Some("github_bad_json"));
        fake.route(&format!("/repos/{REPO}/releases/latest"), FakeResponse::ok("<html>"));
        let error = client.latest_release(REPO).err().ok_or("refused")?;
        assert_eq!(error.code(), "github_bad_json");
        assert!(error.to_string().contains(REPO), "{error}");
        Ok(())
    }

    #[test]
    fn missing_releases_and_files_are_coded_errors_naming_the_repository() -> TestResult {
        let fake = FakeGitHub::start()?;
        let client = client(&fake)?;
        for (error, expected) in [
            (client.latest_release(REPO).err(), "github_no_release"),
            (client.catalog(REPO).err(), "github_not_found"),
        ] {
            let error = HubError::from(error.ok_or("refused")?);
            assert_eq!(error.code(), expected);
            assert!(error.to_string().contains(REPO), "{error}");
        }
        Ok(())
    }

    #[test]
    fn rate_limits_are_recognised() -> TestResult {
        let fake = FakeGitHub::start()?;
        let client = client(&fake)?;
        let path = format!("/repos/{REPO}/releases/latest");
        fake.route(&path, FakeResponse::rate_limited());
        let error = HubError::from(client.latest_release(REPO).err().ok_or("refused")?);
        assert_eq!(error.code(), "github_rate_limited");
        assert!(error.to_string().contains(REPO), "{error}");

        fake.route(&path, FakeResponse::new(429, "slow down"));
        assert_eq!(code(client.latest_release(REPO)), Some("github_rate_limited"));
        // A refusal with requests left is not the rate limit.
        fake.route(&path, FakeResponse::new(403, "no").header("x-ratelimit-remaining", "59"));
        assert_eq!(code(client.latest_release(REPO)), Some("github_http_status"));
        fake.route(&path, FakeResponse::new(500, "oops"));
        assert_eq!(code(client.latest_release(REPO)), Some("github_http_status"));
        Ok(())
    }

    #[test]
    fn an_unreachable_server_is_offline() -> TestResult {
        let port = std::net::TcpListener::bind(("127.0.0.1", 0))?.local_addr()?.port();
        let endpoint = Endpoint::loopback(&format!("http://127.0.0.1:{port}")).ok_or("loopback")?;
        let client = GitHubClient::with_options(endpoint, options())?;
        let error = HubError::from(client.latest_release(REPO).err().ok_or("refused")?);
        assert_eq!(error.code(), "github_offline");
        assert!(error.to_string().contains(REPO), "{error}");
        Ok(())
    }

    #[test]
    fn a_server_that_does_not_answer_times_out() -> TestResult {
        let fake = FakeGitHub::start()?;
        fake.route(
            &format!("/repos/{REPO}/contents/catalog.json"),
            FakeResponse::ok(CATALOG).delayed(Duration::from_secs(3)),
        );
        let options = ClientOptions { timeout: Duration::from_millis(200), ..options() };
        let result = client_with(&fake, options)?.catalog(REPO);
        let error = HubError::from(result.err().ok_or("refused")?);
        assert_eq!(error.code(), "github_timeout");
        assert!(error.to_string().contains(REPO), "{error}");
        Ok(())
    }

    #[test]
    fn json_over_its_limit_is_refused_while_it_streams() -> TestResult {
        let fake = FakeGitHub::start()?;
        let options = ClientOptions { max_json_bytes: 1024, ..options() };
        let client = client_with(&fake, options)?;
        let path = format!("/repos/{REPO}/contents/catalog.json");
        // No declared length: only counting the bytes can stop it.
        fake.route(&path, FakeResponse::ok(vec![b' '; 4096]).without_length());
        assert_eq!(code(client.catalog(REPO)), Some("github_too_large"));
        // A declared length over the limit is refused before the body.
        fake.route(&path, FakeResponse::ok(CATALOG).claiming_length(1025));
        assert_eq!(code(client.catalog(REPO)), Some("github_too_large"));
        // Exactly at the limit is accepted.
        let mut padded = CATALOG.as_bytes().to_vec();
        padded.resize(1024, b' ');
        fake.route(&path, FakeResponse::ok(padded));
        assert!(client.catalog(REPO).is_ok());
        Ok(())
    }

    #[test]
    fn an_asset_over_the_download_limit_is_not_requested() -> TestResult {
        let fake = FakeGitHub::start()?;
        fake.route("/asset", FakeResponse::ok("hello"));
        let options = ClientOptions { max_asset_bytes: 4, ..options() };
        let url = format!("{}/asset", fake.base_url());
        let mut out = Vec::new();
        let result = client_with(&fake, options)?.download(REPO, &asset_at(&url, 5), &mut out);
        assert_eq!(code(result), Some("github_too_large"));
        assert!(fake.requests().is_empty(), "{:?}", fake.requests());
        Ok(())
    }

    #[test]
    fn an_asset_of_another_size_than_listed_is_refused() -> TestResult {
        let fake = FakeGitHub::start()?;
        let client = client(&fake)?;
        let url = format!("{}/asset", fake.base_url());
        // More bytes than listed, with no declared length: stopped at the
        // first chunk over, which is never written.
        fake.route("/asset", FakeResponse::ok(vec![7; 3 * CHUNK_BYTES]).without_length());
        let mut out = Vec::new();
        let result = client.download(REPO, &asset_at(&url, 10), &mut out);
        assert_eq!(code(result), Some("github_size_mismatch"));
        assert!(out.len() <= 10, "wrote {} bytes", out.len());
        // A declared length that differs from the listed size.
        fake.route("/asset", FakeResponse::ok("hello").claiming_length(15));
        let result = client.download(REPO, &asset_at(&url, 5), &mut Vec::new());
        assert_eq!(code(result), Some("github_size_mismatch"));
        // Fewer bytes than listed.
        fake.route("/asset", FakeResponse::ok("hell").without_length());
        let result = client.download(REPO, &asset_at(&url, 5), &mut Vec::new());
        assert_eq!(code(result), Some("github_size_mismatch"));
        Ok(())
    }

    #[test]
    fn a_release_file_over_its_limit_is_refused() -> TestResult {
        let fake = fake_with_release()?;
        let client = client(&fake)?;
        let release = client.latest_release(REPO)?;
        let result = client.release_file(REPO, &release, SUMS_FILE, 3);
        assert_eq!(code(result), Some("github_too_large"));
        let sums = client.release_file(REPO, &release, SUMS_FILE, 4)?;
        assert_eq!(sums.as_deref(), Some(&b"sums"[..]));
        assert_eq!(client.release_file(REPO, &release, "other.txt", 4)?, None);
        Ok(())
    }

    #[test]
    fn redirects_away_from_the_endpoint_are_refused_unrequested() -> TestResult {
        let fake = FakeGitHub::start()?;
        let other = FakeGitHub::start()?;
        other.route("/asset", FakeResponse::ok("hello"));
        let client = client(&fake)?;
        let url = format!("{}/asset", fake.base_url());
        for location in [
            "https://example.com/asset".to_owned(),
            format!("{}/asset", other.base_url()),
            format!("http://user@{}/asset", fake.address()),
            "//example.com/asset".to_owned(),
            "asset".to_owned(),
        ] {
            fake.route("/asset", FakeResponse::redirect(&location));
            let result = client.download(REPO, &asset_at(&url, 5), &mut Vec::new());
            assert_eq!(code(result), Some("github_forbidden_url"), "{location}");
        }
        assert!(other.requests().is_empty(), "{:?}", other.requests());
        // A download URL off the endpoint is refused the same way.
        let off = asset_at(&format!("{}/asset", other.base_url()), 5);
        assert_eq!(
            code(client.download(REPO, &off, &mut Vec::new())),
            Some("github_forbidden_url")
        );
        assert!(other.requests().is_empty(), "{:?}", other.requests());
        Ok(())
    }

    #[test]
    fn redirects_are_bounded() -> TestResult {
        let fake = FakeGitHub::start()?;
        let client = client(&fake)?;
        let url = format!("{}/loop", fake.base_url());
        fake.route("/loop", FakeResponse::redirect(&url));
        let result = client.download(REPO, &asset_at(&url, 5), &mut Vec::new());
        assert_eq!(code(result), Some("github_bad_redirect"));
        assert_eq!(fake.requests().len(), MAX_REDIRECTS + 1);
        // A redirect without a location.
        fake.route("/loop", FakeResponse::new(302, ""));
        let result = client.download(REPO, &asset_at(&url, 5), &mut Vec::new());
        assert_eq!(code(result), Some("github_bad_redirect"));
        Ok(())
    }

    #[test]
    fn invalid_repositories_are_refused_before_any_request() -> TestResult {
        let fake = fake_with_release()?;
        let client = client(&fake)?;
        let release = client.latest_release(REPO)?;
        let asset = release.asset(ASSET).ok_or("asset listed")?;
        let requests = fake.requests().len();
        for repo in
            ["lab", "lab/signal-lab/releases", "../etc", "https://github.com/lab/x", "lab/.."]
        {
            assert_eq!(code(client.latest_release(repo)), Some("github_invalid_repo"), "{repo}");
            assert_eq!(code(client.catalog(repo)), Some("github_invalid_repo"), "{repo}");
            assert_eq!(code(client.checksum_files(repo, &release)), Some("github_invalid_repo"));
            let result = client.download(repo, asset, &mut Vec::new());
            assert_eq!(code(result), Some("github_invalid_repo"), "{repo}");
        }
        assert_eq!(fake.requests().len(), requests);
        let error = HubError::from(GitHubError::InvalidRepo("lab".to_owned()));
        assert_eq!(error.code(), "github_invalid_repo");
        Ok(())
    }

    #[test]
    fn a_failed_write_is_reported() -> TestResult {
        struct Full;
        impl Write for Full {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::other("disk full"))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let fake = fake_with_release()?;
        let client = client(&fake)?;
        let release = client.latest_release(REPO)?;
        let asset = release.asset(ASSET).ok_or("asset listed")?;
        assert_eq!(code(client.download(REPO, asset, &mut Full)), Some("github_write_failed"));
        Ok(())
    }

    #[test]
    fn github_is_reached_over_https_on_its_own_hosts_only() -> TestResult {
        let github = Endpoint::GitHub;
        for url in [
            "https://api.github.com/repos/lab/signal-lab/releases/latest",
            "https://github.com/lab/signal-lab/releases/download/v1/x",
            "https://objects.githubusercontent.com/github-production-release-asset/1",
            "https://release-assets.githubusercontent.com/github-production-release-asset/1",
            "https://API.GitHub.com:443/repos/lab/signal-lab",
        ] {
            assert!(github.allows(&url.parse()?), "{url}");
        }
        for url in [
            "http://api.github.com/repos/lab/signal-lab",
            "https://api.github.com:8443/repos/lab/signal-lab",
            "https://evil.example/repos/lab/signal-lab",
            "https://api.github.com.evil.example/",
            "https://api.github.com@evil.example/",
            "https://user@api.github.com/",
            "https://raw.githubusercontent.com/lab/catalog/main/catalog.json",
            "https://127.0.0.1/",
            "/repos/lab/signal-lab",
        ] {
            assert!(!github.allows(&url.parse()?), "{url}");
        }
        Ok(())
    }

    #[test]
    fn the_github_client_requests_nothing_from_loopback() -> TestResult {
        let fake = fake_with_release()?;
        let client = GitHubClient::with_options(Endpoint::GitHub, options())?;
        let url = format!("{}/{REPO}/releases/download/v1.2.0/{ASSET}", fake.base_url());
        let result = client.download(REPO, &asset_at(&url, 5), &mut Vec::new());
        assert_eq!(code(result), Some("github_forbidden_url"));
        assert!(fake.requests().is_empty(), "{:?}", fake.requests());
        Ok(())
    }

    #[test]
    fn the_override_names_a_loopback_server_only() {
        for (text, host, port) in
            [("http://127.0.0.1:8080", "127.0.0.1", 8080), ("http://localhost:1/", "localhost", 1)]
        {
            let expected = Endpoint::Loopback { host: host.to_owned(), port };
            assert_eq!(Endpoint::loopback(text), Some(expected.clone()), "{text}");
            assert_eq!(Endpoint::from_override(Some(text)), expected, "{text}");
        }
        for text in [
            "https://127.0.0.1:8080",
            "http://127.0.0.1",
            "http://127.0.0.1:",
            "http://127.0.0.1:0",
            "http://127.0.0.1:65536",
            "http://127.0.0.1:+80",
            "http://127.0.0.1:8080/api",
            "http://10.0.0.1:8080",
            "http://example.com:8080",
            "http://[::1]:8080",
            "http://user@127.0.0.1:8080",
            "https://api.github.com",
            "",
        ] {
            assert_eq!(Endpoint::loopback(text), None, "{text}");
            assert_eq!(Endpoint::from_override(Some(text)), Endpoint::GitHub, "{text}");
        }
        assert_eq!(Endpoint::from_override(None), Endpoint::GitHub);
    }

    #[test]
    fn the_loopback_endpoint_replaces_every_github_host() -> TestResult {
        let endpoint = Endpoint::loopback("http://127.0.0.1:8080").ok_or("loopback")?;
        assert!(endpoint.allows(&"http://127.0.0.1:8080/repos/lab/x".parse()?));
        for url in [
            "https://api.github.com/repos/lab/x",
            "https://github.com/lab/x",
            "http://127.0.0.1:8081/repos/lab/x",
            "https://127.0.0.1:8080/repos/lab/x",
            "http://localhost:8080/repos/lab/x",
        ] {
            assert!(!endpoint.allows(&url.parse()?), "{url}");
        }
        Ok(())
    }
}
