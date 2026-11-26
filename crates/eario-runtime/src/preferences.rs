//! User preferences stored on the local machine.
//!
//! Preferences live in one small JSON file. A missing or unreadable file
//! yields the defaults; it never prevents an application from starting.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use directories::ProjectDirs;
use eario_core::Preferences;
use serde::{Deserialize, Serialize};

use crate::error::AppError;

/// Version of the preferences file format.
const FORMAT_VERSION: u32 = 1;

/// Content of the preferences file.
#[derive(Debug, Serialize, Deserialize)]
struct StoredPreferences {
    version: u32,
    #[serde(flatten)]
    preferences: Preferences,
}

/// Returns the default preferences file of an application.
///
/// The file is `preferences.json` in a directory named after the application,
/// inside the Eario directory of the per-user configuration location of the
/// platform. Returns `None` when the platform reports no home directory.
pub fn default_path(app_name: &str) -> Option<PathBuf> {
    let directories = ProjectDirs::from("", "", "eario")?;
    Some(directories.config_dir().join(slug(app_name)).join("preferences.json"))
}

/// Reduces an application name to lower-case letters, digits and dashes.
fn slug(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() { "app".to_owned() } else { slug.to_owned() }
}

/// Holds the current preferences and writes them through to disk.
#[derive(Debug)]
pub(crate) struct PreferenceStore {
    path: Option<PathBuf>,
    current: Mutex<Preferences>,
}

impl PreferenceStore {
    /// Opens the store. With no path, preferences live in memory only.
    pub(crate) fn open(path: Option<PathBuf>) -> Self {
        let current = path.as_deref().map(load).unwrap_or_default();
        Self { path, current: Mutex::new(current) }
    }

    pub(crate) fn get(&self) -> Preferences {
        *self.current.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn set(&self, preferences: Preferences) -> Result<(), AppError> {
        let mut current = self.current.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(path) = &self.path {
            save(path, preferences)
                .map_err(|source| AppError::Preferences { path: path.clone(), source })?;
        }
        *current = preferences;
        Ok(())
    }
}

fn load(path: &Path) -> Preferences {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Preferences::default(),
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "could not read preferences, using defaults");
            return Preferences::default();
        }
    };
    match serde_json::from_str::<StoredPreferences>(&text) {
        Ok(stored) if stored.version <= FORMAT_VERSION => stored.preferences,
        Ok(stored) => {
            tracing::warn!(
                path = %path.display(),
                version = stored.version,
                "preferences were written by a newer version, using defaults"
            );
            Preferences::default()
        }
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "preferences are not valid, using defaults");
            Preferences::default()
        }
    }
}

/// Writes the file next to its destination, then renames it over it, so an
/// interrupted write leaves the previous file intact.
fn save(path: &Path, preferences: Preferences) -> io::Result<()> {
    if let Some(directory) = path.parent() {
        fs::create_dir_all(directory)?;
    }
    let stored = StoredPreferences { version: FORMAT_VERSION, preferences };
    let text = serde_json::to_string_pretty(&stored).map_err(io::Error::other)?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, text)?;
    fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use eario_core::Theme;

    use super::*;

    #[test]
    fn slug_keeps_letters_and_digits_and_collapses_the_rest() {
        assert_eq!(slug("Data Explorer"), "data-explorer");
        assert_eq!(slug("  RBS / PIXE (v2) "), "rbs-pixe-v2");
        assert_eq!(slug("../.."), "app");
    }

    #[test]
    fn preferences_survive_a_restart() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("nested").join("preferences.json");

        let store = PreferenceStore::open(Some(path.clone()));
        assert_eq!(store.get().theme, Theme::System);
        store.set(Preferences { theme: Theme::Dark })?;

        let reopened = PreferenceStore::open(Some(path));
        assert_eq!(reopened.get().theme, Theme::Dark);
        Ok(())
    }

    #[test]
    fn corrupt_file_falls_back_to_defaults() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("preferences.json");
        fs::write(&path, "{ not json")?;
        assert_eq!(PreferenceStore::open(Some(path)).get(), Preferences::default());
        Ok(())
    }

    #[test]
    fn store_without_a_path_keeps_preferences_in_memory() -> Result<(), AppError> {
        let store = PreferenceStore::open(None);
        store.set(Preferences { theme: Theme::Light })?;
        assert_eq!(store.get().theme, Theme::Light);
        Ok(())
    }
}
