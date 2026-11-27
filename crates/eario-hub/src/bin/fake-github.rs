//! A fake GitHub as a process, for the browser tests of the hub
//! (`cargo xtask e2e`). Built only with the feature `fake-github`, which no
//! release of the hub enables.
//!
//! Usage: `fake-github [<directory>]`.
//!
//! It serves, on the loopback interface, a scripted scenario through
//! [`FakeGitHub`]:
//!
//! - the default catalog (`rtonneau/eario-catalog`), listing
//!   `lab/data-explorer` with the key that signs its releases;
//! - the catalog `lab/catalog`, listing `lab/signal-lab`, for a test to add;
//! - with `<directory>`, the directory of the example executables built by
//!   `cargo xtask e2e`: the latest release `v0.0.0` of `lab/data-explorer`
//!   (data-explorer, signed with the catalog's key) and of `lab/signal-lab`
//!   (signal-lab, signed with a key no catalog lists). Each release carries
//!   its `minisign.pub`. Without it, only the catalogs are served: enough for
//!   a hub that must not reach the real GitHub.
//!
//! The keys come from fixed test seeds: they are never real keys.
//!
//! It prints one line, `fake GitHub is running at http://127.0.0.1:<port>,
//! control at http://127.0.0.1:<port>`, and serves until its standard input
//! closes, so it never outlives the test that started it.
//!
//! The control address, a second listener of this process (never the hub),
//! takes `POST` requests that change the scenario:
//!
//! | Path | Change |
//! | --- | --- |
//! | `/data-explorer/newer` | a new build of data-explorer, signed with the same key |
//! | `/data-explorer/new-key` | a new build of data-explorer, signed with another key |
//! | `/data-explorer/offline` | GitHub refuses the release of data-explorer (rate limit) |
//!
//! A new build is the same executable with a few bytes appended (it still
//! runs), so it is another file with the same manifest, version `0.0.0`. A
//! test that updates sets the installed version back first, as the
//! integration tests of this crate do.

use std::error::Error;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::Path;
use std::process::ExitCode;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use eario_hub::fake_github::{FakeAsset, FakeGitHub, FakeResponse, TestKey};
use eario_hub::remote::DEFAULT_CATALOG;
use eario_hub::verify::{self, Target};

type Failure = Box<dyn Error>;

/// Repository data-explorer is published from; the default catalog lists it.
const DATA_EXPLORER_REPO: &str = "lab/data-explorer";
/// Repository signal-lab is published from; no default catalog lists it.
const SIGNAL_LAB_REPO: &str = "lab/signal-lab";
/// A catalog that is not in the hub's list until a test adds it.
const LAB_CATALOG: &str = "lab/catalog";
/// Tag of every release served.
const TAG: &str = "v0.0.0";
/// Largest request head the control listener reads, in bytes.
const MAX_HEAD_BYTES: usize = 16 * 1024;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("fake-github: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Failure> {
    let directory = std::env::args_os().nth(1);
    let fake = FakeGitHub::start()?;
    let control = TcpListener::bind(("127.0.0.1", 0))?;
    let scenario = Scenario::new(fake, directory.as_deref().map(Path::new))?;
    println!(
        "fake GitHub is running at {}, control at http://{}",
        scenario.fake.base_url(),
        control.local_addr()?
    );
    io::stdout().flush()?;
    let scenario = Arc::new(Mutex::new(scenario));
    {
        let scenario = Arc::clone(&scenario);
        thread::spawn(move || serve_control(&control, &scenario));
    }
    // The test that started this process has ended once its end of the pipe
    // is closed. Returning ends the process, with its other threads.
    io::copy(&mut io::stdin().lock(), &mut io::sink())?;
    Ok(())
}

/// A test signing key; `seed` tells keys, and their key ids, apart.
fn test_key(seed: u8) -> io::Result<TestKey> {
    TestKey::new([seed; 32], [b'e', b'2', b'e', 0, 0, 0, 0, seed])
}

/// The published data-explorer: the executable each build starts from, its
/// asset name, and how many builds were published.
struct DataExplorer {
    executable: Vec<u8>,
    asset: String,
    builds: u32,
}

/// What the fake serves, and what the control requests change.
struct Scenario {
    fake: FakeGitHub,
    catalog_key: TestKey,
    new_key: TestKey,
    data_explorer: Option<DataExplorer>,
}

