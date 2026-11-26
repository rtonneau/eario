//! A log file that rotates by size, for a dashboard started by a hub.
//!
//! When a write would take the file past its limit, the file is renamed with
//! `.1` appended, replacing the previous one, and a new file is started. One
//! old file is kept.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Size past which a child's log file is rotated: 1 MiB.
pub(crate) const LOG_FILE_LIMIT: u64 = 1024 * 1024;

/// An append-only log file rotated by size.
#[derive(Debug)]
pub(crate) struct RotatingFile {
    path: PathBuf,
    file: File,
    size: u64,
    limit: u64,
}

impl RotatingFile {
    /// Opens `path` for appending, creating it and its directory when
    /// missing. Output of earlier runs is kept and counts towards `limit`.
    pub(crate) fn open(path: &Path, limit: u64) -> io::Result<Self> {
        if let Some(directory) = path.parent() {
            fs::create_dir_all(directory)?;
        }
        let file = append(path)?;
        let size = file.metadata()?.len();
        Ok(Self { path: path.to_owned(), file, size, limit })
    }

    /// Renames the current file to its `.1` name and starts a new one.
    fn rotate(&mut self) -> io::Result<()> {
        let mut old = self.path.as_os_str().to_owned();
        old.push(".1");
        // The rename replaces an older `.1` file. The file stays open while it
        // is renamed, which the standard library allows on every platform.
        fs::rename(&self.path, PathBuf::from(old))?;
        self.file = append(&self.path)?;
        self.size = 0;
        Ok(())
    }
}

fn append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

impl Write for RotatingFile {
    /// Writes all of `buffer`, so a log line is never split between two files.
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let length = buffer.len() as u64;
        if self.size > 0 && self.size.saturating_add(length) > self.limit {
            // A failed rotation is not reported: losing log lines would be
            // worse than a file above its limit, and the next write retries.
            let _ = self.rotate();
        }
        self.file.write_all(buffer)?;
        self.size = self.size.saturating_add(length);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn rotates_past_the_limit_and_keeps_one_old_file() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("logs").join("alpha.log");
        let old = directory.path().join("logs").join("alpha.log.1");
        let mut log = RotatingFile::open(&path, 10)?;

        log.write_all(b"first\n")?;
        log.write_all(b"second\n")?;
        assert_eq!(fs::read_to_string(&old)?, "first\n");
        assert_eq!(fs::read_to_string(&path)?, "second\n");

        log.write_all(b"third\n")?;
        assert_eq!(fs::read_to_string(&old)?, "second\n", "only one old file is kept");
        assert_eq!(fs::read_to_string(&path)?, "third\n");
        Ok(())
    }

    #[test]
    fn a_line_longer_than_the_limit_is_written_whole() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("alpha.log");
        let mut log = RotatingFile::open(&path, 4)?;
        log.write_all(b"a long line\n")?;
        assert_eq!(fs::read_to_string(&path)?, "a long line\n");
        Ok(())
    }

    #[test]
    fn reopening_appends_and_counts_the_existing_size() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("alpha.log");
        fs::write(&path, "earlier run\n")?;
        let mut log = RotatingFile::open(&path, 20)?;
        log.write_all(b"now\n")?;
        assert_eq!(fs::read_to_string(&path)?, "earlier run\nnow\n");
        log.write_all(b"and later\n")?;
        assert_eq!(fs::read_to_string(&path)?, "and later\n");
        Ok(())
    }
}
