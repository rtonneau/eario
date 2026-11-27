//! HTTP adapter over the [`Supervisor`].
//!
//! Routes, relative to `/api`:
//!
//! | Route | Answer |
//! | --- | --- |
//! | `GET /app` | `AppResponse` of kind `hub`: the installed dashboards, their state and the last update check |
//! | `GET /alive` | `204`, at once: tells a second hub start that this hub runs |
//! | `POST /dashboards/{slug}/start` | `HubInfo`, once the process is created (the dashboard is `starting`) |
//! | `POST /dashboards/{slug}/stop` | `HubInfo`, once the dashboard is stopped |
//! | `POST /dashboards/{slug}/open` | `HubOpen`: the dashboard's address with its token |
//! | `POST /dashboards/{slug}/uninstall` | `HubInfo`, without the dashboard |
//! | `POST /stop-all` | `HubInfo`, once every dashboard is stopped |
//! | `GET /catalogs` | `HubCatalog[]`: each catalog read from GitHub, with its dashboards |
//! | `POST /catalogs/{owner}/{repo}/add` | `HubCatalog[]`, with that catalog, once it was read |
//! | `POST /catalogs/{owner}/{repo}/remove` | `HubCatalog[]`, without that catalog |
//! | `POST /sources/{owner}/{repo}/preview` | `InstallPreview` of the latest release: downloaded and verified, not installed |
//! | `POST /sources/{owner}/{repo}/accept-key/{key_sha256}` | `InstallPreview`, complete: the previewed key is accepted, then the executable is read |
//! | `POST /sources/{owner}/{repo}/install/{sha256}` | `HubInfo`, with the dashboard the preview of that SHA-256 installed |
//! | `POST /check` | `HubInfo`, whose `last_check` is the check just made |
//! | `POST /dashboards/{slug}/update-preview` | `InstallPreview` of the newer release of `slug` |
//! | `POST /dashboards/{slug}/update/{sha256}` | `HubInfo`, once the update previewed with that SHA-256 is installed |
//! | `POST /dashboards/{slug}/update` | `HubInfo`, once `slug` is updated: preview and install in one step |
//! | `POST /dashboards/{slug}/rollback` | `HubInfo`, once the previous version of `slug` is restored |
//! | `GET /settings` | `HubSettings` |
//! | `POST /settings/check-at-start/{on\|off}` | `HubSettings`, with the update check at start turned on or off |
//! | `POST /shutdown` | Added by the server: quits the hub. Dashboards keep running. |
//!
//! No route reads a request body or a query string, so none can be given a
//! URL, a path, an executable or arguments. Every parameter is one path
//! segment, refused with 400 unless it is what it names: `{slug}` a slug,
//! `{owner}/{repo}` a GitHub repository ([`eario_core::is_repo`]),
//! `{sha256}` and `{key_sha256}` 64 lower-case hexadecimal digits, and
//! `on` or `off`. The supervisor checks each before it acts. The server puts
//! every route behind the Host and Origin check and the session token, as
//! for a dashboard.
//!
//! An install takes two requests (ADR 0013): the preview, which the user
//! reads, then the install, which must repeat the previewed SHA-256. A
//! release whose key is not trusted yet (`unknown` or `changed`) needs a
//! third, between them: `accept-key` with the SHA-256 of the key the
//! preview showed. The same applies to an update previewed with
//! `update-preview`; the one-step `update` is refused with
//! `hub_key_changed` when the key changed, and leaves its preview for
//! `accept-key`.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use eario_core::{
    ApiError, AppResponse, HubCatalog, HubInfo, HubOpen, HubSettings, InstallPreview,
};
use eario_runtime::HubDirError;

use crate::github::GitHubError;
use crate::supervisor::{HubError, Supervisor};

/// Path of the route that answers `204` without looking at any dashboard or
/// file, so another hub can tell this one runs even when listing would be
/// slow or fail.
pub(crate) const ALIVE_PATH: &str = "/api/alive";

/// The hub's API routes, with the supervisor as their state. Paths are
/// relative to `/api`; serve them with [`eario_runtime::start_routes`].
pub(crate) fn routes(supervisor: Supervisor) -> Router {
    Router::new()
        .route("/app", get(app))
        .route("/alive", get(alive))
        .route("/dashboards/{slug}/start", post(start))
        .route("/dashboards/{slug}/stop", post(stop))
        .route("/dashboards/{slug}/open", post(open))
        .route("/dashboards/{slug}/uninstall", post(uninstall))
        .route("/stop-all", post(stop_all))
        .route("/catalogs", get(catalogs))
        .route("/catalogs/{owner}/{repo}/add", post(add_catalog))
        .route("/catalogs/{owner}/{repo}/remove", post(remove_catalog))
        .route("/sources/{owner}/{repo}/preview", post(preview))
        .route("/sources/{owner}/{repo}/accept-key/{key_sha256}", post(accept_key))
        .route("/sources/{owner}/{repo}/install/{sha256}", post(install))
        .route("/check", post(check))
        .route("/dashboards/{slug}/update-preview", post(preview_update))
        .route("/dashboards/{slug}/update/{sha256}", post(apply_update))
        .route("/dashboards/{slug}/update", post(update))
        .route("/dashboards/{slug}/rollback", post(rollback))
        .route("/settings", get(settings))
        .route("/settings/check-at-start/{switch}", post(set_check_at_start))
        .with_state(supervisor)
}

