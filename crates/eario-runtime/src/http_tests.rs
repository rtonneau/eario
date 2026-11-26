//! Tests of the HTTP API, driven through the router as a browser would.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use eario_core::{
    ChartMark, ComponentId, ComponentKind, ComponentSpec, DashboardSpec, DataView, DatasetId,
    DatasetSlot, PageSpec, RowSpec, SPEC_VERSION, StateKey, Theme,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tokio::sync::watch;
use tower::ServiceExt;

use crate::app::{App, AppOptions};
use crate::http::{Security, TOKEN_HEADER, router};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const PORT: u16 = 4780;
const HOST: &str = "127.0.0.1:4780";
const TOKEN: &str = "test-token";
const UPLOAD_LIMIT: usize = 64 * 1024;

const CSV: &str = include_str!("../../../fixtures/csv/measurements.csv");
const MALFORMED_CSV: &str = include_str!("../../../fixtures/csv/malformed.csv");
const XLSX: &[u8] = include_bytes!("../../../fixtures/excel/samples.xlsx");

fn spec() -> DashboardSpec {
    let data = DatasetId::new("data");
    let x = StateKey::new("x");
    let y = StateKey::new("y");
    let filter = StateKey::new("filter");
    let view = DataView::of(&data).filtered_by(&filter);
    let component =
        |id: &str, kind| ComponentSpec { id: ComponentId::new(id), title: None, span: 6, kind };
    let column_select = |state: &StateKey, default_index| ComponentKind::ColumnSelect {
        dataset: data.clone(),
        state: state.clone(),
        label: state.to_string(),
        numeric_only: true,
        default_index,
    };
    let items = vec![
        component("import", ComponentKind::FileImport { dataset: data.clone() }),
        component("info", ComponentKind::DatasetInfo { dataset: data.clone() }),
        component("x", column_select(&x, 0)),
        component("y", column_select(&y, 1)),
        component(
            "filter",
            ComponentKind::FilterInput { dataset: data.clone(), state: filter.clone() },
        ),
        component(
            "chart",
            ComponentKind::Chart {
                view: view.clone(),
                mark: ChartMark::Scatter,
                x: x.clone(),
                y: y.clone(),
                max_points: 10,
            },
        ),
        component("table", ComponentKind::DataTable { view: view.clone(), page_size: 5 }),
        component("stats", ComponentKind::Statistics { view }),
    ];
    DashboardSpec {
        spec_version: SPEC_VERSION,
        title: "Test".to_owned(),
        datasets: vec![DatasetSlot { id: data, label: "Data".to_owned() }],
        pages: vec![PageSpec {
            id: "main".to_owned(),
            title: "Main".to_owned(),
            rows: vec![RowSpec { items }],
        }],
    }
}

/// A router over a fresh application, plus the handles tests inspect.
struct Harness {
    router: Router,
    app: App,
    shutdown: watch::Receiver<bool>,
}

fn harness() -> Result<Harness, Box<dyn std::error::Error>> {
    let app = App::new(spec(), AppOptions::default())?;
    let security = Arc::new(Security::for_port(TOKEN.to_owned(), PORT, None));
    let (sender, shutdown) = watch::channel(false);
    let router = router(app.clone(), security, sender, UPLOAD_LIMIT, None);
    Ok(Harness { router, app, shutdown })
}

/// Starts a request carrying the headers a legitimate client sends.
fn request(method: Method, uri: &str) -> axum::http::request::Builder {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, HOST)
        .header(TOKEN_HEADER, TOKEN)
}

impl Harness {
    async fn send(
        &self,
        request: Request<Body>,
    ) -> Result<(StatusCode, Vec<u8>), Box<dyn std::error::Error>> {
        let response = self.router.clone().oneshot(request).await?;
        let status = response.status();
        let body = response.into_body().collect().await?.to_bytes().to_vec();
        Ok((status, body))
    }

