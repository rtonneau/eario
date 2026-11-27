//! The launch contract: what every executable built with
//! [`Dashboard::run`](crate::Dashboard::run) does when given a `--eario-*`
//! argument.
//!
//! - `--eario-manifest` prints the [`Manifest`] as one line of JSON.
//! - `--eario-install [--yes]` copies the running executable into the hub
//!   directory and records it in the registry.
//! - `--eario-uninstall [--yes]` removes that copy and its registry entry.
//! - `--eario-child <run file>` serves the dashboard for a hub.
//!
//! Every other argument is left to the application. Install and uninstall ask
//! for a confirmation in the terminal unless `--yes` is given, and are refused
//! while the dashboard runs from the hub. Their questions and messages, and
//! the manifest, are the only text the contract writes on standard output.

use std::ffi::OsString;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::{env, fs};

use eario_core::Manifest;
use eario_runtime::HubDirError;
use eario_runtime::hub_dir::{self, HubDir, RegistryEntry};

use crate::error::LaunchError;

pub(crate) use eario_core::CHILD_ARGUMENT;

/// What the command line asks the executable to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Launch {
    /// No `--eario-*` argument: run as before, standalone.
    Standalone,
    /// Print the manifest and exit.
    Manifest,
    /// Install into the hub, without asking when `yes`.
    Install {
        /// `--yes` was given.
        yes: bool,
    },
    /// Remove from the hub, without asking when `yes`.
    Uninstall {
        /// `--yes` was given.
        yes: bool,
    },
    /// Serve for a hub, announcing the address in this run file.
    Child {
        /// The run file.
        run_file: PathBuf,
    },
}

/// Reads the launch contract out of the command-line arguments, program name
/// excluded. Arguments outside the contract are skipped; `--yes` counts only
/// with an install or an uninstall.
pub(crate) fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Launch, LaunchError> {
    let mut chosen: Option<(String, Launch)> = None;
    let mut yes = false;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        // An argument that is not Unicode is never one of the contract.
        let Some(text) = argument.to_str() else { continue };
        let launch = match text {
            "--eario-manifest" => Launch::Manifest,
            "--eario-install" => Launch::Install { yes: false },
            "--eario-uninstall" => Launch::Uninstall { yes: false },
            CHILD_ARGUMENT => {
                let run_file = arguments.next().ok_or(LaunchError::MissingRunFile)?;
                Launch::Child { run_file: PathBuf::from(run_file) }
            }
            "--yes" => {
                yes = true;
                continue;
            }
            other if other.starts_with("--eario-") => {
                return Err(LaunchError::UnknownArgument(other.to_owned()));
            }
            _ => continue,
        };
        if let Some((first, _)) = chosen {
            return Err(LaunchError::ConflictingArguments { first, second: text.to_owned() });
        }
        chosen = Some((text.to_owned(), launch));
    }
    Ok(match chosen.map(|(_, launch)| launch) {
        None => Launch::Standalone,
        Some(Launch::Install { .. }) => Launch::Install { yes },
        Some(Launch::Uninstall { .. }) => Launch::Uninstall { yes },
        Some(other) => other,
    })
}

/// Prints the manifest on standard output, as one line of JSON.
pub(crate) fn print_manifest(manifest: &Manifest) -> Result<(), LaunchError> {
    let text = serde_json::to_string(manifest)
        .map_err(|error| LaunchError::Terminal(io::Error::other(error)))?;
    say(&text)
}

/// Copies the running executable into `<hub>/bin/<slug>/` and records it in
/// the registry, replacing an earlier install of the same slug.
pub(crate) fn install(manifest: &Manifest, yes: bool) -> Result<(), LaunchError> {
    let hub = HubDir::resolve()?;
    let slug = &manifest.slug;
    refuse_if_running(&hub, slug)?;
    let registry = hub.read_registry()?;

    let source = env::current_exe().map_err(LaunchError::CurrentExecutable)?;
    let file_name = source.file_name().ok_or_else(|| {
        LaunchError::CurrentExecutable(io::Error::other(format!(
            "{} has no file name",
            source.display()
        )))
    })?;
    let destination = std::path::absolute(hub.bin_dir(slug)?.join(file_name))
        .map_err(|source| HubDirError::Io { path: hub.root().to_owned(), source })?;

    let previous = registry.get(slug).cloned();
    let question = install_question(manifest, previous.as_ref().map(|old| &old.manifest), &hub);
    if !confirm(&question, yes, "installing", slug)? {
        return say("Nothing was changed.");
    }
    // The answer can take any time: a hub may have started this dashboard or
    // changed the registry meanwhile.
    refuse_if_running(&hub, slug)?;
    let mut registry = hub.read_registry()?;

    // Reinstalling the hub's own copy keeps it as it is.
    if !same_file(&source, &destination) {
        hub_dir::copy_atomic(&source, &destination)?;
    }
    let (size, sha256) = hub_dir::file_digest(&destination)?;
    registry.insert(RegistryEntry {
        slug: slug.clone(),
        path: destination.clone(),
        size,
        sha256,
        manifest: manifest.clone(),
    });
    hub.write_registry(&registry)?;

    let mut message = format!(
        "Installed {}, version {}, as `{slug}`: {}",
        manifest.name,
        version_text(&manifest.version),
        destination.display()
    );
    // An earlier copy under another file name is in the same directory.
    let stale = previous
        .map(|old| old.path)
        .filter(|old| old != &destination && old.parent() == destination.parent() && old.exists());
    if let Some(stale) = stale
        && let Err(error) = fs::remove_file(&stale)
    {
        message.push_str(&format!(
            "\nThe previous copy {} could not be removed ({error}); delete it by hand.",
            stale.display()
        ));
    }
    say(&message)
}