impl Scenario {
    fn new(fake: FakeGitHub, directory: Option<&Path>) -> Result<Self, Failure> {
        let catalog_key = test_key(0x41)?;
        let signal_lab_key = test_key(0x42)?;
        let new_key = test_key(0x43)?;
        serve_catalog(
            &fake,
            DEFAULT_CATALOG,
            "Example catalog",
            &[(DATA_EXPLORER_REPO, "Data Explorer", "Explore a table", &catalog_key)],
        );
        serve_catalog(
            &fake,
            LAB_CATALOG,
            "Lab catalog",
            &[(SIGNAL_LAB_REPO, "Signal Lab", "Signals and their spectra", &signal_lab_key)],
        );
        let mut scenario = Self { fake, catalog_key, new_key, data_explorer: None };
        if let Some(directory) = directory {
            let target = Target::CURRENT.ok_or("no release asset name for this machine")?;
            let read = |name: &str| -> Result<Vec<u8>, Failure> {
                let path = directory.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
                std::fs::read(&path)
                    .map_err(|error| format!("cannot read {}: {error}", path.display()).into())
            };
            let signal_lab =
                FakeAsset::new(&verify::asset_name("signal-lab", target)?, read("signal-lab")?);
            publish(&scenario.fake, SIGNAL_LAB_REPO, &signal_lab_key, signal_lab);
            scenario.data_explorer = Some(DataExplorer {
                executable: read("data-explorer")?,
                asset: verify::asset_name("data-explorer", target)?,
                builds: 0,
            });
            scenario.publish_data_explorer(false)?;
        }
        Ok(scenario)
    }

    /// Publishes the next build of data-explorer, signed with the catalog's
    /// key or, when `new_key`, with another.
    fn publish_data_explorer(&mut self, new_key: bool) -> Result<(), Failure> {
        let published = self.data_explorer.as_mut().ok_or("started without the executables")?;
        published.builds += 1;
        let mut content = published.executable.clone();
        content.extend_from_slice(format!("build {}", published.builds).as_bytes());
        let key = if new_key { &self.new_key } else { &self.catalog_key };
        publish(&self.fake, DATA_EXPLORER_REPO, key, FakeAsset::new(&published.asset, content));
        Ok(())
    }

    /// Applies the control request for `path`; false for an unknown one.
    fn control(&mut self, path: &str) -> Result<bool, Failure> {
        match path {
            "/data-explorer/newer" => self.publish_data_explorer(false)?,
            "/data-explorer/new-key" => self.publish_data_explorer(true)?,
            "/data-explorer/offline" => self.fake.route(
                &format!("/repos/{DATA_EXPLORER_REPO}/releases/latest"),
                FakeResponse::rate_limited(),
            ),
            _ => return Ok(false),
        }
        Ok(true)
    }
}

/// Serves a catalog at `repo` listing `entries` (repository, name,
/// description, key).
fn serve_catalog(
    fake: &FakeGitHub,
    repo: &str,
    name: &str,
    entries: &[(&str, &str, &str, &TestKey)],
) {
    let dashboards: Vec<serde_json::Value> = entries
        .iter()
        .map(|(repo, name, description, key)| {
            serde_json::json!({
                "repo": repo, "name": name, "description": description,
                "public_key": key.public_key(),
            })
        })
        .collect();
    let catalog =
        serde_json::json!({ "format_version": 1, "name": name, "dashboards": dashboards });
    fake.catalog(repo, catalog.to_string());
}

/// Serves `asset` as the latest release of `repo`, with its checksum list
/// signed by `key` and `key`'s `minisign.pub`.
fn publish(fake: &FakeGitHub, repo: &str, key: &TestKey, asset: FakeAsset) {
    let signed = key.release_files(std::slice::from_ref(&asset));
    let mut files = vec![asset];
    files.extend(signed);
    fake.release(repo, TAG, &files);
}

/// Answers the control requests, one connection at a time.
fn serve_control(listener: &TcpListener, scenario: &Mutex<Scenario>) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            continue;
        };
        if let Err(error) = answer_control(stream, scenario) {
            eprintln!("fake-github: control request failed: {error}");
        }
    }
}

fn answer_control(mut stream: TcpStream, scenario: &Mutex<Scenario>) -> Result<(), Failure> {
    let (method, path) = read_request_line(&stream)?;
    let (status, body) = if method != "POST" {
        (405, "only POST".to_owned())
    } else {
        // A request that panicked while holding the lock left valid data.
        let mut scenario = scenario.lock().unwrap_or_else(PoisonError::into_inner);
        match scenario.control(&path) {
            Ok(true) => (200, "ok".to_owned()),
            Ok(false) => (404, format!("unknown control {path}")),
            Err(error) => (500, error.to_string()),
        }
    };
    write!(
        stream,
        "HTTP/1.1 {status} Fake\r\nconnection: close\r\ncontent-type: text/plain\r\ncontent-length: {}\r\n\r\n{body}",
        body.len()
    )?;
    stream.flush()?;
    stream.shutdown(Shutdown::Write)?;
    Ok(())
}

/// The method and path of a request, once its head is read.
fn read_request_line(stream: &TcpStream) -> io::Result<(String, String)> {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut read = request_line.len();
    // Reads the headers to their end, so the client is not reset. The
    // control requests have no body.
    loop {
        let mut line = String::new();
        let bytes = reader.read_line(&mut line)?;
        read += bytes;
        if bytes == 0 || line == "\r\n" || line == "\n" || read > MAX_HEAD_BYTES {
            break;
        }
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let target = parts.next().unwrap_or_default();
    let path = target.split('?').next().unwrap_or_default().to_owned();
    Ok((method, path))
}