/// A failed request: a status and a JSON [`ApiError`] body.
struct Failure {
    status: StatusCode,
    body: ApiError,
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

impl From<HubError> for Failure {
    fn from(error: HubError) -> Self {
        let status = match &error {
            HubError::InvalidSlug(_) | HubError::HubDir(HubDirError::InvalidSlug(_)) => {
                StatusCode::BAD_REQUEST
            }
            HubError::NotInstalled { .. } => StatusCode::NOT_FOUND,
            HubError::AlreadyRunning(_)
            | HubError::Running(_)
            | HubError::Starting(_)
            | HubError::NotRunning(_)
            | HubError::UnsupportedProtocol { .. }
            | HubError::Modified { .. }
            | HubError::MissingExecutable { .. }
            | HubError::ForeignExecutable { .. } => StatusCode::CONFLICT,
            HubError::StopTimeout { .. } => StatusCode::GATEWAY_TIMEOUT,
            HubError::InvalidSha256(_) => StatusCode::BAD_REQUEST,
            // Nothing to act on: preview again, or name a listed catalog.
            HubError::NoPreview { .. }
            | HubError::PreviewGone { .. }
            | HubError::UnknownCatalog { .. } => StatusCode::NOT_FOUND,
            // Refused in the hub's present state; the message says what the
            // user can do (accept the key, stop the dashboard, preview
            // again, uninstall first).
            HubError::SlugTaken { .. }
            | HubError::AlreadyInstalled { .. }
            | HubError::PreviewMismatch { .. }
            | HubError::KeyNotAccepted { .. }
            | HubError::KeyChanged { .. }
            | HubError::KeyMismatch { .. }
            | HubError::NotFromGitHub { .. }
            | HubError::NotNewer { .. }
            | HubError::VersionNotComparable { .. }
            | HubError::NoPrevious { .. }
            | HubError::PreviousUnusable { .. }
            | HubError::ChangedMeanwhile { .. } => StatusCode::CONFLICT,
            // What GitHub served was refused.
            HubError::Verify(_)
            | HubError::NoAsset { .. }
            | HubError::AmbiguousAsset { .. }
            | HubError::ManifestFailed { .. }
            | HubError::ManifestMismatch { .. }
            | HubError::CatalogVersion { .. } => StatusCode::BAD_GATEWAY,
            HubError::UnsupportedPlatform => StatusCode::INTERNAL_SERVER_ERROR,
            HubError::GitHub(GitHubError::InvalidRepo(_)) => StatusCode::BAD_REQUEST,
            // Local failures: the disk or the system's certificate store.
            HubError::GitHub(GitHubError::Write { .. } | GitHubError::Certificates { .. }) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
            HubError::GitHub(GitHubError::Timeout { .. }) => StatusCode::GATEWAY_TIMEOUT,
            // GitHub, not the hub, failed, refused or could not be reached.
            HubError::GitHub(_) => StatusCode::BAD_GATEWAY,
            HubError::Spawn { .. }
            | HubError::Remove { .. }
            | HubError::HubDir(_)
            | HubError::Serve(_)
            | HubError::Runtime(_)
            | HubError::Task(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        if status.is_server_error() {
            tracing::error!(%error, "hub request failed");
        } else {
            tracing::debug!(%error, "hub request refused");
        }
        Self {
            status,
            body: ApiError { code: error.code().to_owned(), message: error.to_string() },
        }
    }
}

async fn alive() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn app(State(supervisor): State<Supervisor>) -> Result<Json<AppResponse>, Failure> {
    Ok(Json(AppResponse::Hub(supervisor.list().await?)))
}

async fn start(
    State(supervisor): State<Supervisor>,
    Path(slug): Path<String>,
) -> Result<Json<HubInfo>, Failure> {
    supervisor.start(&slug).await?;
    Ok(Json(supervisor.list().await?))
}

async fn stop(
    State(supervisor): State<Supervisor>,
    Path(slug): Path<String>,
) -> Result<Json<HubInfo>, Failure> {
    supervisor.stop(&slug).await?;
    Ok(Json(supervisor.list().await?))
}

async fn open(
    State(supervisor): State<Supervisor>,
    Path(slug): Path<String>,
) -> Result<Json<HubOpen>, Failure> {
    Ok(Json(supervisor.open(&slug).await?))
}

async fn uninstall(
    State(supervisor): State<Supervisor>,
    Path(slug): Path<String>,
) -> Result<Json<HubInfo>, Failure> {
    supervisor.uninstall(&slug).await?;
    Ok(Json(supervisor.list().await?))
}

async fn stop_all(State(supervisor): State<Supervisor>) -> Result<Json<HubInfo>, Failure> {
    supervisor.stop_all().await?;
    Ok(Json(supervisor.list().await?))
}

/// `owner/repo` from the two path segments `{owner}` and `{repo}`. The
/// supervisor refuses it unless it is a GitHub repository, so a segment that
/// decoded to a `/` or a `\` never passes.
fn repository(owner: &str, repo: &str) -> String {
    format!("{owner}/{repo}")
}

async fn catalogs(State(supervisor): State<Supervisor>) -> Result<Json<Vec<HubCatalog>>, Failure> {
    Ok(Json(supervisor.catalog_listing().await?))
}

async fn add_catalog(
    State(supervisor): State<Supervisor>,
    Path((owner, repo)): Path<(String, String)>,
) -> Result<Json<Vec<HubCatalog>>, Failure> {
    supervisor.add_catalog(&repository(&owner, &repo)).await?;
    Ok(Json(supervisor.catalog_listing().await?))
}

async fn remove_catalog(
    State(supervisor): State<Supervisor>,
    Path((owner, repo)): Path<(String, String)>,
) -> Result<Json<Vec<HubCatalog>>, Failure> {
    supervisor.remove_catalog(&repository(&owner, &repo)).await?;
    Ok(Json(supervisor.catalog_listing().await?))
}

async fn preview(
    State(supervisor): State<Supervisor>,
    Path((owner, repo)): Path<(String, String)>,
) -> Result<Json<InstallPreview>, Failure> {
    Ok(Json(supervisor.preview(&repository(&owner, &repo)).await?))
}

async fn accept_key(
    State(supervisor): State<Supervisor>,
    Path((owner, repo, key_sha256)): Path<(String, String, String)>,
) -> Result<Json<InstallPreview>, Failure> {
    Ok(Json(supervisor.accept_key(&repository(&owner, &repo), &key_sha256).await?))
}

async fn install(
    State(supervisor): State<Supervisor>,
    Path((owner, repo, sha256)): Path<(String, String, String)>,
) -> Result<Json<HubInfo>, Failure> {
    supervisor.install(&repository(&owner, &repo), &sha256).await?;
    Ok(Json(supervisor.list().await?))
}

async fn check(State(supervisor): State<Supervisor>) -> Result<Json<HubInfo>, Failure> {
    supervisor.check().await?;
    Ok(Json(supervisor.list().await?))
}

async fn preview_update(
    State(supervisor): State<Supervisor>,
    Path(slug): Path<String>,
) -> Result<Json<InstallPreview>, Failure> {
    Ok(Json(supervisor.preview_update(&slug).await?))
}

async fn apply_update(
    State(supervisor): State<Supervisor>,
    Path((slug, sha256)): Path<(String, String)>,
) -> Result<Json<HubInfo>, Failure> {
    supervisor.apply_update(&slug, &sha256).await?;
    Ok(Json(supervisor.list().await?))
}

async fn update(
    State(supervisor): State<Supervisor>,
    Path(slug): Path<String>,
) -> Result<Json<HubInfo>, Failure> {
    supervisor.update(&slug).await?;
    Ok(Json(supervisor.list().await?))
}

async fn rollback(
    State(supervisor): State<Supervisor>,
    Path(slug): Path<String>,
) -> Result<Json<HubInfo>, Failure> {
    supervisor.rollback(&slug).await?;
    Ok(Json(supervisor.list().await?))
}

async fn settings(State(supervisor): State<Supervisor>) -> Result<Json<HubSettings>, Failure> {
    Ok(Json(supervisor.settings().await?))
}

async fn set_check_at_start(
    State(supervisor): State<Supervisor>,
    Path(switch): Path<String>,
) -> Result<Json<HubSettings>, Failure> {
    let on = match switch.as_str() {
        "on" => true,
        "off" => false,
        _ => {
            tracing::debug!(switch, "hub request refused: neither on nor off");
            return Err(Failure {
                status: StatusCode::BAD_REQUEST,
                body: ApiError {
                    code: "hub_invalid_switch".to_owned(),
                    message: format!("`{switch}` is neither `on` nor `off`"),
                },
            });
        }
    };
    Ok(Json(supervisor.set_check_updates_at_start(on).await?))
}