/// The confirmation asked before installing `new`; with the version of the
/// installed one when it replaces `old`.
fn install_question(new: &Manifest, old: Option<&Manifest>, hub: &HubDir) -> String {
    match old {
        Some(old) => format!(
            "Replace {} in the hub at {}: installed version {}, new version {}?",
            new.name,
            hub.root().display(),
            version_text(&old.version),
            version_text(&new.version),
        ),
        None => format!(
            "Install {}, version {}, as `{}` in the hub at {}?",
            new.name,
            version_text(&new.version),
            new.slug,
            hub.root().display(),
        ),
    }
}

/// Removes the hub's copy of the executable and its registry entry.
pub(crate) fn uninstall(manifest: &Manifest, yes: bool) -> Result<(), LaunchError> {
    let hub = HubDir::resolve()?;
    let slug = &manifest.slug;
    refuse_if_running(&hub, slug)?;
    let registry = hub.read_registry()?;
    let Some(entry) = registry.get(slug) else {
        return Err(LaunchError::NotInstalled {
            slug: slug.clone(),
            registry: hub.registry_path(),
        });
    };

    let question = format!(
        "Remove {}, version {}, from the hub at {}?",
        entry.manifest.name,
        version_text(&entry.manifest.version),
        hub.root().display(),
    );
    if !confirm(&question, yes, "removing", slug)? {
        return say("Nothing was changed.");
    }
    let name = entry.manifest.name.clone();
    // The answer can take any time: a hub may have started this dashboard or
    // changed the registry meanwhile.
    refuse_if_running(&hub, slug)?;
    let mut registry = hub.read_registry()?;

    // Only the hub's own directory for this slug is removed, never a path read
    // from the registry. It goes first: if it cannot be removed, the entry
    // stays and the user can try again.
    let directory = hub.bin_dir(slug)?;
    match fs::remove_dir_all(&directory) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(source) => return Err(LaunchError::Remove { path: directory, source }),
    }
    registry.remove(slug);
    hub.write_registry(&registry)?;
    say(&format!("Removed {name} (`{slug}`) from the hub."))
}

