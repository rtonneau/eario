//! A dashboard built with the public API, served on a real socket.

use std::path::PathBuf;

use eario::polars::prelude::{DataType, IntoLazy, col, lit};
use eario::prelude::*;
use eario::runtime::{RunningServer, start};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn fixture(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures").join(relative)
}

fn explorer() -> Dashboard {
    let data = DatasetId::new("data");
    let x = State::new("x");
    let y = State::new("y");
    Dashboard::new("End to end")
        .open_browser(false)
        .remember_preferences(false)
        .page("Explore", |page| {
            page.row([file_import(&data), dataset_info(&data).id("info")])
                .row([
                    numeric_column_select("X", &data, &x, 0),
                    numeric_column_select("Y", &data, &y, 1),
                ])
                .row([scatter_chart(&data, &x, &y).id("chart")])
        })
        .load_file(&data, fixture("excel/samples.xlsx"))
}

/// A chart of the imported data beside a chart of a derived dataset: the
/// filtered imported rows with the column on the vertical axis scaled by a
/// constant.
fn scaled_explorer() -> Dashboard {
    let data = DatasetId::new("data");
    let scaled = DatasetId::new("scaled");
    let x = State::<String>::new("x");
    let y = State::<String>::new("y");
    let filter = State::<Filter>::new("filter");
    let column = y.clone();
    let scale = Computation::new("scale", move |ctx| {
        // Read through the handle of the state the parameter is bound to.
        let name: String = ctx.param(&column)?;
        let factor: f64 = ctx.param("factor")?;
        let rows = ctx.input("table")?.clone().lazy();
        Ok(rows.with_column(col(name.as_str()).cast(DataType::Float64) * lit(factor)).collect()?)
    })
    .input("table")
    .param("column")
    .param("factor")
    .describe("The table with one column multiplied by a factor.");

    Dashboard::new("Derived end to end")
        .open_browser(false)
        .remember_preferences(false)
        .computation(scale)
        .derived(&scaled, "scale", |d| {
            d.input("table", filtered(&data, &filter)).param("column", &y).literal("factor", 2.0)
        })
        .page("Compare", |page| {
            page.row([
                numeric_column_select("X", &data, &x, 0),
                numeric_column_select("Y", &data, &y, 1),
                filter_input(&data, &filter),
            ])
            .row([
                scatter_chart(&data, &x, &y).id("raw"),
                scatter_chart(&scaled, &x, &y).id("scaled"),
            ])
        })
        .load_file(&data, fixture("excel/samples.xlsx"))
}

