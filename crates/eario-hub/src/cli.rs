//! The `eario-hub` command line.
//!
//! Without arguments `eario-hub` is the hub, as [`crate::run`] describes.
//! With a command (see [`USAGE`]) it does that one thing on the hub directory
//! and exits. A command that changes something shows what it will do and
//! asks for a confirmation in the terminal, unless `--yes` is given; without
//! a terminal it refuses. A key the hub does not trust yet is trusted only
//! when the user agrees in the terminal or gives its SHA-256 with
//! `--accept-key`, never because of `--yes`.
//!
//! An install or an update of a release signed with such a key goes in two
//! steps: the command shows the key and asks to accept it by its SHA-256;
//! only then is the downloaded executable run for its manifest, and the
//! command shows the full preview and asks to install it.
//!
//! `install` takes `--slug` to choose one of the dashboards a repository
//! publishes in one release; without it the release must have only one for
//! this machine.
//!
//! The commands work beside a running hub. The hub reads the registry again
//! for every request, and every file is written atomically. Every change
//! that reads a file of the hub directory and writes it back (the registry,
//! the catalogs, the settings) holds the hub directory's lock, which the hub
//! takes for its own changes too, so neither undoes the other's. A command's
//! preview waits in a staging area of its own, which a hub that starts
//! meanwhile leaves alone. A dashboard that runs is neither updated nor
//! rolled back.
//!
//! `self-update` replaces the executable it runs from, so it refuses while
//! a hub runs: the user updates that hub from its own page, or quits it
//! first. It never restarts anything; the next start runs the new version.
//!
//! Text from a release, a catalog or GitHub is printed with its control and
//! bidirectional characters replaced, so it cannot rewrite the lines the
//! user decides on, such as the key status.

use std::ffi::OsString;
use std::io::{self, BufRead, IsTerminal, Write};
use std::process::ExitCode;

use eario_core::{HubCatalog, InstallPreview, KeyStatus, Manifest, UpdateCheck};
use eario_runtime::hub_dir::HubDir;

use crate::remote;
use crate::self_update::{HUB_SLUG, SelfUpdateError, VERSION_ARGUMENT};
use crate::supervisor::{HubError, Supervisor, SupervisorOptions};

/// What `eario-hub help` prints.
pub const USAGE: &str = "\
Usage: eario-hub [COMMAND] [OPTIONS]

Without a command, starts the hub, or opens the one that runs.

Commands:
  catalog list                List the catalogs and their dashboards
  catalog add <owner/repo>    Add the catalog of a GitHub repository
  catalog remove <owner/repo> Remove a catalog
  install <owner/repo>        Install the latest release of a GitHub repository
  check                       Check GitHub for updates of the installed dashboards
  update <slug>               Update a dashboard to its latest release
  rollback <slug>             Restore the version the last update replaced
  check-at-start [on|off]     Show, or turn on or off, the update check when the hub starts
  self-update                 Update the hub itself to its latest release (quit the hub first)
  help                        Show this help

Options:
  --version               Show the version of the hub
  --yes                   Do not ask for confirmations
  --slug <slug>           With install: the dashboard to install, when the
                          repository publishes several in one release
  --accept-key <sha256>   With install or update: trust the key the release is
                          signed with if it has this SHA-256, as the command
                          shows it. Compare the key with the one its author
                          publishes first.
";

/// What the command line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Command {
    /// No command: the hub itself.
    Hub,
    /// The help.
    Help,
    /// The version.
    Version,
    /// A command on the hub directory.
    Action(Action),
}

/// A command on the hub directory.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Action {
    CatalogList,
    CatalogAdd(String),
    CatalogRemove(String),
    Install(String),
    Check,
    Update(String),
    Rollback(String),
    CheckAtStart(Option<bool>),
    SelfUpdate,
}

/// The options of a command.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Options {
    yes: bool,
    accept_key: Option<String>,
    slug: Option<String>,
}

