//! A fake GitHub on the loopback interface, for tests only (feature
//! `fake-github`, always on in this crate's tests).
//!
//! A [`FakeGitHub`] answers plain HTTP on `127.0.0.1` with the answers it was
//! given, by path: releases, catalogs, assets, any status. Point a
//! [`GitHubClient`](crate::github::GitHubClient) at it with
//! [`Endpoint::loopback`](crate::github::Endpoint::loopback) of
//! [`FakeGitHub::base_url`], or a debug build of the hub with
//! `EARIO_HUB_GITHUB_API`.
//!
//! [`FakeGitHub::release`] serves a release as GitHub does: the API lists
//! each asset with a download URL that redirects once to the file. It is a
//! test tool: one thread per connection, one request per connection, no
//! keep-alive.

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Largest request head read, in bytes.
const MAX_HEAD_BYTES: usize = 16 * 1024;

/// One answer of the fake server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    length: Length,
    delay: Option<Duration>,
}

/// What the `content-length` header says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Length {
    /// The length of the body.
    Actual,
    /// No header: the body ends when the connection closes.
    Omitted,
    /// This number, whatever the body.
    Claimed(u64),
}

impl FakeResponse {
    /// A `status` answer with `body`.
    pub fn new(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self { status, headers: Vec::new(), body: body.into(), length: Length::Actual, delay: None }
    }

    /// A `200` answer with `body`.
    pub fn ok(body: impl Into<Vec<u8>>) -> Self {
        Self::new(200, body)
    }

    /// A `302` redirect to `location`.
    pub fn redirect(location: &str) -> Self {
        Self::new(302, Vec::new()).header("location", location)
    }

    /// GitHub's answer once the unauthenticated rate limit is used up.
    pub fn rate_limited() -> Self {
        Self::new(403, r#"{"message":"API rate limit exceeded"}"#)
            .header("x-ratelimit-limit", "60")
            .header("x-ratelimit-remaining", "0")
    }

    /// The answer with one more header.
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }

    /// The answer without `content-length`: its body ends with the
    /// connection, so only the bytes read can bound it.
    pub fn without_length(mut self) -> Self {
        self.length = Length::Omitted;
        self
    }

    /// The answer with `content-length: length`, whatever its body.
    pub fn claiming_length(mut self, length: u64) -> Self {
        self.length = Length::Claimed(length);
        self
    }

    /// The answer sent only after `delay`, to test timeouts.
    pub fn delayed(mut self, delay: Duration) -> Self {
        self.delay = Some(delay);
        self
    }
}

/// A file of a fake release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeAsset {
    /// File name.
    pub name: String,
    /// Content.
    pub content: Vec<u8>,
}

impl FakeAsset {
    /// The asset `name` with `content`.
    pub fn new(name: &str, content: impl Into<Vec<u8>>) -> Self {
        Self { name: name.to_owned(), content: content.into() }
    }
}

/// What the server holds, shared with its threads.
#[derive(Default)]
struct State {
    routes: HashMap<String, FakeResponse>,
    requests: Vec<String>,
}