    async fn json(
        &self,
        request: Request<Body>,
    ) -> Result<(StatusCode, Value), Box<dyn std::error::Error>> {
        let (status, body) = self.send(request).await?;
        let value = if body.is_empty() { Value::Null } else { serde_json::from_slice(&body)? };
        Ok((status, value))
    }

    async fn get(&self, uri: &str) -> Result<(StatusCode, Value), Box<dyn std::error::Error>> {
        self.json(request(Method::GET, uri).body(Body::empty())?).await
    }

    async fn put(
        &self,
        uri: &str,
        body: Value,
    ) -> Result<(StatusCode, Value), Box<dyn std::error::Error>> {
        let request = request(Method::PUT, uri)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))?;
        self.json(request).await
    }

    async fn upload(
        &self,
        filename: &str,
        bytes: impl Into<Body>,
    ) -> Result<(StatusCode, Value), Box<dyn std::error::Error>> {
        let uri = format!("/api/datasets/data/upload?filename={filename}");
        self.json(request(Method::POST, &uri).body(bytes.into())?).await
    }
}

fn ids(value: &Value) -> Vec<&str> {
    value.as_array().map(|ids| ids.iter().filter_map(Value::as_str).collect()).unwrap_or_default()
}

#[tokio::test]
async fn api_request_without_token_is_unauthorized() -> TestResult {
    let harness = harness()?;
    let request =
        Request::builder().uri("/api/app").header(header::HOST, HOST).body(Body::empty())?;
    let (status, body) = harness.json(request).await?;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], "missing_token");
    Ok(())
}

#[tokio::test]
async fn api_request_with_wrong_token_is_unauthorized() -> TestResult {
    let harness = harness()?;
    let request = Request::builder()
        .uri("/api/app")
        .header(header::HOST, HOST)
        .header(TOKEN_HEADER, "test-tokeN")
        .body(Body::empty())?;
    assert_eq!(harness.send(request).await?.0, StatusCode::UNAUTHORIZED);
    Ok(())
}

#[tokio::test]
async fn bearer_token_is_accepted() -> TestResult {
    let harness = harness()?;
    let request = Request::builder()
        .uri("/api/app")
        .header(header::HOST, HOST)
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .body(Body::empty())?;
    assert_eq!(harness.send(request).await?.0, StatusCode::OK);
    Ok(())
}

#[tokio::test]
async fn request_for_another_host_is_forbidden() -> TestResult {
    let harness = harness()?;
    for host in ["attacker.example:4780", "127.0.0.1:9999", "127.0.0.1"] {
        let request = Request::builder()
            .uri("/api/app")
            .header(header::HOST, host)
            .header(TOKEN_HEADER, TOKEN)
            .body(Body::empty())?;
        let (status, body) = harness.json(request).await?;
        assert_eq!(status, StatusCode::FORBIDDEN, "{host}");
        assert_eq!(body["code"], "forbidden_host", "{host}");
    }
    Ok(())
}

#[tokio::test]
async fn request_from_another_origin_is_forbidden() -> TestResult {
    let harness = harness()?;
    let request = request(Method::POST, "/api/shutdown")
        .header(header::ORIGIN, "https://attacker.example")
        .body(Body::empty())?;
    let (status, body) = harness.json(request).await?;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "forbidden_origin");
    assert!(!*harness.shutdown.borrow(), "the application must keep running");
    Ok(())
}

#[tokio::test]
async fn request_from_own_origin_is_accepted() -> TestResult {
    let harness = harness()?;
    let request = request(Method::GET, "/api/app")
        .header(header::ORIGIN, "http://localhost:4780")
        .header(header::HOST, "localhost:4780")
        .body(Body::empty())?;
    // The builder already carries the 127.0.0.1 Host header; replace it.
    let (mut parts, body) = request.into_parts();
    parts.headers.insert(header::HOST, "localhost:4780".parse()?);
    assert_eq!(harness.send(Request::from_parts(parts, body)).await?.0, StatusCode::OK);
    Ok(())
}