/// Fails while the dashboard's run file answers with its token. A run file
/// that cannot be parsed belongs to no live dashboard: one is always written
/// whole.
fn refuse_if_running(hub: &HubDir, slug: &str) -> Result<(), LaunchError> {
    let run = match hub.read_run_file(slug) {
        Ok(Some(run)) => run,
        Ok(None) | Err(HubDirError::Invalid { .. }) => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let live = hub_dir::probe_blocking(&run.address, &run.token)
        .map_err(|source| LaunchError::Probe { slug: slug.to_owned(), source })?;
    if live {
        return Err(LaunchError::Running {
            slug: slug.to_owned(),
            run_file: hub.run_file_path(slug)?,
        });
    }
    Ok(())
}

/// Asks `question` in the terminal, unless `yes`. Without a terminal the
/// answer cannot be given, so the action is refused.
fn confirm(
    question: &str,
    yes: bool,
    action: &'static str,
    slug: &str,
) -> Result<bool, LaunchError> {
    if yes {
        return Ok(true);
    }
    let input = io::stdin();
    if !input.is_terminal() {
        return Err(LaunchError::ConfirmationRequired { action, slug: slug.to_owned() });
    }
    ask(question, &mut input.lock(), &mut io::stdout().lock()).map_err(LaunchError::Terminal)
}

/// Writes `question` and reads one line: `y` or `yes`, in any case, agrees.
fn ask(question: &str, input: &mut impl BufRead, output: &mut impl Write) -> io::Result<bool> {
    write!(output, "{question} [y/N] ")?;
    output.flush()?;
    let mut answer = String::new();
    input.read_line(&mut answer)?;
    Ok(matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes"))
}

/// Writes one message on standard output.
fn say(message: &str) -> Result<(), LaunchError> {
    writeln!(io::stdout().lock(), "{message}").map_err(LaunchError::Terminal)
}

fn version_text(version: &Option<String>) -> &str {
    version.as_deref().unwrap_or("unknown")
}

/// Whether both paths name one existing file.
fn same_file(first: &Path, second: &Path) -> bool {
    match (fs::canonicalize(first), fs::canonicalize(second)) {
        (Ok(first), Ok(second)) => first == second,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(arguments: &[&str]) -> Result<Launch, LaunchError> {
        parse(arguments.iter().map(OsString::from))
    }

    #[test]
    fn without_a_contract_argument_the_executable_runs_standalone() {
        assert_eq!(parsed(&[]).ok(), Some(Launch::Standalone));
        let arguments = ["data.csv", "--mcp", "--yes", "-"];
        assert_eq!(parsed(&arguments).ok(), Some(Launch::Standalone));
    }

    #[test]
    fn contract_arguments_are_found_among_the_others() {
        assert_eq!(parsed(&["a.csv", "--eario-manifest", "b.csv"]).ok(), Some(Launch::Manifest));
        assert_eq!(parsed(&["--eario-install"]).ok(), Some(Launch::Install { yes: false }));
        assert_eq!(parsed(&["--yes", "--eario-install"]).ok(), Some(Launch::Install { yes: true }));
        assert_eq!(
            parsed(&["--eario-uninstall", "--yes"]).ok(),
            Some(Launch::Uninstall { yes: true })
        );
        assert_eq!(
            parsed(&["--eario-child", "run/a.json", "data.csv"]).ok(),
            Some(Launch::Child { run_file: PathBuf::from("run/a.json") })
        );
    }

    #[test]
    fn malformed_contract_arguments_are_refused() {
        let code = |arguments: &[&str]| parsed(arguments).err().map(|error| error.code());
        assert_eq!(code(&["--eario-child"]), Some("launch_missing_run_file"));
        assert_eq!(code(&["--eario-manfest"]), Some("launch_unknown_argument"));
        assert_eq!(
            code(&["--eario-install", "--eario-uninstall"]),
            Some("launch_conflicting_arguments")
        );
        assert_eq!(
            code(&["--eario-manifest", "--eario-manifest"]),
            Some("launch_conflicting_arguments")
        );
        let message = parsed(&["--eario-oops"]).err().map(|error| error.to_string());
        assert!(message.is_some_and(|message| message.contains("--eario-oops")));
    }

    #[test]
    fn the_error_main_prints_is_the_message_and_its_code() {
        // `main` returning an error prints `Error: {error:?}`.
        let error = crate::Error::from(LaunchError::ConfirmationRequired {
            action: "installing",
            slug: "alpha".to_owned(),
        });
        let printed = format!("{error:?}");
        assert!(printed.contains("installing `alpha` needs a confirmation"), "{printed}");
        assert!(printed.contains("add --yes (launch_confirmation_required)"), "{printed}");
    }

    #[test]
    fn only_yes_agrees() -> io::Result<()> {
        for (answer, agreed) in
            [("y\n", true), ("YES\r\n", true), ("n\n", false), ("\n", false), ("", false)]
        {
            let mut output = Vec::new();
            assert_eq!(ask("Go?", &mut answer.as_bytes(), &mut output)?, agreed, "{answer:?}");
            assert_eq!(output, b"Go? [y/N] ");
        }
        Ok(())
    }

    #[test]
    fn the_install_question_shows_the_old_and_the_new_version() {
        let manifest = |version: Option<&str>| Manifest {
            name: "Data Explorer".to_owned(),
            slug: "data-explorer".to_owned(),
            version: version.map(str::to_owned),
            eario_version: "0.2.0".to_owned(),
            protocol: eario_core::LAUNCH_PROTOCOL_VERSION,
            description: String::new(),
        };
        let hub = HubDir::at("hub");
        let first = install_question(&manifest(Some("1.1.0")), None, &hub);
        assert!(first.starts_with("Install Data Explorer, version 1.1.0, as `data-explorer`"));

        let replace = install_question(&manifest(Some("1.2.0")), Some(&manifest(None)), &hub);
        assert!(replace.contains("installed version unknown, new version 1.2.0"), "{replace}");
    }
}
