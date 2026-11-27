//! HTTP adapter over the application services.
//!
//! Handlers translate requests into [`App`] calls and nothing more. The
//! middleware below is the whole security boundary of the local server; see
//! `docs/architecture/security.md`.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use eario_core::{
    ApiError, AppResponse, ComponentData, ComponentId, DatasetId, Preferences, StateKey,
    StateUpdate,
};
use eario_data::DataError;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::watch;

use crate::app::App;
use crate::assets;
use crate::error::AppError;

/// Header carrying the session token on API requests.
pub(crate) const TOKEN_HEADER: &str = "x-eario-token";

/// What the server accepts requests from.
#[derive(Debug)]
pub(crate) struct Security {
    /// Secret generated at start-up and handed to the browser in the URL
    /// fragment. Required on every API request.
    pub(crate) token: String,
    /// Accepted values of the `Host` header.
    pub(crate) hosts: Vec<String>,
    /// Accepted values of the `Origin` header, when the header is present.
    pub(crate) origins: Vec<String>,
}

impl Security {
    /// Accepts the loopback names of the server on `port`, plus `extra_origin`
    /// when a frontend development server is in use.
    pub(crate) fn for_port(token: String, port: u16, extra_origin: Option<String>) -> Self {
        let hosts = vec![format!("127.0.0.1:{port}"), format!("localhost:{port}")];
        let mut origins: Vec<String> = hosts.iter().map(|host| format!("http://{host}")).collect();
        origins.extend(extra_origin);
        Self { token, hosts, origins }
    }
}

/// What the protection middleware and the shutdown route need. It carries no
/// application: each route set brings its own state.
#[derive(Clone)]
struct HttpState {
    security: Arc<Security>,
    shutdown: watch::Sender<bool>,
}

/// The API of a dashboard: the routes served under `/api` for an [`App`], with
/// the application as their state.
///
/// Paths are relative to `/api`; pass the result to [`router`].
pub(crate) fn dashboard_api(app: App, max_upload_bytes: usize) -> Router {
    Router::new()
        .route("/app", get(app_info))
        .route("/state/{key}", put(set_state))
        .route("/components/{id}/data", get(component_data))
        .route(
            "/datasets/{id}/upload",
            post(upload_dataset).layer(DefaultBodyLimit::max(max_upload_bytes)),
        )
        .route("/datasets/{id}/sheet", put(select_sheet))
        .route("/preferences", put(set_preferences))
        .with_state(app)
}

/// Builds the complete router around a set of API routes: the routes
/// themselves, `POST /api/shutdown`, optional extra routes and the frontend.
///
/// `api_routes` are mounted under `/api` and have their state applied already
/// (see [`dashboard_api`]). A route set must not define `/shutdown`, which
/// every server answers the same way. `extra_routes` are mounted at the top
/// level. Both get the same protection: the Host and Origin check, and the
/// session token. The frontend is served without a token.
pub(crate) fn router(
    api_routes: Router,
    security: Arc<Security>,
    shutdown: watch::Sender<bool>,
    extra_routes: Option<Router>,
) -> Router {
    let state = HttpState { security, shutdown };

    let shutdown_route =
        Router::new().route("/shutdown", post(request_shutdown)).with_state(state.shutdown.clone());
    let api = api_routes.merge(shutdown_route).fallback(unknown_endpoint);

    let mut protected = Router::new().nest("/api", api);
    if let Some(extra_routes) = extra_routes {
        protected = protected.merge(extra_routes);
    }
    let protected = protected.layer(middleware::from_fn_with_state(state.clone(), require_token));

    protected
        .fallback(assets::serve)
        .layer(middleware::from_fn_with_state(state, check_host_and_origin))
}

/// A failed request: a status and a JSON [`ApiError`] body.
struct Failure {
    status: StatusCode,
    body: ApiError,
}

