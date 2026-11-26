//! A dashboard built with the public API, served on a real socket.

use std::path::PathBuf;

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
    let x = StateKey::new("x");
    let y = StateKey::new("y");
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