/// Why a command failed.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    /// The arguments are not a command.
    #[error("{0}")]
    Usage(String),
    /// The hub refused or failed.
    #[error(transparent)]
    Hub(#[from] HubError),
    /// A confirmation is needed and there is no terminal.
    #[error(
        "{action} needs a confirmation, and there is no terminal to ask it in; run the command in a terminal, or add --yes"
    )]
    ConfirmationRequired {
        /// What needs it.
        action: &'static str,
    },
    /// The terminal could not be read or written.
    #[error("could not use the terminal: {0}")]
    Terminal(io::Error),
}

impl CliError {
    /// Stable, machine-readable code of the error.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Usage(_) => "hub_usage",
            Self::Hub(error) => error.code(),
            Self::ConfirmationRequired { .. } => "hub_confirmation_required",
            Self::Terminal(_) => "hub_terminal_failed",
        }
    }
}

/// The `eario-hub` executable, given its arguments without the program
/// name. Exits with 0 on success, 1 on a failure, 2 on arguments that are no
/// command.
pub fn main(arguments: impl IntoIterator<Item = OsString>) -> ExitCode {
    let (command, options) = match parse(arguments) {
        Ok(parsed) => parsed,
        Err(error) => {
            eprintln!("Error: {error} ({})\n\n{USAGE}", error.code());
            return ExitCode::from(2);
        }
    };
    let result = match command {
        Command::Hub => crate::run().map_err(CliError::from),
        Command::Help => write!(io::stdout().lock(), "{USAGE}").map_err(CliError::Terminal),
        Command::Version => {
            writeln!(io::stdout().lock(), "{}", version_line()).map_err(CliError::Terminal)
        }
        Command::Action(action) => run_action(action, &options),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {} ({})", shown(&error.to_string()), error.code());
            if matches!(error.code(), "hub_key_not_accepted" | "hub_key_changed") {
                eprintln!(
                    "To trust the key, run the command again in a terminal, or add --accept-key <its SHA-256>, once you have compared it with the one its author publishes."
                );
            }
            ExitCode::FAILURE
        }
    }
}

/// Reads the command and its options. `--yes` and `--accept-key` may come
/// anywhere.
fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<(Command, Options), CliError> {
    let usage = |message: String| CliError::Usage(message);
    let mut options = Options::default();
    let mut words = Vec::new();
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        let argument = argument.into_string().map_err(|argument| {
            usage(format!("`{}` is not valid Unicode", argument.to_string_lossy()))
        })?;
        match argument.as_str() {
            "--yes" => options.yes = true,
            "--accept-key" => {
                let value = arguments.next().and_then(|value| value.into_string().ok());
                let value =
                    value.ok_or_else(|| usage("--accept-key needs the SHA-256 of a key".into()))?;
                options.accept_key = Some(value);
            }
            "--slug" => {
                let value = arguments.next().and_then(|value| value.into_string().ok());
                let value =
                    value.ok_or_else(|| usage("--slug needs the slug of a dashboard".into()))?;
                options.slug = Some(value);
            }
            "--help" | "-h" => return Ok((Command::Help, Options::default())),
            VERSION_ARGUMENT => return Ok((Command::Version, Options::default())),
            other if other.starts_with('-') => {
                return Err(usage(format!("unknown option `{other}`")));
            }
            _ => words.push(argument),
        }
    }
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    let action = |action| Command::Action(action);
    let command = match words.as_slice() {
        [] => Command::Hub,
        ["help"] => Command::Help,
        ["catalog", "list"] => action(Action::CatalogList),
        ["catalog", "add", repo] => action(Action::CatalogAdd((*repo).to_owned())),
        ["catalog", "remove", repo] => action(Action::CatalogRemove((*repo).to_owned())),
        ["install", repo] => action(Action::Install((*repo).to_owned())),
        ["check"] => action(Action::Check),
        ["update", slug] => action(Action::Update((*slug).to_owned())),
        ["rollback", slug] => action(Action::Rollback((*slug).to_owned())),
        ["check-at-start"] => action(Action::CheckAtStart(None)),
        ["check-at-start", "on"] => action(Action::CheckAtStart(Some(true))),
        ["check-at-start", "off"] => action(Action::CheckAtStart(Some(false))),
        ["self-update"] => action(Action::SelfUpdate),
        _ => return Err(usage(format!("`{}` is not a command", words.join(" ")))),
    };
    let takes_key = matches!(command, Command::Action(Action::Install(_) | Action::Update(_)));
    if options.accept_key.is_some() && !takes_key {
        return Err(usage("--accept-key goes with install or update".into()));
    }
    if options.slug.is_some() && !matches!(command, Command::Action(Action::Install(_))) {
        return Err(usage("--slug goes with install".into()));
    }
    if command == Command::Hub && options.yes {
        return Err(usage("--yes goes with a command".into()));
    }
    Ok((command, options))
}