impl Failure {
    fn new(status: StatusCode, code: &str, message: impl Into<String>) -> Self {
        Self { status, body: ApiError { code: code.to_owned(), message: message.into() } }
    }
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

impl From<AppError> for Failure {
    fn from(error: AppError) -> Self {
        let status = match &error {
            AppError::UnknownComponent(_)
            | AppError::UnknownDataset(_)
            | AppError::UnknownState(_) => StatusCode::NOT_FOUND,
            AppError::DatasetNotLoaded(_)
            | AppError::DerivedDataset(_)
            | AppError::NotComputed(_) => StatusCode::CONFLICT,
            AppError::InvalidStateValue { .. } | AppError::Computation { .. } => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            AppError::Data(DataError::Engine(_))
            | AppError::Spec(_)
            | AppError::Preferences { .. }
            | AppError::UnknownComputation { .. }
            | AppError::UndeclaredComputationInput { .. }
            | AppError::MissingComputationInput { .. }
            | AppError::UndeclaredComputationParameter { .. }
            | AppError::MissingComputationParameter { .. }
            | AppError::SelectDependsOnItsState { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::Data(_) => StatusCode::UNPROCESSABLE_ENTITY,
        };
        if status.is_server_error() {
            tracing::error!(%error, "request failed");
        } else {
            tracing::debug!(%error, "request rejected");
        }
        Self::new(status, error.code(), error.to_string())
    }
}

/// Runs a CPU-bound service call off the asynchronous runtime.
async fn blocking<T, F>(call: F) -> Result<Json<T>, Failure>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
{
    match tokio::task::spawn_blocking(call).await {
        Ok(result) => Ok(Json(result?)),
        Err(error) => {
            tracing::error!(%error, "service call panicked or was cancelled");
            Err(Failure::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "The operation failed unexpectedly.",
            ))
        }
    }
}

/// Compares two strings in time independent of where they differ.
fn constant_time_eq(left: &str, right: &str) -> bool {
    left.len() == right.len()
        && left.bytes().zip(right.bytes()).fold(0, |difference, (a, b)| difference | (a ^ b)) == 0
}

fn header_text(headers: &HeaderMap, name: impl header::AsHeaderName) -> Option<&str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

/// Rejects requests that do not address the loopback server by name, which
/// defeats DNS rebinding, and requests sent by a page of another origin.
async fn check_host_and_origin(
    State(state): State<HttpState>,
    request: Request,
    next: Next,
) -> Response {
    let headers = request.headers();
    let host_accepted = header_text(headers, header::HOST)
        .is_some_and(|host| state.security.hosts.iter().any(|accepted| accepted == host));
    if !host_accepted {
        return Failure::new(StatusCode::FORBIDDEN, "forbidden_host", "Unexpected Host header.")
            .into_response();
    }
    if let Some(origin) = headers.get(header::ORIGIN) {
        let origin_accepted = origin
            .to_str()
            .is_ok_and(|origin| state.security.origins.iter().any(|accepted| accepted == origin));
        if !origin_accepted {
            return Failure::new(
                StatusCode::FORBIDDEN,
                "forbidden_origin",
                "Requests from this origin are not accepted.",
            )
            .into_response();
        }
    }

    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    response
}

/// Requires the session token, in the `x-eario-token` header or as a bearer
/// token. A page of another origin cannot read the token, so it cannot forge
/// a request.
async fn require_token(State(state): State<HttpState>, request: Request, next: Next) -> Response {
    let headers = request.headers();
    let presented = header_text(headers, TOKEN_HEADER).or_else(|| {
        header_text(headers, header::AUTHORIZATION).and_then(|value| value.strip_prefix("Bearer "))
    });
    if presented.is_some_and(|presented| constant_time_eq(presented, &state.security.token)) {
        next.run(request).await
    } else {
        Failure::new(
            StatusCode::UNAUTHORIZED,
            "missing_token",
            "Open the application with the address printed when it started.",
        )
        .into_response()
    }
}

async fn unknown_endpoint() -> Failure {
    Failure::new(StatusCode::NOT_FOUND, "unknown_endpoint", "This API endpoint does not exist.")
}

async fn app_info(State(app): State<App>) -> Json<AppResponse> {
    Json(AppResponse::Dashboard(app.info()))
}

async fn set_state(
    State(app): State<App>,
    Path(key): Path<StateKey>,
    Json(value): Json<Value>,
) -> Result<Json<StateUpdate>, Failure> {
    blocking(move || app.set_state(&key, value)).await
}

#[derive(Debug, Deserialize)]
struct DataQuery {
    /// First row of the requested page. Only tables use it.
    #[serde(default)]
    offset: u32,
}

async fn component_data(
    State(app): State<App>,
    Path(id): Path<ComponentId>,
    Query(query): Query<DataQuery>,
) -> Result<Json<ComponentData>, Failure> {
    blocking(move || app.component_data(&id, query.offset)).await
}

#[derive(Debug, Deserialize)]
struct UploadQuery {
    /// Name of the uploaded file. Only its extension and its display matter:
    /// it is never used as a path.
    filename: String,
}

async fn upload_dataset(
    State(app): State<App>,
    Path(id): Path<DatasetId>,
    Query(query): Query<UploadQuery>,
    body: Bytes,
) -> Result<Json<StateUpdate>, Failure> {
    // Keep the last path segment in case a client sends a full path.
    let name = query.filename.rsplit(['/', '\\']).next().unwrap_or_default().to_owned();
    blocking(move || app.import_dataset(&id, &name, &body)).await
}

#[derive(Debug, Deserialize)]
struct SheetSelection {
    sheet: String,
}

async fn select_sheet(
    State(app): State<App>,
    Path(id): Path<DatasetId>,
    Json(selection): Json<SheetSelection>,
) -> Result<Json<StateUpdate>, Failure> {
    blocking(move || app.select_sheet(&id, &selection.sheet)).await
}

async fn set_preferences(
    State(app): State<App>,
    Json(preferences): Json<Preferences>,
) -> Result<Json<Preferences>, Failure> {
    blocking(move || app.set_preferences(preferences).map(|()| preferences)).await
}

async fn request_shutdown(State(shutdown): State<watch::Sender<bool>>) -> StatusCode {
    tracing::info!("shutdown requested from the user interface");
    // Sending only fails when the server is already stopping.
    let _ = shutdown.send(true);
    StatusCode::NO_CONTENT
}
