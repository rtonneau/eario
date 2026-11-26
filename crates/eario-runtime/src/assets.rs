//! The generic frontend, embedded into the executable.

use axum::http::{HeaderValue, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

/// Build output of the `frontend/` project. Release builds embed the files;
/// debug builds read them from disk, so a frontend rebuild needs no
/// recompilation.
#[derive(RustEmbed)]
#[folder = "frontend-dist/"]
struct Assets;

const INDEX: &str = "index.html";

/// Restricts the page to its own origin: no remote script, style, font,
/// image or connection. ECharts needs inline styles for its tooltips.
const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; style-src 'self' 'unsafe-inline'; \
     img-src 'self' data: blob:; object-src 'none'; base-uri 'none'; frame-ancestors 'none'";

/// Serves an embedded file. Paths that name no file and have no extension
/// are application routes and receive the index page.
///
/// The path is only ever used as a key into the embedded file table, so it
/// cannot reach the file system.
pub(crate) async fn serve(method: Method, uri: Uri) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { INDEX } else { path };
    if let Some(file) = Assets::get(path) {
        return respond(path, file.data.into_owned());
    }
    let is_route = !path.rsplit('/').next().unwrap_or(path).contains('.');
    match Assets::get(INDEX) {
        Some(index) if is_route => respond(INDEX, index.data.into_owned()),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

fn respond(path: &str, body: Vec<u8>) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let content_type = HeaderValue::from_str(mime.as_ref())
        .unwrap_or(HeaderValue::from_static("application/octet-stream"));
    let mut response = body.into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, content_type);
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    if path == INDEX {
        headers.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(CONTENT_SECURITY_POLICY),
        );
    }
    response
}