/// Runs `action` on this user's hub directory, in this terminal.
fn run_action(action: Action, options: &Options) -> Result<(), CliError> {
    crate::init_logging();
    let hub = HubDir::resolve().map_err(HubError::from)?;
    let supervisor = Supervisor::new(hub, SupervisorOptions::default())?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(HubError::Runtime)?;
    let input = io::stdin();
    let interactive = input.is_terminal();
    let mut terminal = Terminal {
        input: &mut input.lock(),
        output: &mut io::stdout().lock(),
        interactive,
        yes: options.yes,
    };
    runtime.block_on(execute(&supervisor, action, options, &mut terminal))
}

/// Where a command talks to the user.
struct Terminal<'a> {
    input: &'a mut dyn BufRead,
    output: &'a mut dyn Write,
    /// Whether the input is a terminal, where the user can answer.
    interactive: bool,
    /// `--yes` was given.
    yes: bool,
}

impl Terminal<'_> {
    fn say(&mut self, text: &str) -> Result<(), CliError> {
        writeln!(self.output, "{text}").map_err(CliError::Terminal)
    }

    /// Asks `question`, whatever `--yes` says; `y` or `yes`, in any case,
    /// agrees. Refused without a terminal.
    fn ask(&mut self, question: &str, action: &'static str) -> Result<bool, CliError> {
        if !self.interactive {
            return Err(CliError::ConfirmationRequired { action });
        }
        write!(self.output, "{question} [y/N] ").map_err(CliError::Terminal)?;
        self.output.flush().map_err(CliError::Terminal)?;
        let mut answer = String::new();
        self.input.read_line(&mut answer).map_err(CliError::Terminal)?;
        Ok(matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes"))
    }

    /// Asks `question` unless `--yes` was given.
    fn confirm(&mut self, question: &str, action: &'static str) -> Result<bool, CliError> {
        if self.yes {
            return Ok(true);
        }
        self.ask(question, action)
    }
}

const NOTHING_CHANGED: &str = "Nothing was changed.";

async fn execute(
    supervisor: &Supervisor,
    action: Action,
    options: &Options,
    terminal: &mut Terminal<'_>,
) -> Result<(), CliError> {
    match action {
        Action::CatalogList => {
            let catalogs = supervisor.catalog_listing().await?;
            terminal.say(&describe_catalogs(&catalogs))
        }
        Action::CatalogAdd(repo) => {
            let question =
                format!("Add the catalog `{repo}`? The hub will read its catalog.json on GitHub.");
            if !terminal.confirm(&question, "adding a catalog")? {
                return terminal.say(NOTHING_CHANGED);
            }
            let catalogs = supervisor.add_catalog(&repo).await?;
            terminal.say(&format!("Catalogs: {}", list_text(&catalogs)))
        }
        Action::CatalogRemove(repo) => {
            let question = format!("Remove the catalog `{repo}`?");
            if !terminal.confirm(&question, "removing a catalog")? {
                return terminal.say(NOTHING_CHANGED);
            }
            let catalogs = supervisor.remove_catalog(&repo).await?;
            terminal.say(&format!("Catalogs: {}", list_text(&catalogs)))
        }
        Action::Install(repo) => install(supervisor, &repo, options, terminal).await,
        Action::Check => {
            let check = supervisor.check().await?;
            terminal.say(&describe_check(&check))
        }
        Action::Update(slug) => update(supervisor, &slug, options, terminal).await,
        Action::Rollback(slug) => rollback(supervisor, &slug, terminal).await,
        Action::CheckAtStart(None) => {
            let settings = supervisor.settings().await?;
            terminal.say(check_at_start_text(settings.check_updates_at_start))
        }
        Action::CheckAtStart(Some(on)) => {
            let settings = supervisor.set_check_updates_at_start(on).await?;
            terminal.say(check_at_start_text(settings.check_updates_at_start))
        }
        Action::SelfUpdate => self_update(supervisor, terminal).await,
    }
}