#[tokio::test]
async fn app_info_starts_with_empty_state() -> TestResult {
    let harness = harness()?;
    let (status, info) = harness.get("/api/app").await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(info["spec"]["title"], "Test");
    assert_eq!(info["state"], json!({ "filter": null, "x": null, "y": null }));
    assert_eq!(info["revision"], 0);
    assert_eq!(info["preferences"]["theme"], "system");
    Ok(())
}

#[tokio::test]
async fn components_report_no_dataset_before_an_import() -> TestResult {
    let harness = harness()?;
    for id in ["info", "x", "filter", "chart", "table", "stats"] {
        let (status, data) = harness.get(&format!("/api/components/{id}/data")).await?;
        assert_eq!(status, StatusCode::OK, "{id}");
        assert_eq!(data["type"], "empty", "{id}");
    }
    Ok(())
}

#[tokio::test]
async fn csv_upload_selects_default_columns_and_invalidates_dependents() -> TestResult {
    let harness = harness()?;
    let (status, update) = harness.upload("measurements.csv", CSV).await?;
    assert_eq!(status, StatusCode::OK, "{update}");
    assert_eq!(update["revision"], 1);
    assert_eq!(update["state"]["x"], "energy_keV");
    assert_eq!(update["state"]["y"], "counts");
    assert_eq!(update["state"]["filter"], Value::Null);
    assert_eq!(
        ids(&update["invalidated"]),
        ["import", "info", "x", "y", "filter", "chart", "table", "stats"]
    );

    let (_, info) = harness.get("/api/components/info/data").await?;
    assert_eq!(info["type"], "dataset");
    assert_eq!(info["info"]["source_name"], "measurements.csv");
    assert_eq!(info["info"]["rows"], 40);

    let (_, options) = harness.get("/api/components/x/data").await?;
    assert_eq!(options["options"], json!(["energy_keV", "counts", "dead_time"]));
    Ok(())
}

#[tokio::test]
async fn chart_is_downsampled_and_follows_the_selected_columns() -> TestResult {
    let harness = harness()?;
    harness.upload("measurements.csv", CSV).await?;

    let (_, chart) = harness.get("/api/components/chart/data").await?;
    assert_eq!(chart["type"], "chart");
    assert_eq!(chart["chart"]["x_label"], "energy_keV");
    assert_eq!(chart["chart"]["y_label"], "counts");
    // 40 rows, 2 without counts; the component allows 10 points.
    assert_eq!(chart["chart"]["total_points"], 38);
    assert_eq!(chart["chart"]["skipped_rows"], 2);
    assert_eq!(chart["chart"]["points"].as_array().map(Vec::len), Some(10));

    let (status, update) = harness.put("/api/state/y", json!("dead_time")).await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ids(&update["invalidated"]), ["chart"]);
    let (_, chart) = harness.get("/api/components/chart/data").await?;
    assert_eq!(chart["chart"]["y_label"], "dead_time");
    assert_eq!(chart["chart"]["total_points"], 40);
    Ok(())
}

#[tokio::test]
async fn filter_narrows_table_chart_and_statistics_only() -> TestResult {
    let harness = harness()?;
    harness.upload("measurements.csv", CSV).await?;

    let filter = json!({ "column": "detector", "op": "eq", "value": "A" });
    let (status, update) = harness.put("/api/state/filter", filter).await?;
    assert_eq!(status, StatusCode::OK, "{update}");
    assert_eq!(update["revision"], 2);
    assert_eq!(ids(&update["invalidated"]), ["chart", "table", "stats"]);

    let (_, table) = harness.get("/api/components/table/data?offset=5").await?;
    assert_eq!(table["table"]["total_rows"], 20);
    assert_eq!(table["table"]["unfiltered_rows"], 40);
    assert_eq!(table["table"]["offset"], 5);
    assert_eq!(table["table"]["rows"].as_array().map(Vec::len), Some(5));

    let (_, stats) = harness.get("/api/components/stats/data").await?;
    assert_eq!(stats["stats"]["rows"], 20);

    let (_, cleared) = harness.put("/api/state/filter", Value::Null).await?;
    assert_eq!(cleared["revision"], 3);
    let (_, table) = harness.get("/api/components/table/data").await?;
    assert_eq!(table["table"]["total_rows"], 40);
    Ok(())
}

