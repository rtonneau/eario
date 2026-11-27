//! HTTP adapter over the [`Supervisor`].
//!
//! Routes, relative to `/api`; `{slug}` is the only parameter of any route:
//!
//! | Route | Answer |
//! | --- | --- |
//! | `GET /app` | `AppResponse` of kind `hub`: the installed dashboards and their state |
//! | `GET /alive` | `204`, at once: tells a second hub start that this hub runs |
//! | `POST /dashboards/{slug}/start` | `HubInfo`, once the process is created (the dashboard is `starting`) |
//! | `POST /dashboards/{slug}/stop` | `HubInfo`, once the dashboard is stopped |
//! | `POST /dashboards/{slug}/open` | `HubOpen`: the dashboard's address with its token |
//! | `POST /dashboards/{slug}/uninstall` | `HubInfo`, without the dashboard |
//! | `POST /stop-all` | `HubInfo`, once every dashboard is stopped |
//! | `POST /shutdown` | Added by the server: quits the hub. Dashboards keep running. |
//!
//! No route reads a request body or a query string, so none can be given a
//! path, an executable or arguments. A slug that is not one is refused with
//! 400. The server puts every route behind the Host and Origin check and the
//! session token, as for a dashboard.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use eario_core::{ApiError, AppResponse, HubInfo, HubOpen};
use eario_runtime::HubDirError;

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