/// What `eario-hub --version` prints, and what the hub's own update reads
/// from a new executable: `eario-hub <version>`.
fn version_line() -> String {
    format!("{HUB_SLUG} {}", env!("CARGO_PKG_VERSION"))
}

/// Updates the hub's executable, which is this command's, to the latest
/// release, after a confirmation. Refused while a hub runs.
async fn self_update(supervisor: &Supervisor, terminal: &mut Terminal<'_>) -> Result<(), CliError> {
    // The running hub's executable is the one to replace, from the hub.
    if let Some(running) = crate::running_hub(supervisor.hub_dir()).await {
        return Err(HubError::from(SelfUpdateError::HubRunning { address: running.address }).into());
    }
    let current = env!("CARGO_PKG_VERSION");
    terminal.say("Reading the latest release of the Eario hub...")?;
    let found = supervisor.check_hub().await?;
    let Some(available) = found.available_version.as_deref() else {
        return terminal.say(&format!(
            "The Eario hub is up to date: version {current}, latest release {}.",
            version(found.latest_version.as_deref())
        ));
    };
    let question = format!(
        "Update the Eario hub from version {current} to {}? The release is verified with the Eario release key built into this hub before it replaces this executable.",
        shown(available)
    );
    if !terminal.confirm(&question, "updating the hub")? {
        return terminal.say(NOTHING_CHANGED);
    }
    let updated = supervisor.self_update().await?;
    terminal.say(&format!(
        "Updated the Eario hub from version {} to {}. Start it with `eario-hub`.",
        updated.previous_version,
        shown(&updated.version)
    ))
}

async fn install(
    supervisor: &Supervisor,
    repo: &str,
    options: &Options,
    terminal: &mut Terminal<'_>,
) -> Result<(), CliError> {
    let slug = options.slug.as_deref();
    terminal.say(&format!(
        "Reading the latest release of `{repo}`{} and verifying it...",
        remote::dashboard_text(slug)
    ))?;
    let preview = supervisor.preview(repo, slug).await?;
    terminal.say(&describe_preview(&preview))?;
    let Some(preview) = trust_key(supervisor, preview, options, terminal).await? else {
        return terminal.say(NOTHING_CHANGED);
    };
    let manifest = manifest_of(&preview)?;
    let question = format!(
        "Install {}, version {}, from `{repo}`?",
        shown(&manifest.name),
        version(manifest.version.as_deref())
    );
    if !terminal.confirm(&question, "installing")? {
        return terminal.say(NOTHING_CHANGED);
    }
    let slug = supervisor.install(repo, Some(&preview.slug), &preview.sha256).await?;
    terminal.say(&format!(
        "Installed {}, version {}, as `{slug}`. Start it from the hub.",
        shown(&manifest.name),
        version(manifest.version.as_deref())
    ))
}