#[tokio::test]
async fn setting_a_state_to_its_current_value_changes_nothing() -> TestResult {
    let harness = harness()?;
    harness.upload("measurements.csv", CSV).await?;
    let (_, update) = harness.put("/api/state/x", json!("energy_keV")).await?;
    assert_eq!(update["revision"], 1);
    assert_eq!(ids(&update["invalidated"]), Vec::<&str>::new());
    Ok(())
}

#[tokio::test]
async fn invalid_state_values_are_rejected_with_a_reason() -> TestResult {
    let harness = harness()?;
    harness.upload("measurements.csv", CSV).await?;

    let (status, error) = harness.put("/api/state/x", json!("sample")).await?;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error["code"], "invalid_state_value");

    let (status, error) = harness
        .put("/api/state/filter", json!({ "column": "energy_keV", "op": "gt", "value": "high" }))
        .await?;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error["code"], "invalid_filter");

    let (status, error) = harness
        .put("/api/state/filter", json!({ "column": "mass", "op": "gt", "value": 1 }))
        .await?;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error["code"], "unknown_column");

    let (status, error) = harness.put("/api/state/unknown", json!(1)).await?;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error["code"], "unknown_state");

    let (_, info) = harness.get("/api/app").await?;
    assert_eq!(info["revision"], 1, "rejected values must not change the state");
    Ok(())
}

#[tokio::test]
async fn workbook_upload_lists_sheets_and_a_sheet_can_be_selected() -> TestResult {
    let harness = harness()?;
    let (status, update) = harness.upload("samples.xlsx", XLSX).await?;
    assert_eq!(status, StatusCode::OK, "{update}");
    assert_eq!(update["state"]["x"], "energy_keV");

    let (_, info) = harness.get("/api/components/import/data").await?;
    assert_eq!(info["info"]["sheets"], json!(["Measurements", "Calibration", "Empty"]));
    assert_eq!(info["info"]["selected_sheet"], "Measurements");

    let (status, update) =
        harness.put("/api/datasets/data/sheet", json!({ "sheet": "Calibration" })).await?;
    assert_eq!(status, StatusCode::OK, "{update}");
    // `counts` does not exist in the new sheet: y falls back to its default.
    assert_eq!(update["state"]["x"], "energy_keV");
    assert_eq!(update["state"]["y"], "energy_keV");
    let (_, info) = harness.get("/api/components/info/data").await?;
    assert_eq!(info["info"]["selected_sheet"], "Calibration");
    assert_eq!(info["info"]["rows"], 8);

    let (status, error) =
        harness.put("/api/datasets/data/sheet", json!({ "sheet": "Results" })).await?;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error["code"], "unknown_sheet");
    Ok(())
}

#[tokio::test]
async fn failed_import_reports_the_reason_and_keeps_the_previous_dataset() -> TestResult {
    let harness = harness()?;
    harness.upload("measurements.csv", CSV).await?;

    let (status, error) = harness.upload("malformed.csv", MALFORMED_CSV).await?;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error["code"], "parse_error");
    assert!(error["message"].as_str().is_some_and(|message| !message.is_empty()));

    let (status, error) = harness.upload("notes.pdf", "%PDF").await?;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error["code"], "unsupported_format");

    let (status, error) = harness.upload("empty.csv", "").await?;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error["code"], "empty_source");

    let (_, info) = harness.get("/api/components/info/data").await?;
    assert_eq!(info["info"]["source_name"], "measurements.csv");
    Ok(())
}

