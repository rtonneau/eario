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
//!
//! [`TestKey`] signs a release's checksum list as the minisign tool does, so
//! a test can publish a release the hub verifies. Its secret key comes from
//! a seed the test gives: it is never a real key.

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use ring::signature::{Ed25519KeyPair, KeyPair};
use sha2::{Digest, Sha256};

use crate::verify::{KEY_FILE, SIGNATURE_FILE, SUMS_FILE};

/// Largest request head read, in bytes.
const MAX_HEAD_BYTES: usize = 16 * 1024;

/// One answer of the fake server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeResponse {
    status: u16,
    headers: Vec<(String, String)>,
    /// Shared, so answering a large asset copies no bytes.
    body: Arc<Vec<u8>>,
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
        let body = Arc::new(body.into());
        Self { status, headers: Vec::new(), body, length: Length::Actual, delay: None }
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

/// A minisign key pair for tests. It signs as `minisign -S` does (the
/// BLAKE2b-512 of the file, then a global signature over that signature and
/// the trusted comment), so [`crate::verify::verify_sums`] accepts what it
/// signs.
pub struct TestKey {
    pair: Ed25519KeyPair,
    key_id: [u8; 8],
}

impl TestKey {
    /// The key whose Ed25519 secret key is `seed` and whose minisign key id is
    /// `key_id`. Any seed is a valid key.
    pub fn new(seed: [u8; 32], key_id: [u8; 8]) -> io::Result<Self> {
        let pair = Ed25519KeyPair::from_seed_unchecked(&seed)
            .map_err(|error| io::Error::other(error.to_string()))?;
        Ok(Self { pair, key_id })
    }

    /// The key line of its `.pub` file, as catalogs and the registry hold it.
    pub fn public_key(&self) -> String {
        let mut bytes = Vec::with_capacity(42);
        bytes.extend_from_slice(b"Ed");
        bytes.extend_from_slice(&self.key_id);
        bytes.extend_from_slice(self.pair.public_key().as_ref());
        base64(&bytes)
    }

    /// Its `.pub` file, as minisign writes it.
    pub fn pub_file(&self) -> String {
        let id: String = self.key_id.iter().rev().map(|byte| format!("{byte:02X}")).collect();
        format!("untrusted comment: minisign public key {id}\n{}\n", self.public_key())
    }

    /// The minisign signature of `content`, as `minisign -S` writes it.
    pub fn sign(&self, content: &[u8]) -> String {
        let signature = self.pair.sign(&blake2b_512(content));
        let mut line = Vec::with_capacity(74);
        line.extend_from_slice(b"ED");
        line.extend_from_slice(&self.key_id);
        line.extend_from_slice(signature.as_ref());
        let trusted = format!("timestamp:1764230400\tfile:{SUMS_FILE}\thashed");
        let mut global = signature.as_ref().to_vec();
        global.extend_from_slice(trusted.as_bytes());
        let global = self.pair.sign(&global);
        format!(
            "untrusted comment: signature from a test key\n{}\ntrusted comment: {trusted}\n{}\n",
            base64(&line),
            base64(global.as_ref())
        )
    }

    /// What makes `assets` a signed release: their [`SUMS_FILE`], its
    /// [`SIGNATURE_FILE`] by this key, and this key's [`KEY_FILE`].
    pub fn release_files(&self, assets: &[FakeAsset]) -> [FakeAsset; 3] {
        let sums = sums_file(assets);
        [
            FakeAsset::new(SIGNATURE_FILE, self.sign(sums.as_bytes())),
            FakeAsset::new(SUMS_FILE, sums),
            FakeAsset::new(KEY_FILE, self.pub_file()),
        ]
    }
}

/// The [`SUMS_FILE`] of `assets`, as `sha256sum` writes it.
pub fn sums_file(assets: &[FakeAsset]) -> String {
    assets
        .iter()
        .map(|asset| {
            let sha256: String =
                Sha256::digest(&asset.content).iter().map(|byte| format!("{byte:02x}")).collect();
            format!("{sha256}  {}\n", asset.name)
        })
        .collect()
}

/// Standard base64, with padding.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let byte = |index: usize| u32::from(chunk.get(index).copied().unwrap_or(0));
        let word = (byte(0) << 16) | (byte(1) << 8) | byte(2);
        for position in 0..4 {
            if position <= chunk.len() {
                let index = (word >> (18 - 6 * position)) & 63;
                text.push(char::from(ALPHABET[index as usize]));
            } else {
                text.push('=');
            }
        }
    }
    text
}

/// Initialization vector of BLAKE2b (RFC 7693).
const BLAKE2B_IV: [u64; 8] = [
    0x6a09_e667_f3bc_c908,
    0xbb67_ae85_84ca_a73b,
    0x3c6e_f372_fe94_f82b,
    0xa54f_f53a_5f1d_36f1,
    0x510e_527f_ade6_82d1,
    0x9b05_688c_2b3e_6c1f,
    0x1f83_d9ab_fb41_bd6b,
    0x5be0_cd19_137e_2179,
];