async fn update(
    supervisor: &Supervisor,
    slug: &str,
    options: &Options,
    terminal: &mut Terminal<'_>,
) -> Result<(), CliError> {
    let installed = supervisor.installed(slug).await?;
    terminal.say(&format!("Reading the latest release of `{slug}` and verifying it..."))?;
    let preview = supervisor.preview_update(slug).await?;
    terminal.say(&describe_preview(&preview))?;
    let Some(preview) = trust_key(supervisor, preview, options, terminal).await? else {
        return terminal.say(NOTHING_CHANGED);
    };
    let manifest = manifest_of(&preview)?;
    let question = format!(
        "Update {} from version {} to {}? The installed version is kept for a rollback.",
        shown(&manifest.name),
        version(installed.manifest.version.as_deref()),
        version(manifest.version.as_deref())
    );
    if !terminal.confirm(&question, "updating")? {
        return terminal.say(NOTHING_CHANGED);
    }
    supervisor.apply_update(slug, &preview.sha256).await?;
    terminal.say(&format!(
        "Updated {} to version {}. `eario-hub rollback {slug}` restores version {}.",
        shown(&manifest.name),
        version(manifest.version.as_deref()),
        version(installed.manifest.version.as_deref())
    ))
}

async fn rollback(
    supervisor: &Supervisor,
    slug: &str,
    terminal: &mut Terminal<'_>,
) -> Result<(), CliError> {
    let installed = supervisor.installed(slug).await?;
    let Some(previous) = &installed.previous else {
        return Err(HubError::NoPrevious { slug: slug.to_owned() }.into());
    };
    let question = format!(
        "Roll {} back from version {} to {}? Version {} is removed.",
        shown(&installed.manifest.name),
        version(installed.manifest.version.as_deref()),
        version(previous.manifest.version.as_deref()),
        version(installed.manifest.version.as_deref()),
    );
    if !terminal.confirm(&question, "rolling back")? {
        return terminal.say(NOTHING_CHANGED);
    }
    supervisor.rollback(slug).await?;
    terminal.say(&format!(
        "Restored {}, version {}.",
        shown(&previous.manifest.name),
        version(previous.manifest.version.as_deref())
    ))
}

/// The full preview to install: `preview` itself when its key is trusted.
/// Otherwise the key is accepted when the user agrees, by `--accept-key` or
/// in the terminal, and the hub then runs the verified executable for its
/// manifest; the full preview is shown and returned. `None` when the user
/// declines in the terminal; refused (`hub_key_not_accepted`,
/// `hub_key_changed`) without `--accept-key` and without a terminal, even
/// with `--yes`.
async fn trust_key(
    supervisor: &Supervisor,
    preview: InstallPreview,
    options: &Options,
    terminal: &mut Terminal<'_>,
) -> Result<Option<InstallPreview>, CliError> {
    let question = match preview.key_status {
        KeyStatus::Catalog | KeyStatus::Pinned | KeyStatus::Accepted => return Ok(Some(preview)),
        KeyStatus::Unknown => {
            "No catalog lists this repository, so the hub does not know its key yet, and has not run the downloaded executable. Trust the key above, once you have compared its SHA-256 with the one its author publishes?"
        }
        KeyStatus::Changed => {
            "WARNING: this release is signed with another key than the one trusted for this repository. Its author may have changed keys, or someone else published this release. The hub has not run the downloaded executable. Trust the new key?"
        }
    };
    let key_sha256 = match &options.accept_key {
        Some(given) => given.clone(),
        None if terminal.yes || !terminal.interactive => {
            return Err(remote::untrusted_key(&preview).into());
        }
        None if terminal.ask(question, "trusting a key")? => preview.key_sha256.clone(),
        None => return Ok(None),
    };
    terminal.say("Accepting the key, then reading the manifest of the verified executable...")?;
    let full = supervisor.accept_key(&preview.repo, Some(&preview.slug), &key_sha256).await?;
    terminal.say("Key accepted.")?;
    terminal.say(&describe_preview(&full))?;
    Ok(Some(full))
}

/// The manifest of a full preview.
fn manifest_of(preview: &InstallPreview) -> Result<&Manifest, CliError> {
    preview.manifest.as_ref().ok_or_else(|| remote::untrusted_key(preview).into())
}

/// `text` with every control character (line breaks, carriage returns,
/// terminal escapes) and every bidirectional formatting character replaced
/// by U+FFFD, so that what a release or a catalog says stays on its line and
/// in its order.
fn shown(text: &str) -> String {
    text.chars()
        .map(|character| {
            let bidirectional = matches!(
                character,
                '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
            );
            if character.is_control() || bidirectional { '\u{FFFD}' } else { character }
        })
        .collect()
}