#[tokio::test]
async fn upload_keeps_only_the_file_name_of_a_path() -> TestResult {
    let harness = harness()?;
    let (status, _) = harness.upload("..%2F..%2Fsecret%5Cmeasurements.csv", CSV).await?;
    assert_eq!(status, StatusCode::OK);
    let (_, info) = harness.get("/api/components/info/data").await?;
    assert_eq!(info["info"]["source_name"], "measurements.csv");
    Ok(())
}

#[tokio::test]
async fn upload_larger_than_the_limit_is_rejected() -> TestResult {
    let harness = harness()?;
    let oversized = vec![b'1'; UPLOAD_LIMIT + 1];
    let request = request(Method::POST, "/api/datasets/data/upload?filename=big.csv")
        .body(Body::from(oversized))?;
    assert_eq!(harness.send(request).await?.0, StatusCode::PAYLOAD_TOO_LARGE);
    Ok(())
}

#[tokio::test]
async fn upload_to_an_undeclared_dataset_is_not_found() -> TestResult {
    let harness = harness()?;
    let request =
        request(Method::POST, "/api/datasets/other/upload?filename=a.csv").body(Body::from(CSV))?;
    let (status, error) = harness.json(request).await?;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error["code"], "unknown_dataset");
    Ok(())
}

#[tokio::test]
async fn unknown_component_and_endpoint_are_not_found() -> TestResult {
    let harness = harness()?;
    let (status, error) = harness.get("/api/components/nope/data").await?;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error["code"], "unknown_component");

    let (status, error) = harness.get("/api/nope").await?;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error["code"], "unknown_endpoint");
    Ok(())
}

#[tokio::test]
async fn preferences_are_stored() -> TestResult {
    let harness = harness()?;
    let (status, stored) = harness.put("/api/preferences", json!({ "theme": "dark" })).await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["theme"], "dark");
    assert_eq!(harness.app.preferences().theme, Theme::Dark);
    Ok(())
}

#[tokio::test]
async fn shutdown_endpoint_signals_the_server() -> TestResult {
    let harness = harness()?;
    let request = request(Method::POST, "/api/shutdown").body(Body::empty())?;
    assert_eq!(harness.send(request).await?.0, StatusCode::NO_CONTENT);
    assert!(*harness.shutdown.borrow());
    Ok(())
}

#[tokio::test]
async fn frontend_is_served_without_a_token_and_with_security_headers() -> TestResult {
    let harness = harness()?;
    let request = Request::builder().uri("/").header(header::HOST, HOST).body(Body::empty())?;
    let response = harness.router.clone().oneshot(request).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers();
    assert!(
        headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/html"))
    );
    assert!(headers.contains_key(header::CONTENT_SECURITY_POLICY));
    assert_eq!(
        headers.get(header::X_CONTENT_TYPE_OPTIONS).and_then(|value| value.to_str().ok()),
        Some("nosniff")
    );
    Ok(())
}

#[tokio::test]
async fn paths_outside_the_embedded_frontend_are_not_served() -> TestResult {
    let harness = harness()?;
    for path in ["/../Cargo.toml", "/..%2F..%2FCargo.toml", "/assets/../../build.rs", "/missing.js"]
    {
        let request = Request::builder().uri(path).header(header::HOST, HOST).body(Body::empty())?;
        assert_eq!(harness.send(request).await?.0, StatusCode::NOT_FOUND, "{path}");
    }
    Ok(())
}

#[tokio::test]
async fn unknown_route_without_extension_gets_the_index_page() -> TestResult {
    let harness = harness()?;
    let request =
        Request::builder().uri("/pages/rows").header(header::HOST, HOST).body(Body::empty())?;
    assert_eq!(harness.send(request).await?.0, StatusCode::OK);
    Ok(())
}

#[tokio::test]
async fn frontend_routes_only_answer_read_requests() -> TestResult {
    let harness = harness()?;
    let request = request(Method::POST, "/mcp").body(Body::empty())?;
    assert_eq!(harness.send(request).await?.0, StatusCode::METHOD_NOT_ALLOWED);
    Ok(())
}