/// A fake GitHub listening on `127.0.0.1`. Dropping it stops it.
pub struct FakeGitHub {
    address: SocketAddr,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl FakeGitHub {
    /// Starts a server on a free port of `127.0.0.1`, answering `404` to
    /// everything until given routes.
    pub fn start() -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let address = listener.local_addr()?;
        let state = Arc::new(Mutex::new(State::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let state = Arc::clone(&state);
            let stop = Arc::clone(&stop);
            thread::spawn(move || accept(&listener, &state, &stop))
        };
        Ok(Self { address, state, stop, thread: Some(thread) })
    }

    /// Address of the server, for example `127.0.0.1:49152`.
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// `http://127.0.0.1:<port>`, the value of `EARIO_HUB_GITHUB_API`.
    pub fn base_url(&self) -> String {
        format!("http://{}", self.address)
    }

    /// Answers `response` to every `GET` of `path` (no query), replacing
    /// what it answered before.
    pub fn route(&self, path: &str, response: FakeResponse) {
        lock(&self.state).routes.insert(path.to_owned(), response);
    }

    /// Serves `assets` as the latest release `tag` of `repo`, as GitHub
    /// does: the API at `/repos/<repo>/releases/latest` lists them with a
    /// download URL under `/<repo>/releases/download/<tag>/`, which
    /// redirects to `/assets/<repo>/<tag>/<name>`, the file itself.
    pub fn release(&self, repo: &str, tag: &str, assets: &[FakeAsset]) {
        let base = self.base_url();
        let listed: Vec<serde_json::Value> = assets
            .iter()
            .map(|asset| {
                let download = format!("/{repo}/releases/download/{tag}/{}", asset.name);
                let file = format!("/assets/{repo}/{tag}/{}", asset.name);
                self.route(&download, FakeResponse::redirect(&format!("{base}{file}")));
                self.route(&file, FakeResponse::ok(asset.content.clone()));
                serde_json::json!({
                    "name": asset.name,
                    "size": asset.content.len(),
                    "browser_download_url": format!("{base}{download}"),
                })
            })
            .collect();
        let release = serde_json::json!({
            "tag_name": tag,
            "draft": false,
            "prerelease": false,
            "assets": listed,
        });
        self.route(
            &format!("/repos/{repo}/releases/latest"),
            FakeResponse::ok(release.to_string()).header("content-type", "application/json"),
        );
    }

    /// Serves `content` as the `catalog.json` of `repo`.
    pub fn catalog(&self, repo: &str, content: impl Into<Vec<u8>>) {
        self.route(&format!("/repos/{repo}/contents/catalog.json"), FakeResponse::ok(content));
    }

    /// Paths requested so far, in order, with their query.
    pub fn requests(&self) -> Vec<String> {
        lock(&self.state).requests.clone()
    }
}

impl Drop for FakeGitHub {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wakes the accept loop so it sees the flag. If the connection
        // fails, the loop is already gone.
        let _ = TcpStream::connect(self.address);
        if let Some(thread) = self.thread.take() {
            // A panicked accept loop has nothing left to clean up.
            let _ = thread.join();
        }
    }
}

fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    // A test thread that panicked while holding the lock left valid data.
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

fn accept(listener: &TcpListener, state: &Arc<Mutex<State>>, stop: &AtomicBool) {
    for stream in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let Ok(stream) = stream else {
            continue;
        };
        let state = Arc::clone(state);
        thread::spawn(move || {
            // The client may hang up first; nothing to report to anyone.
            let _ = serve(stream, &state);
        });
    }
}

/// Answers the one request of `stream`.
fn serve(mut stream: TcpStream, state: &Mutex<State>) -> io::Result<()> {
    let Some(target) = read_request_target(&stream)? else {
        return Ok(());
    };
    let path = target.split('?').next().unwrap_or_default().to_owned();
    let response = {
        let mut state = lock(state);
        state.requests.push(target);
        state.routes.get(&path).cloned()
    };
    let response = response.unwrap_or_else(|| FakeResponse::new(404, r#"{"message":"Not Found"}"#));
    if let Some(delay) = response.delay {
        thread::sleep(delay);
    }
    let mut head = format!("HTTP/1.1 {} Fake\r\nconnection: close\r\n", response.status);
    for (name, value) in &response.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    match response.length {
        Length::Actual => head.push_str(&format!("content-length: {}\r\n", response.body.len())),
        Length::Claimed(length) => head.push_str(&format!("content-length: {length}\r\n")),
        Length::Omitted => {}
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(&response.body)?;
    stream.flush()?;
    stream.shutdown(Shutdown::Write)
}

/// The target of a `GET` request (`/path?query`), once its head is read;
/// `None` for anything else.
fn read_request_target(stream: &TcpStream) -> io::Result<Option<String>> {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut read = request_line.len();
    // Reads the headers to their end, so the client is not reset.
    loop {
        let mut line = String::new();
        let bytes = reader.read_line(&mut line)?;
        read += bytes;
        if bytes == 0 || line == "\r\n" || line == "\n" || read > MAX_HEAD_BYTES {
            break;
        }
    }
    let mut parts = request_line.split_whitespace();
    match (parts.next(), parts.next()) {
        (Some("GET"), Some(target)) => Ok(Some(target.to_owned())),
        _ => Ok(None),
    }
}
