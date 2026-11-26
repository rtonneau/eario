//! Tests of the HTTP API, driven through the router as a browser would.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use eario_core::{
    ChartMark, ComponentId, ComponentKind, ComponentSpec, DashboardSpec, DataView, DatasetId,
    DatasetSlot, DatasetSource, PageSpec, ParamBinding, RowSpec, SPEC_VERSION, StateKey, Theme,
};
use eario_data::polars::prelude::Column;
use eario_data::{Computation, ComputationRegistry, ComputeError, DataFrame, Dataset};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tokio::sync::watch;
use tower::ServiceExt;

use crate::app::{App, AppOptions};
use crate::error::AppError;
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
        datasets: vec![DatasetSlot {
            id: data,
            label: "Data".to_owned(),
            source: DatasetSource::Imported,
        }],
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
    harness_for(spec(), AppOptions::default())
}

fn harness_for(
    spec: DashboardSpec,
    options: AppOptions,
) -> Result<Harness, Box<dyn std::error::Error>> {
    let app = App::new(spec, options)?;
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

// Derived datasets.

/// A dashboard reading derived datasets of `data`:
///
/// - `selected`: the column of `data` chosen in `x`. A column selection `s`
///   and a filter input `sfilter` are bound to it, so it is recomputed when
///   `x` or `data` changes;
/// - `count`: the number of rows of `selected`, read by `count_table` only;
/// - `broken` and `crashing`: a computation failing with an error, and with
///   a panic.
fn derived_spec() -> DashboardSpec {
    let data = DatasetId::new("data");
    let selected = DatasetId::new("selected");
    let x = StateKey::new("x");
    let s = StateKey::new("s");
    let filter = StateKey::new("filter");
    let selected_filter = StateKey::new("sfilter");
    let component =
        |id: &str, kind| ComponentSpec { id: ComponentId::new(id), title: None, span: 6, kind };
    let column_select = |dataset: &DatasetId, state: &StateKey| ComponentKind::ColumnSelect {
        dataset: dataset.clone(),
        state: state.clone(),
        label: format!("{state} column"),
        numeric_only: true,
        default_index: 0,
    };
    let table = |dataset: &str| ComponentKind::DataTable {
        view: DataView::of(&DatasetId::new(dataset)),
        page_size: 5,
    };
    let derived =
        |id: &str, function: &str, input: &DatasetId, param: (&str, ParamBinding)| DatasetSlot {
            id: DatasetId::new(id),
            label: id.to_owned(),
            source: DatasetSource::Computed {
                function: function.to_owned(),
                inputs: BTreeMap::from([("table".to_owned(), DataView::of(input))]),
                params: BTreeMap::from([(param.0.to_owned(), param.1)]),
            },
        };
    let items = vec![
        component("import", ComponentKind::FileImport { dataset: data.clone() }),
        component("x", column_select(&data, &x)),
        component(
            "filter",
            ComponentKind::FilterInput { dataset: data.clone(), state: filter.clone() },
        ),
        component(
            "stats",
            ComponentKind::Statistics { view: DataView::of(&data).filtered_by(&filter) },
        ),
        component("s", column_select(&selected, &s)),
        component(
            "sfilter",
            ComponentKind::FilterInput {
                dataset: selected.clone(),
                state: selected_filter.clone(),
            },
        ),
        component("selected_info", ComponentKind::DatasetInfo { dataset: selected.clone() }),
        component(
            "selected_table",
            ComponentKind::DataTable {
                view: DataView::of(&selected).filtered_by(&selected_filter),
                page_size: 5,
            },
        ),
        component(
            "chart",
            ComponentKind::Chart {
                view: DataView::of(&selected),
                mark: ChartMark::Line,
                x: s.clone(),
                y: s,
                max_points: 10,
            },
        ),
        component("count_table", table("count")),
        component("broken_table", table("broken")),
        component("crashing_table", table("crashing")),
    ];
    DashboardSpec {
        spec_version: SPEC_VERSION,
        title: "Derived".to_owned(),
        datasets: vec![
            DatasetSlot {
                id: data.clone(),
                label: "Data".to_owned(),
                source: DatasetSource::Imported,
            },
            derived("selected", "select_column", &data, ("column", ParamBinding::State(x))),
            derived(
                "count",
                "count_rows",
                &selected,
                ("label", ParamBinding::Literal(json!("rows"))),
            ),
            derived("broken", "fail", &data, ("panic", ParamBinding::Literal(json!(false)))),
            derived("crashing", "fail", &data, ("panic", ParamBinding::Literal(json!(true)))),
        ],
        pages: vec![PageSpec {
            id: "main".to_owned(),
            title: "Main".to_owned(),
            rows: vec![RowSpec { items }],
        }],
    }
}

/// How many times each computation of the derived harness ran.
#[derive(Default)]
struct Calls {
    select: AtomicUsize,
    count: AtomicUsize,
    fail: AtomicUsize,
}

fn calls(counter: &AtomicUsize) -> usize {
    counter.load(Ordering::SeqCst)
}

/// A harness over `derived_spec`. `count_rows` takes `count_delay` to run.
fn derived_harness(
    count_delay: Duration,
) -> Result<(Harness, Arc<Calls>), Box<dyn std::error::Error>> {
    let counters = Arc::new(Calls::default());
    let mut computations = ComputationRegistry::default();

    let calls = Arc::clone(&counters);
    computations.register(
        Computation::new("select_column", move |ctx| {
            calls.select.fetch_add(1, Ordering::SeqCst);
            let column: String = ctx.param("column")?;
            if column == "dead_time" {
                return Err(ComputeError::new("dead time cannot be selected"));
            }
            Ok(ctx.input("table")?.select([column.as_str()])?)
        })
        .input("table")
        .param("column"),
    );

    let calls = Arc::clone(&counters);
    computations.register(
        Computation::new("count_rows", move |ctx| {
            calls.count.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(count_delay);
            let label: String = ctx.param("label")?;
            let rows = ctx.input("table")?.height() as u64;
            Ok(DataFrame::new(1, vec![Column::new(label.as_str().into(), [rows])])?)
        })
        .input("table")
        .param("label"),
    );

    let calls = Arc::clone(&counters);
    computations.register(
        Computation::new("fail", move |ctx| {
            calls.fail.fetch_add(1, Ordering::SeqCst);
            if ctx.param::<bool>("panic")? {
                panic!("the detector exploded");
            }
            Err(ComputeError::new("the detector is not calibrated"))
        })
        .input("table")
        .param("panic"),
    );

    let options = AppOptions { computations, ..AppOptions::default() };
    Ok((harness_for(derived_spec(), options)?, counters))
}

/// The `derived` status of every dataset `App::datasets` describes.
fn derived_statuses(app: &App) -> Result<Vec<(String, Value)>, serde_json::Error> {
    app.datasets()
        .into_iter()
        .map(|info| Ok((info.id.to_string(), serde_json::to_value(info.derived)?)))
        .collect()
}

fn status(id: &str, derived: Value) -> (String, Value) {
    (id.to_owned(), derived)
}

#[tokio::test]
async fn derived_datasets_are_computed_when_read() -> TestResult {
    let (harness, calls_of) = derived_harness(Duration::ZERO)?;
    assert_eq!(calls(&calls_of.select), 0, "nothing to compute before an import");
    assert_eq!(
        derived_statuses(&harness.app)?,
        [
            status("selected", json!("not_computed")),
            status("count", json!("not_computed")),
            status("broken", json!("not_computed")),
            status("crashing", json!("not_computed")),
        ]
    );
    let (_, table) = harness.get("/api/components/count_table/data").await?;
    assert_eq!(table, json!({ "type": "empty", "reason": "No dataset loaded yet." }));

    harness.upload("measurements.csv", CSV).await?;
    // `selected` has inputs bound to it, so the import computed it; nothing
    // reads `count` yet.
    assert_eq!(calls(&calls_of.select), 1);
    assert_eq!(calls(&calls_of.count), 0);
    assert_eq!(
        derived_statuses(&harness.app)?,
        [
            status("data", Value::Null),
            status("selected", json!("computed")),
            status("count", json!("not_computed")),
            status("broken", json!("not_computed")),
            status("crashing", json!("not_computed")),
        ]
    );

    let (status_code, table) = harness.get("/api/components/count_table/data").await?;
    assert_eq!(status_code, StatusCode::OK, "{table}");
    assert_eq!(table["table"]["columns"], json!(["rows"]));
    assert_eq!(table["table"]["rows"], json!([[40]]));
    assert_eq!(calls(&calls_of.count), 1);
    assert_eq!(calls(&calls_of.select), 1, "the chained input comes from the cache");

    let (_, info) = harness.get("/api/components/selected_info/data").await?;
    assert_eq!(info["info"]["source_name"], "select_column");
    assert_eq!(info["info"]["format"], "computed");
    assert_eq!(info["info"]["derived"], "computed");
    assert_eq!(info["info"]["rows"], 40);
    assert_eq!(info["info"]["columns"][0]["name"], "energy_keV");
    Ok(())
}

#[tokio::test]
async fn cached_tables_are_reused_until_a_dependency_changes() -> TestResult {
    let (harness, calls_of) = derived_harness(Duration::ZERO)?;
    harness.upload("measurements.csv", CSV).await?;
    for id in ["count_table", "selected_table", "chart", "selected_info", "s", "count_table"] {
        let (status_code, data) = harness.get(&format!("/api/components/{id}/data")).await?;
        assert_eq!(status_code, StatusCode::OK, "{id}: {data}");
    }
    assert_eq!((calls(&calls_of.select), calls(&calls_of.count)), (1, 1));

    // The filter of `data` is read by `stats` only.
    let filter = json!({ "column": "detector", "op": "eq", "value": "A" });
    let (_, update) = harness.put("/api/state/filter", filter).await?;
    assert_eq!(ids(&update["invalidated"]), ["stats"]);
    harness.get("/api/components/count_table/data").await?;
    harness.get("/api/components/selected_table/data").await?;
    assert_eq!((calls(&calls_of.select), calls(&calls_of.count)), (1, 1));

    // `x` is a parameter of `selected`, which `count` reads.
    let (status_code, update) = harness.put("/api/state/x", json!("counts")).await?;
    assert_eq!(status_code, StatusCode::OK, "{update}");
    assert_eq!(calls(&calls_of.select), 2, "recomputed during the change");
    assert_eq!(
        ids(&update["invalidated"]),
        ["s", "sfilter", "selected_info", "selected_table", "chart", "count_table"]
    );
    harness.get("/api/components/count_table/data").await?;
    harness.get("/api/components/count_table/data").await?;
    assert_eq!((calls(&calls_of.select), calls(&calls_of.count)), (2, 2));
    Ok(())
}

#[tokio::test]
async fn bound_inputs_are_reconciled_when_their_derived_dataset_changes() -> TestResult {
    let (harness, _) = derived_harness(Duration::ZERO)?;
    let (_, update) = harness.upload("measurements.csv", CSV).await?;
    assert_eq!(update["state"]["x"], "energy_keV");
    assert_eq!(update["state"]["s"], "energy_keV", "reconciled with `selected`");
    assert_eq!(update["revision"], 2, "the import, then the reconciliation of `s`");
    let (_, info) = harness.get("/api/app").await?;
    assert_eq!((&info["revision"], &info["state"]), (&update["revision"], &update["state"]));

    let (status_code, error) = harness.put("/api/state/s", json!("counts")).await?;
    assert_eq!(status_code, StatusCode::UNPROCESSABLE_ENTITY, "checked against `selected`");
    assert_eq!(error["code"], "invalid_state_value");
    let narrow = json!({ "column": "energy_keV", "op": "gt", "value": 1000 });
    let (status_code, update) = harness.put("/api/state/sfilter", narrow.clone()).await?;
    assert_eq!(status_code, StatusCode::OK, "{update}");
    let (_, table) = harness.get("/api/components/selected_table/data").await?;
    assert_eq!(table["table"]["unfiltered_rows"], 40);
    let narrowed = table["table"]["total_rows"].as_u64().unwrap_or_default();
    assert!(narrowed > 0 && narrowed < 40, "{table}");

    // The same columns again: the filter still applies and is kept.
    let (_, update) = harness.upload("measurements.csv", CSV).await?;
    assert_eq!(update["state"]["sfilter"], narrow);
    assert_eq!(update["state"]["s"], "energy_keV");

    // Another column: the selection falls back to it, the filter is cleared.
    let (_, update) = harness.put("/api/state/x", json!("counts")).await?;
    assert_eq!(update["state"]["s"], "counts");
    assert_eq!(update["state"]["sfilter"], Value::Null);
    let (_, info) = harness.get("/api/app").await?;
    assert_eq!(info["state"], update["state"]);
    Ok(())
}

#[tokio::test]
async fn unset_parameter_asks_for_a_value_instead_of_failing() -> TestResult {
    let (harness, _) = derived_harness(Duration::ZERO)?;
    harness.upload("measurements.csv", CSV).await?;
    let (status_code, update) = harness.put("/api/state/x", Value::Null).await?;
    assert_eq!(status_code, StatusCode::OK, "{update}");
    assert_eq!(update["state"]["s"], "energy_keV", "nothing to reconcile with");

    let expected = json!({
        "type": "empty",
        "reason": "Choose a value for \u{201c}x column\u{201d} to see this data."
    });
    for id in ["selected_table", "selected_info", "s", "count_table"] {
        let (status_code, data) = harness.get(&format!("/api/components/{id}/data")).await?;
        assert_eq!(status_code, StatusCode::OK, "{id}");
        assert_eq!(data, expected, "{id}");
    }
    Ok(())
}

#[tokio::test]
async fn failing_computation_is_reported_and_nothing_is_cached() -> TestResult {
    let (harness, calls_of) = derived_harness(Duration::ZERO)?;
    harness.upload("measurements.csv", CSV).await?;
    for attempt in 1..=2 {
        let (status_code, error) = harness.get("/api/components/broken_table/data").await?;
        assert_eq!(status_code, StatusCode::UNPROCESSABLE_ENTITY, "{error}");
        assert_eq!(error["code"], "computation_failed");
        assert_eq!(error["message"], "computation `fail` failed: the detector is not calibrated");
        assert_eq!(calls(&calls_of.fail), attempt, "a failure is not cached");
    }
    let (status_code, _) = harness.get("/api/components/count_table/data").await?;
    assert_eq!(status_code, StatusCode::OK, "other datasets still work");
    let broken = derived_statuses(&harness.app)?.into_iter().find(|(id, _)| id == "broken");
    assert_eq!(broken, Some(status("broken", json!("not_computed"))));
    Ok(())
}

#[tokio::test]
async fn panicking_computation_is_reported_and_the_app_keeps_working() -> TestResult {
    let (harness, calls_of) = derived_harness(Duration::ZERO)?;
    harness.upload("measurements.csv", CSV).await?;
    for attempt in 1..=2 {
        let (status_code, error) = harness.get("/api/components/crashing_table/data").await?;
        assert_eq!(status_code, StatusCode::UNPROCESSABLE_ENTITY, "{error}");
        assert_eq!(error["code"], "computation_failed");
        assert_eq!(error["message"], "computation `fail` failed: panicked: the detector exploded");
        assert_eq!(calls(&calls_of.fail), attempt);
    }
    let (status_code, update) = harness.put("/api/state/x", json!("counts")).await?;
    assert_eq!(status_code, StatusCode::OK, "{update}");
    let (status_code, table) = harness.get("/api/components/selected_table/data").await?;
    assert_eq!(status_code, StatusCode::OK, "{table}");
    assert_eq!(table["table"]["columns"], json!(["counts"]));
    Ok(())
}

#[tokio::test]
async fn failure_during_a_change_is_reported_by_the_readers() -> TestResult {
    let (harness, _) = derived_harness(Duration::ZERO)?;
    harness.upload("measurements.csv", CSV).await?;
    let (status_code, update) = harness.put("/api/state/x", json!("dead_time")).await?;
    assert_eq!(status_code, StatusCode::OK, "the state change itself is valid: {update}");
    assert_eq!(update["state"]["x"], "dead_time");

    let (status_code, error) = harness.get("/api/components/selected_table/data").await?;
    assert_eq!(status_code, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        error["message"],
        "computation `select_column` failed: dead time cannot be selected"
    );

    harness.put("/api/state/x", json!("energy_keV")).await?;
    let (status_code, _) = harness.get("/api/components/selected_table/data").await?;
    assert_eq!(status_code, StatusCode::OK);
    Ok(())
}

#[tokio::test]
async fn nothing_can_be_imported_or_loaded_into_a_derived_dataset() -> TestResult {
    let (harness, _) = derived_harness(Duration::ZERO)?;
    harness.upload("samples.xlsx", XLSX).await?;

    let request = request(Method::POST, "/api/datasets/selected/upload?filename=a.csv")
        .body(Body::from(CSV))?;
    let (status_code, error) = harness.json(request).await?;
    assert_eq!(status_code, StatusCode::CONFLICT);
    assert_eq!(error["code"], "derived_dataset");

    let (status_code, error) =
        harness.put("/api/datasets/selected/sheet", json!({ "sheet": "Calibration" })).await?;
    assert_eq!(status_code, StatusCode::CONFLICT);
    assert_eq!(error["code"], "derived_dataset");

    let frame = DataFrame::new(1, vec![Column::new("energy_keV".into(), [1.0])])?;
    let loaded =
        harness.app.load_dataset(&DatasetId::new("selected"), Dataset::from_frame("t", frame)?);
    assert!(matches!(loaded, Err(AppError::DerivedDataset(id)) if id.as_str() == "selected"));

    let (_, info) = harness.get("/api/components/selected_info/data").await?;
    assert_eq!(info["info"]["source_name"], "select_column", "still computed");
    Ok(())
}

#[tokio::test]
async fn concurrent_readers_share_one_computation() -> TestResult {
    let (harness, calls_of) = derived_harness(Duration::from_millis(200))?;
    harness.upload("measurements.csv", CSV).await?;
    let read = || harness.get("/api/components/count_table/data");
    let (first, second, third, fourth) = tokio::join!(read(), read(), read(), read());
    for (status_code, table) in [first?, second?, third?, fourth?] {
        assert_eq!(status_code, StatusCode::OK, "{table}");
        assert_eq!(table["table"]["rows"], json!([[40]]));
    }
    assert_eq!(calls(&calls_of.count), 1);
    Ok(())
}

#[tokio::test]
async fn derived_dataset_of_literals_reconciles_its_inputs_at_start_up() -> TestResult {
    let generated = DatasetId::new("generated");
    let counter = Arc::new(AtomicUsize::new(0));
    let runs = Arc::clone(&counter);
    let mut computations = ComputationRegistry::default();
    computations.register(Computation::new("constant", move |_| {
        runs.fetch_add(1, Ordering::SeqCst);
        let columns =
            vec![Column::new("a".into(), [1.0, 2.0]), Column::new("b".into(), [3.0, 4.0])];
        Ok(DataFrame::new(2, columns)?)
    }));
    let select = ComponentSpec {
        id: ComponentId::new("pick"),
        title: None,
        span: 12,
        kind: ComponentKind::ColumnSelect {
            dataset: generated.clone(),
            state: StateKey::new("picked"),
            label: "Column".to_owned(),
            numeric_only: true,
            default_index: 1,
        },
    };
    let spec = DashboardSpec {
        spec_version: SPEC_VERSION,
        title: "Generated".to_owned(),
        datasets: vec![DatasetSlot {
            id: generated,
            label: "Generated".to_owned(),
            source: DatasetSource::Computed {
                function: "constant".to_owned(),
                inputs: BTreeMap::new(),
                params: BTreeMap::new(),
            },
        }],
        pages: vec![PageSpec {
            id: "main".to_owned(),
            title: "Main".to_owned(),
            rows: vec![RowSpec { items: vec![select] }],
        }],
    };
    let harness = harness_for(spec, AppOptions { computations, ..AppOptions::default() })?;

    let (_, info) = harness.get("/api/app").await?;
    assert_eq!(info["state"]["picked"], "b");
    assert_eq!(info["revision"], 0, "part of the initial state");
    let (_, options) = harness.get("/api/components/pick/data").await?;
    assert_eq!(options["options"], json!(["a", "b"]));
    assert_eq!(calls(&counter), 1);
    Ok(())
}

/// `hist` is declared before `cols` but reads `col`, which the input bound
/// to `cols` writes: a change refreshes `cols` first, so the input bound to
/// `hist` is reconciled with a table computed from the new `col`.
#[tokio::test]
async fn inputs_are_reconciled_through_a_state_another_derived_dataset_reconciles() -> TestResult {
    let data = DatasetId::new("data");
    let cols = DatasetId::new("cols");
    let hist = DatasetId::new("hist");
    let (col, bin) = (StateKey::new("col"), StateKey::new("bin"));
    let mut computations = ComputationRegistry::default();
    computations
        .register(Computation::new("copy", |ctx| Ok(ctx.input("table")?.clone())).input("table"));
    computations.register(
        Computation::new("select_column", |ctx| {
            let column: String = ctx.param("column")?;
            Ok(ctx.input("table")?.select([column.as_str()])?)
        })
        .input("table")
        .param("column"),
    );
    let derived = |id: &DatasetId, function: &str, params| DatasetSlot {
        id: id.clone(),
        label: id.to_string(),
        source: DatasetSource::Computed {
            function: function.to_owned(),
            inputs: BTreeMap::from([("table".to_owned(), DataView::of(&data))]),
            params,
        },
    };
    let select = |id: &str, dataset: &DatasetId, state: &StateKey| ComponentSpec {
        id: ComponentId::new(id),
        title: None,
        span: 4,
        kind: ComponentKind::ColumnSelect {
            dataset: dataset.clone(),
            state: state.clone(),
            label: id.to_owned(),
            numeric_only: true,
            default_index: 0,
        },
    };
    let chart = ComponentSpec {
        id: ComponentId::new("chart"),
        title: None,
        span: 4,
        kind: ComponentKind::Chart {
            view: DataView::of(&hist),
            mark: ChartMark::Scatter,
            x: bin.clone(),
            y: bin.clone(),
            max_points: 10,
        },
    };
    let spec = DashboardSpec {
        spec_version: SPEC_VERSION,
        title: "Chained through state".to_owned(),
        datasets: vec![
            DatasetSlot {
                id: data.clone(),
                label: "Data".to_owned(),
                source: DatasetSource::Imported,
            },
            derived(
                &hist,
                "select_column",
                BTreeMap::from([("column".to_owned(), ParamBinding::State(col.clone()))]),
            ),
            derived(&cols, "copy", BTreeMap::new()),
        ],
        pages: vec![PageSpec {
            id: "main".to_owned(),
            title: "Main".to_owned(),
            rows: vec![RowSpec {
                items: vec![select("col", &cols, &col), select("bin", &hist, &bin), chart],
            }],
        }],
    };
    let harness = harness_for(spec, AppOptions { computations, ..AppOptions::default() })?;

    let (_, update) = harness.upload("measurements.csv", CSV).await?;
    assert_eq!(
        (&update["state"]["col"], &update["state"]["bin"]),
        (&json!("energy_keV"), &json!("energy_keV"))
    );

    // Other columns: `col` falls back to `a`, and `bin` to the column of the
    // `hist` computed from it.
    let (status_code, update) = harness.upload("other.csv", "a,b\n1,2\n3,4\n").await?;
    assert_eq!(status_code, StatusCode::OK, "{update}");
    assert_eq!((&update["state"]["col"], &update["state"]["bin"]), (&json!("a"), &json!("a")));
    let (_, options) = harness.get("/api/components/bin/data").await?;
    assert_eq!(options["options"], json!(["a"]));
    let (status_code, data) = harness.get("/api/components/chart/data").await?;
    assert_eq!((status_code, &data["type"]), (StatusCode::OK, &json!("chart")), "{data}");
    let (_, info) = harness.get("/api/app").await?;
    assert_eq!((&info["revision"], &info["state"]), (&update["revision"], &update["state"]));
    Ok(())
}