/// A version as printed: shown, or `unknown`.
fn version(version: Option<&str>) -> String {
    shown(version.unwrap_or("unknown"))
}

fn describe_preview(preview: &InstallPreview) -> String {
    let status = match preview.key_status {
        KeyStatus::Catalog => "listed with this key by a catalog of the hub",
        KeyStatus::Pinned => {
            "the key pinned for this repository when a dashboard was installed from it"
        }
        KeyStatus::Unknown => "UNKNOWN: no catalog lists this repository",
        KeyStatus::Changed => {
            "CHANGED: not the key trusted for this repository (pinned, or listed by a catalog)"
        }
        KeyStatus::Accepted => "accepted by you",
    };
    let dashboard = match &preview.manifest {
        Some(manifest) => format!(
            "{} (`{}`), version {}",
            shown(&manifest.name),
            preview.slug,
            version(manifest.version.as_deref())
        ),
        None => format!(
            "`{}`; its name and version are read from the executable once its key is trusted",
            preview.slug
        ),
    };
    format!(
        "Repository:  {}\nDashboard:   {dashboard}\nRelease:     {}\nAsset:       {} ({} bytes)\nSHA-256:     {}\nSigning key: {}\nKey SHA-256: {}\nKey status:  {status}",
        preview.repo,
        shown(&preview.tag),
        shown(&preview.asset),
        preview.size,
        preview.sha256,
        preview.public_key,
        preview.key_sha256,
    )
}

fn describe_check(check: &UpdateCheck) -> String {
    if check.dashboards.is_empty() {
        return "No dashboard is installed from GitHub: nothing to check.".to_owned();
    }
    let lines: Vec<String> = check
        .dashboards
        .iter()
        .map(|found| {
            let state = match (&found.error, &found.available_version, &found.latest_version) {
                (Some(error), _, _) => {
                    format!("could not check: {} ({})", shown(&error.message), error.code)
                }
                (None, Some(available), _) => format!(
                    "version {} available: `eario-hub update {}`",
                    shown(available),
                    found.slug
                ),
                (None, None, Some(latest)) => {
                    format!("up to date (latest release {})", shown(latest))
                }
                (None, None, None) => "up to date (the latest tag is not a version)".to_owned(),
            };
            format!("{} ({}): {state}", found.slug, found.repo)
        })
        .collect();
    lines.join("\n")
}

fn describe_catalogs(catalogs: &[HubCatalog]) -> String {
    if catalogs.is_empty() {
        return "No catalog. Add one with `eario-hub catalog add <owner/repo>`.".to_owned();
    }
    let mut lines = Vec::new();
    for catalog in catalogs {
        match (&catalog.error, &catalog.name) {
            (Some(error), _) => lines.push(format!(
                "{}: could not be read: {} ({})",
                catalog.repo,
                shown(&error.message),
                error.code
            )),
            (None, name) => {
                let name = shown(name.as_deref().unwrap_or_default());
                lines.push(format!("{}: {name}", catalog.repo));
                if catalog.dashboards.is_empty() {
                    lines.push("  (no dashboard)".to_owned());
                }
                for entry in &catalog.dashboards {
                    let state = match &entry.installed_slug {
                        Some(slug) => format!(" [installed as `{slug}`]"),
                        None => String::new(),
                    };
                    let description = if entry.description.is_empty() {
                        String::new()
                    } else {
                        format!(": {}", shown(&entry.description))
                    };
                    let name = shown(&entry.name);
                    let source = match &entry.slug {
                        Some(slug) => format!("{} --slug {slug}", entry.repo),
                        None => entry.repo.clone(),
                    };
                    lines.push(format!("  {source}  {name}{description}{state}"));
                }
            }
        }
    }
    lines.join("\n")
}

fn list_text(catalogs: &[String]) -> String {
    if catalogs.is_empty() { "none".to_owned() } else { catalogs.join(", ") }
}