/// Sends one HTTP/1.1 request and returns the status and the body.
async fn http(
    server: &RunningServer,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> Result<(u16, String), Box<dyn std::error::Error>> {
    let base_url = server.base_url();
    let address = base_url.trim_start_matches("http://");
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str("\r\n");
    request.push_str(body);

    let mut stream = TcpStream::connect(address).await?;
    stream.write_all(request.as_bytes()).await?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    let response = String::from_utf8(response)?;

    let (head, body) = response.split_once("\r\n\r\n").ok_or("response without a header end")?;
    let status = head.split_whitespace().nth(1).ok_or("response without a status")?.parse()?;
    Ok((status, body.to_owned()))
}

#[tokio::test]
async fn dashboard_is_served_with_its_startup_file_and_stops_on_request() -> TestResult {
    let (app, config) = explorer().build()?;
    let server = start(app, config).await?;
    let token = server.token().to_owned();
    let authorized = [("x-eario-token", token.as_str())];

    let (status, _) = http(&server, "GET", "/api/app", &[], "").await?;
    assert_eq!(status, 401, "the API must not answer without the session token");

    let (status, body) = http(&server, "GET", "/api/app", &authorized, "").await?;
    assert_eq!(status, 200, "{body}");
    let info: Value = serde_json::from_str(&body)?;
    assert_eq!(info["spec"]["title"], "End to end");
    assert_eq!(info["state"]["x"], "energy_keV");
    assert_eq!(info["state"]["y"], "counts");

    let (status, body) = http(&server, "GET", "/api/components/info/data", &authorized, "").await?;
    assert_eq!(status, 200, "{body}");
    let data: Value = serde_json::from_str(&body)?;
    assert_eq!(data["info"]["source_name"], "samples.xlsx");
    assert_eq!(data["info"]["selected_sheet"], "Measurements");

    let (status, _) = http(&server, "GET", "/", &[], "").await?;
    assert_eq!(status, 200, "the frontend is served without a token");

    let (status, _) = http(&server, "POST", "/api/shutdown", &authorized, "").await?;
    assert_eq!(status, 204);
    server.wait().await?;
    Ok(())
}

#[tokio::test]
async fn server_stops_when_asked_from_rust() -> TestResult {
    let (app, config) = explorer().build()?;
    let server = start(app, config).await?;
    server.shutdown();
    server.wait().await?;
    Ok(())
}

/// The `[x, y]` points of a chart component, as served.
async fn chart_points(
    server: &RunningServer,
    id: &str,
) -> Result<(Value, Vec<(f64, f64)>), Box<dyn std::error::Error>> {
    let authorized = [("x-eario-token", server.token())];
    let path = format!("/api/components/{id}/data");
    let (status, body) = http(server, "GET", &path, &authorized, "").await?;
    assert_eq!(status, 200, "{body}");
    let data: Value = serde_json::from_str(&body)?;
    assert_eq!(data["type"], "chart", "{body}");
    let points = data["chart"]["points"]
        .as_array()
        .ok_or("chart without points")?
        .iter()
        .map(|point| Some((point[0].as_f64()?, point[1].as_f64()?)))
        .collect::<Option<Vec<_>>>()
        .ok_or("point that is not a pair of numbers")?;
    Ok((data, points))
}

#[tokio::test]
async fn chart_reads_a_derived_dataset_through_the_server() -> TestResult {
    let (app, config) = scaled_explorer().build()?;
    let server = start(app, config).await?;
    let token = server.token().to_owned();

    let (status, body) =
        http(&server, "GET", "/api/app", &[("x-eario-token", token.as_str())], "").await?;
    assert_eq!(status, 200, "{body}");
    let info: Value = serde_json::from_str(&body)?;
    assert_eq!(info["spec"]["datasets"][1]["id"], "scaled");
    assert_eq!(info["spec"]["datasets"][1]["source"]["type"], "computed");

    let (_, raw) = chart_points(&server, "raw").await?;
    let (scaled_data, scaled) = chart_points(&server, "scaled").await?;
    assert_eq!(scaled_data["chart"]["y_label"], "counts");
    assert_eq!(scaled.len(), 38, "the rows without counts are skipped in both");
    assert_eq!(scaled.len(), raw.len());
    for ((raw_x, raw_y), (x, y)) in raw.iter().zip(&scaled) {
        assert_eq!((*x, *y), (*raw_x, raw_y * 2.0));
    }

    // Choosing another column recomputes the derived dataset with it.
    let headers = [("x-eario-token", token.as_str()), ("Content-Type", "application/json")];
    let (status, body) = http(&server, "PUT", "/api/state/y", &headers, r#""dead_time""#).await?;
    assert_eq!(status, 200, "{body}");
    let (_, raw) = chart_points(&server, "raw").await?;
    let (scaled_data, scaled) = chart_points(&server, "scaled").await?;
    assert_eq!(scaled_data["chart"]["y_label"], "dead_time");
    assert_eq!(scaled.len(), 40);
    for ((raw_x, raw_y), (x, y)) in raw.iter().zip(&scaled) {
        assert_eq!((*x, *y), (*raw_x, raw_y * 2.0));
    }

    // The filter of the input view applies before the computation.
    let filter = r#"{"column":"detector","op":"eq","value":"A"}"#;
    let (status, body) = http(&server, "PUT", "/api/state/filter", &headers, filter).await?;
    assert_eq!(status, 200, "{body}");
    let (_, raw) = chart_points(&server, "raw").await?;
    let (_, scaled) = chart_points(&server, "scaled").await?;
    assert_eq!((raw.len(), scaled.len()), (40, 20), "half the rows are from detector A");

    server.shutdown();
    server.wait().await?;
    Ok(())
}

#[cfg(feature = "mcp")]
mod mcp {
    use super::*;

    const JSON_RPC_HEADERS: [(&str, &str); 2] =
        [("Content-Type", "application/json"), ("Accept", "application/json, text/event-stream")];

    async fn call(
        server: &RunningServer,
        body: &str,
    ) -> Result<(u16, String), Box<dyn std::error::Error>> {
        let bearer = format!("Bearer {}", server.token());
        let mut headers = JSON_RPC_HEADERS.to_vec();
        headers.push(("Authorization", &bearer));
        http(server, "POST", "/mcp", &headers, body).await
    }

    #[tokio::test]
    async fn agent_can_list_tools_and_inspect_the_dataset() -> TestResult {
        let (app, config) = explorer().mcp(true).build()?;
        let server = start(app, config).await?;

        let (status, _) = http(&server, "POST", "/mcp", &JSON_RPC_HEADERS, "{}").await?;
        assert_eq!(status, 401, "the MCP endpoint must not answer without the session token");

        let (status, body) =
            call(&server, r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#).await?;
        assert_eq!(status, 200, "{body}");
        let listed: Value = serde_json::from_str(&body)?;
        let tools = listed["result"]["tools"].as_array().ok_or("no tool list")?;
        let mut names: Vec<&str> = tools.iter().filter_map(|tool| tool["name"].as_str()).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "dataset_statistics",
                "describe_dashboard",
                "get_component_data",
                "list_datasets",
                "preview_dataset"
            ]
        );
        for tool in tools {
            assert_eq!(tool["annotations"]["readOnlyHint"], true, "{}", tool["name"]);
        }

        let request = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":
            {"name":"preview_dataset","arguments":{"dataset_id":"data","limit":5000}}}"#;
        let (status, body) = call(&server, request).await?;
        assert_eq!(status, 200, "{body}");
        let called: Value = serde_json::from_str(&body)?;
        let text = called["result"]["content"][0]["text"].as_str().ok_or("no text content")?;
        let page: Value = serde_json::from_str(text)?;
        assert_eq!(page["total_rows"], 40);
        assert_eq!(page["rows"].as_array().map(Vec::len), Some(40));

        let request = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":
            {"name":"dataset_statistics","arguments":{"dataset_id":"missing"}}}"#;
        let (_, body) = call(&server, request).await?;
        assert!(body.contains("not declared"), "{body}");

        server.shutdown();
        server.wait().await?;
        Ok(())
    }

    #[tokio::test]
    async fn mcp_endpoint_is_absent_unless_enabled() -> TestResult {
        let (app, config) = explorer().build()?;
        let server = start(app, config).await?;
        let (status, _) = call(&server, "{}").await?;
        assert_eq!(status, 405);
        server.shutdown();
        server.wait().await?;
        Ok(())
    }
}