/// Message word schedule of the twelve rounds of BLAKE2b (RFC 7693).
const BLAKE2B_SIGMA: [[usize; 16]; 12] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
    [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
    [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
    [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
    [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
    [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
    [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
    [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
    [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
];

/// BLAKE2b with a 64-byte digest and no key (RFC 7693), what minisign hashes
/// a file with before signing it. minisign-verify has its own, not public.
fn blake2b_512(input: &[u8]) -> [u8; 64] {
    let mut state = BLAKE2B_IV;
    // Parameter block: digest length 64, no key, fan-out and depth 1.
    state[0] ^= 0x0101_0000 ^ 64;
    let mut counter = 0_u128;
    let mut blocks = input.chunks(128).peekable();
    if blocks.peek().is_none() {
        blake2b_compress(&mut state, &[0; 128], 0, true);
    }
    while let Some(chunk) = blocks.next() {
        counter += chunk.len() as u128;
        let mut block = [0_u8; 128];
        block[..chunk.len()].copy_from_slice(chunk);
        blake2b_compress(&mut state, &block, counter, blocks.peek().is_none());
    }
    let mut digest = [0_u8; 64];
    for (bytes, word) in digest.chunks_mut(8).zip(state) {
        bytes.copy_from_slice(&word.to_le_bytes());
    }
    digest
}

fn blake2b_compress(state: &mut [u64; 8], block: &[u8; 128], counter: u128, last: bool) {
    let mut message = [0_u64; 16];
    for (word, bytes) in message.iter_mut().zip(block.chunks(8)) {
        let mut le = [0_u8; 8];
        le.copy_from_slice(bytes);
        *word = u64::from_le_bytes(le);
    }
    let mut v = [0_u64; 16];
    v[..8].copy_from_slice(state);
    v[8..].copy_from_slice(&BLAKE2B_IV);
    // The counter is split into its low and high 64 bits.
    v[12] ^= counter as u64;
    v[13] ^= (counter >> 64) as u64;
    if last {
        v[14] = !v[14];
    }
    for sigma in BLAKE2B_SIGMA {
        let m = |index: usize| message[sigma[index]];
        blake2b_mix(&mut v, [0, 4, 8, 12], m(0), m(1));
        blake2b_mix(&mut v, [1, 5, 9, 13], m(2), m(3));
        blake2b_mix(&mut v, [2, 6, 10, 14], m(4), m(5));
        blake2b_mix(&mut v, [3, 7, 11, 15], m(6), m(7));
        blake2b_mix(&mut v, [0, 5, 10, 15], m(8), m(9));
        blake2b_mix(&mut v, [1, 6, 11, 12], m(10), m(11));
        blake2b_mix(&mut v, [2, 7, 8, 13], m(12), m(13));
        blake2b_mix(&mut v, [3, 4, 9, 14], m(14), m(15));
    }
    for (index, word) in state.iter_mut().enumerate() {
        *word ^= v[index] ^ v[index + 8];
    }
}

/// The mixing function G of BLAKE2b.
fn blake2b_mix(v: &mut [u64; 16], [a, b, c, d]: [usize; 4], x: u64, y: u64) {
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(x);
    v[d] = (v[d] ^ v[a]).rotate_right(32);
    v[c] = v[c].wrapping_add(v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(24);
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(y);
    v[d] = (v[d] ^ v[a]).rotate_right(16);
    v[c] = v[c].wrapping_add(v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(63);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verify::{PublicKey, verify_sums};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn blake2b_gives_the_digests_of_rfc_7693() {
        // RFC 7693, appendix A, and the BLAKE2 reference vectors.
        assert_eq!(
            hex(&blake2b_512(b"abc")),
            "ba80a53f981c4d0d6a2797b69f12f6e94c212f14685ac4b74b12bb6fdbffa2d1\
             7d87c5392aab792dc252d5de4533cc9518d38aa8dbf1925ab92386edd4009923"
        );
        assert_eq!(
            hex(&blake2b_512(b"")),
            "786a02f742015903c6c6fd852552d272912f4740e15847618a86e217f71f5419\
             d25e1031afee585313896444934eb04b903a685b1448b755d56f701afe9be2ce"
        );
        // Exactly one block, then one byte more: the last-block flag and the
        // counter.
        assert_ne!(blake2b_512(&[7; 128]), blake2b_512(&[7; 129]));
    }

    #[test]
    fn base64_pads_as_the_standard_says() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn what_a_test_key_signs_verifies() -> TestResult {
        let key = TestKey::new([3; 32], *b"testkey1")?;
        let assets = [FakeAsset::new("a-x86_64-unknown-linux-gnu", "hello")];
        let [signature, sums, pub_file] = key.release_files(&assets);
        let public = PublicKey::from_pub_file("o/r", &pub_file.content)?;
        assert_eq!(public.as_base64(), key.public_key());
        assert_eq!(public.fingerprint(), "3179656B74736574");
        let checksums = verify_sums("o/r", &public, &sums.content, Some(&signature.content))?;
        assert_eq!(
            checksums.sha256("a-x86_64-unknown-linux-gnu")?,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        // Another key's signature does not verify with this one.
        let other = TestKey::new([4; 32], *b"testkey1")?;
        let forged = other.sign(&sums.content);
        let refused = verify_sums("o/r", &public, &sums.content, Some(forged.as_bytes()));
        assert_eq!(refused.err().map(|error| error.code()), Some("release_bad_signature"));
        Ok(())
    }
}