fn check_at_start_text(on: bool) -> &'static str {
    if on {
        "The hub checks GitHub for updates when it starts."
    } else {
        "The hub does not check GitHub for updates when it starts."
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(arguments: &[&str]) -> Result<(Command, Options), CliError> {
        parse(arguments.iter().map(OsString::from))
    }

    fn command(arguments: &[&str]) -> Option<Command> {
        parsed(arguments).ok().map(|(command, _)| command)
    }

    #[test]
    fn without_arguments_it_is_the_hub() {
        assert_eq!(command(&[]), Some(Command::Hub));
        assert_eq!(command(&["help"]), Some(Command::Help));
        assert_eq!(command(&["--help"]), Some(Command::Help));
        assert_eq!(command(&["--version"]), Some(Command::Version));
        assert_eq!(command_of(&["self-update", "--yes"]), Some(Action::SelfUpdate));
        assert_eq!(version_line(), format!("eario-hub {}", env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn commands_and_their_options_are_read() -> Result<(), CliError> {
        let (command, options) = parsed(&["install", "lab/signal-lab", "--yes"])?;
        assert_eq!(command, Command::Action(Action::Install("lab/signal-lab".into())));
        assert!(options.yes);
        let key = "ab".repeat(32);
        let (command, options) = parsed(&["--accept-key", &key, "update", "signal-lab"])?;
        assert_eq!(command, Command::Action(Action::Update("signal-lab".into())));
        assert_eq!(options.accept_key, Some(key));
        assert!(!options.yes);
        assert_eq!(
            command_of(&["catalog", "remove", "a/b"]),
            Some(Action::CatalogRemove("a/b".into()))
        );
        assert_eq!(command_of(&["check-at-start", "off"]), Some(Action::CheckAtStart(Some(false))));
        assert_eq!(command_of(&["check-at-start"]), Some(Action::CheckAtStart(None)));
        let (command, options) = parsed(&["install", "lab/suite", "--slug", "signal-lab"])?;
        assert_eq!(command, Command::Action(Action::Install("lab/suite".into())));
        assert_eq!(options.slug.as_deref(), Some("signal-lab"));
        let (_, options) = parsed(&["install", "lab/signal-lab"])?;
        assert_eq!(options.slug, None, "without --slug, the release's only dashboard");
        Ok(())
    }

    fn command_of(arguments: &[&str]) -> Option<Action> {
        match command(arguments)? {
            Command::Action(action) => Some(action),
            _ => None,
        }
    }

    #[test]
    fn anything_else_is_a_usage_error() {
        for arguments in [
            &["install"][..],
            &["install", "a/b", "c/d"],
            &["catalog"],
            &["catalog", "add"],
            &["update"],
            &["check-at-start", "maybe"],
            &["frobnicate"],
            &["--force"],
            &["check", "--accept-key"],
            &["check", "--accept-key", "ab"],
            &["catalog", "list", "--accept-key", "ab"],
            &["--yes"],
            &["self-update", "now"],
            &["self-update", "--accept-key", "ab"],
            &["install", "a/b", "--slug"],
            &["update", "signal-lab", "--slug", "signal-lab"],
            &["check", "--slug", "x"],
            &["--slug", "x"],
        ] {
            let code = parsed(arguments).err().map(|error| error.code());
            assert_eq!(code, Some("hub_usage"), "{arguments:?}");
        }
    }

    #[test]
    fn without_a_terminal_a_confirmation_is_refused_unless_yes() -> Result<(), CliError> {
        let mut input: &[u8] = b"";
        let mut output = Vec::new();
        let mut terminal =
            Terminal { input: &mut input, output: &mut output, interactive: false, yes: false };
        let refused = terminal.confirm("Install?", "installing").err().map(|error| error.code());
        assert_eq!(refused, Some("hub_confirmation_required"));
        terminal.yes = true;
        assert!(terminal.confirm("Install?", "installing")?);
        // A key is never trusted because of --yes.
        let refused = terminal.ask("Trust?", "trusting a key").err().map(|error| error.code());
        assert_eq!(refused, Some("hub_confirmation_required"));
        Ok(())
    }

    #[test]
    fn text_from_outside_stays_on_its_line() {
        let tag = "v1.0.0\r\nKey status:  pinned\u{1b}[2K\u{202e}";
        let printed = shown(tag);
        assert!(!printed.contains(['\r', '\n', '\u{1b}', '\u{202e}']), "{printed:?}");
        assert!(printed.starts_with("v1.0.0\u{FFFD}\u{FFFD}Key status:"), "{printed:?}");
        assert_eq!(shown("Signal Lab é 1.2.0"), "Signal Lab é 1.2.0");
        assert_eq!(version(None), "unknown");
    }

    #[test]
    fn a_catalog_entry_with_a_slug_shows_how_to_install_it() {
        use eario_core::HubCatalogEntry;
        let entry = |slug: Option<&str>, installed: Option<&str>| HubCatalogEntry {
            repo: "lab/suite".to_owned(),
            slug: slug.map(str::to_owned),
            name: "Signal Lab".to_owned(),
            description: String::new(),
            installed_slug: installed.map(str::to_owned),
        };
        let catalogs = [HubCatalog {
            repo: "lab/catalog".to_owned(),
            name: Some("Suite".to_owned()),
            dashboards: vec![entry(Some("signal-lab"), Some("signal-lab")), entry(None, None)],
            error: None,
        }];
        let text = describe_catalogs(&catalogs);
        assert!(
            text.contains("  lab/suite --slug signal-lab  Signal Lab [installed as `signal-lab`]"),
            "{text}"
        );
        assert!(
            text.contains("  lab/suite  Signal Lab\n") || text.ends_with("  lab/suite  Signal Lab"),
            "{text}"
        );
    }

    #[test]
    fn a_preview_without_a_manifest_says_why() {
        let pending = InstallPreview {
            repo: "lab/signal-lab".to_owned(),
            slug: "signal-lab".to_owned(),
            manifest: None,
            tag: "v1.0.0".to_owned(),
            asset: "signal-lab-x86_64-unknown-linux-gnu".to_owned(),
            size: 5,
            sha256: "ab".repeat(32),
            public_key: "RWQabc".to_owned(),
            key_sha256: "cd".repeat(32),
            key_status: KeyStatus::Unknown,
        };
        let printed = describe_preview(&pending);
        assert!(printed.contains("`signal-lab`; its name and version are read"), "{printed}");
        assert!(printed.contains("UNKNOWN") && printed.contains(&"cd".repeat(32)), "{printed}");
        let refused = manifest_of(&pending).err().map(|error| error.code());
        assert_eq!(refused, Some("hub_key_not_accepted"));

        let full = InstallPreview {
            manifest: Some(Manifest {
                name: "Signal Lab".to_owned(),
                slug: "signal-lab".to_owned(),
                version: Some("1.0.0".to_owned()),
                eario_version: "0.2.0".to_owned(),
                protocol: eario_core::LAUNCH_PROTOCOL_VERSION,
                description: String::new(),
            }),
            key_status: KeyStatus::Accepted,
            ..pending
        };
        let printed = describe_preview(&full);
        assert!(printed.contains("Signal Lab (`signal-lab`), version 1.0.0"), "{printed}");
        assert!(manifest_of(&full).is_ok());
    }

    #[test]
    fn only_yes_agrees_in_the_terminal() -> Result<(), CliError> {
        for (answer, agreed) in [("y\n", true), ("YES\n", true), ("n\n", false), ("\n", false)] {
            let mut input = answer.as_bytes();
            let mut output = Vec::new();
            let mut terminal =
                Terminal { input: &mut input, output: &mut output, interactive: true, yes: false };
            assert_eq!(terminal.confirm("Install?", "installing")?, agreed, "{answer:?}");
            assert_eq!(String::from_utf8_lossy(&output), "Install? [y/N] ");
        }
        Ok(())
    }
}
