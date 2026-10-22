//! What a device can do, over MCP (issue #298): `knx_show_device` groups an
//! application without channels of its own by ETS parameter page and marks
//! the inactive ones, and `knx_model_lookup` finds a function on an inactive
//! page through the parameter and choice texts of the product model.
//!
//! The product model is the importer's output for the fabricated System 7
//! application in `crates/bussard-prod/tests/fixtures/blocks_app.xml`.
//! No bus is opened: the connection points at a loopback port nothing
//! listens on, and these tools read files only.

use std::net::SocketAddrV4;
use std::path::Path;
use std::time::Duration;

use bussard_mcp::McpConfig;
use bussard_transport::ConnectionConfig;
use rmcp::ServiceExt;
use rmcp::model::CallToolRequestParams;
use serde_json::{Value, json};

type TestResult = anyhow::Result<()>;

const APP: &str = "M-00FA_A-0298-10-0000";

const PRODUCT: &str = include_str!("../../bussard-model/tests/fixtures/blocks_app.yaml");

const LOCK: &str = r#"version = 3

[[device]]
address = "1.1.30"
product = "WS-298"
application = "M-00FA_A-0298-10-0000"
mask = "0701"
channels = [
  { key = "generic-0", id = "CH-0", number = 0 },
]
objects = [
  { number = 0, key = "brightness", channel = "generic-0", text = "Brightness", function = "Physical value", dpt = "9.004", flags = "CRT", ref = "O-0_R-0" },
]
parameters = [
  { key = "universal-channel-a-enable", channel = "generic-0", ref = "P-1_R-1", param = "P-1" },
  { key = "universal-channel-b-enable", channel = "generic-0", ref = "P-2_R-2", param = "P-2" },
  { key = "channel-function", channel = "generic-0", ref = "P-11_R-11", param = "P-11" },
]
"#;

const DEVICE: &str = r#"address = "1.1.30"
name = "Weather station"
product = "WS-298"

[channel.generic-0]
universal-channel-a-enable = "yes"
channel-function = "Temperature sensor"
brightness.send = "4/1/5"
"#;

fn write_model(dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir.join("devices"))?;
    std::fs::create_dir_all(dir.join(".bussard/models"))?;
    std::fs::write(
        dir.join("groups.toml"),
        "groups = [\n  { address = \"4/1/5\", name = \"Brightness\", dpt = \"9.004\" },\n]\n",
    )?;
    std::fs::write(dir.join("devices/1.1.30.toml"), DEVICE)?;
    std::fs::write(dir.join("bussard.lock"), LOCK)?;
    std::fs::write(dir.join(format!(".bussard/models/{APP}.yaml")), PRODUCT)?;
    Ok(())
}

async fn serve(
    dir: &Path,
) -> anyhow::Result<(
    rmcp::service::RunningService<rmcp::RoleClient, ()>,
    tokio::task::JoinHandle<()>,
)> {
    let config = McpConfig {
        dir: dir.to_path_buf(),
        connection: ConnectionConfig::tunnel(SocketAddrV4::new([127, 0, 0, 1].into(), 9)),
        passive: false,
        allow_writes: false,
        no_model_edits: false,
        capture_db: None,
        allow_programming: false,
        allow_remote_gateway: false,
        plan_ttl: Duration::from_secs(600),
        keyring: None,
        allow_home_assistant: false,
    };
    let state = bussard_mcp::build_state(&config)?;
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    let server = bussard_mcp::server::BussardMcp::new(state);
    let task = tokio::spawn(async move {
        if let Ok(running) = server.serve(server_io).await {
            let _ = running.waiting().await;
        }
    });
    let client = ().serve(client_io).await?;
    Ok((client, task))
}

async fn call(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    name: &'static str,
    args: Value,
) -> anyhow::Result<Value> {
    let mut params = CallToolRequestParams::new(name);
    if let Some(map) = args.as_object() {
        params = params.with_arguments(map.clone());
    }
    client
        .call_tool(params)
        .await?
        .structured_content
        .ok_or_else(|| anyhow::anyhow!("{name} returned no structured content"))
}

#[tokio::test]
async fn test_show_device_lists_pages_and_marks_the_inactive_ones() -> TestResult {
    let dir = tempfile::tempdir()?;
    write_model(dir.path())?;
    let (client, task) = serve(dir.path()).await?;

    let view = call(&client, "knx_show_device", json!({"address": "1.1.30"})).await?;
    let blocks = view["blocks"].as_array().cloned().unwrap_or_default();
    let rows: Vec<(String, bool)> = blocks
        .iter()
        .map(|b| {
            (
                b["handle"].as_str().unwrap_or_default().to_string(),
                b["active"].as_bool().unwrap_or_default(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("general".to_string(), true),
            ("universal-channel-a-function".to_string(), true),
            ("universal-channel-b-function".to_string(), false),
            ("universal-channel-c-function".to_string(), false),
        ],
        "{view}"
    );
    let b = &blocks[2]["inactive"];
    assert_eq!(
        b["enable"][0]["key"], "universal-channel-b-enable",
        "{view}"
    );
    assert_eq!(b["enable"][0]["set_to"], "yes", "{view}");
    assert_eq!(b["would_show"][0]["number"], 2, "{view}");
    assert!(
        b["needs"]
            .as_str()
            .is_some_and(|n| n.contains("bussard flash 1.1.30")),
        "{view}"
    );
    assert!(
        view["summary"]
            .as_str()
            .is_some_and(|s| s.starts_with("2 of 4 pages inactive")),
        "{view}"
    );

    // A page handle selects the page; an inactive one lists what it would
    // show.
    let page = call(
        &client,
        "knx_show_device",
        json!({"address": "1.1.30", "channel": "universal-channel-b-function"}),
    )
    .await?;
    assert_eq!(
        page["scope"]["block"], "universal-channel-b-function",
        "{page}"
    );
    assert_eq!(
        page["scope"]["parameters"][0]["key"], "channel-function",
        "{page}"
    );
    assert_eq!(page["scope"]["objects"][0]["number"], 2, "{page}");

    client.cancel().await?;
    task.abort();
    Ok(())
}

#[tokio::test]
async fn test_model_lookup_finds_a_function_on_an_inactive_page() -> TestResult {
    let dir = tempfile::tempdir()?;
    write_model(dir.path())?;
    let (client, task) = serve(dir.path()).await?;

    let found = call(
        &client,
        "knx_model_lookup",
        json!({"query": "Brightness sensor"}),
    )
    .await?;
    let caps = found["capabilities"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let got: Vec<(String, String)> = caps
        .iter()
        .map(|c| {
            (
                c["block_or_channel"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                c["status"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            (
                "Universal channel B: function".to_string(),
                "could provide".to_string()
            ),
            (
                "Universal channel C: function".to_string(),
                "could provide".to_string()
            ),
            (
                "Universal channel A: function".to_string(),
                "could provide".to_string()
            ),
        ],
        "{found}"
    );
    let b = &caps[0];
    assert_eq!(b["device_ia"], "1.1.30", "{found}");
    assert_eq!(b["parameter"], "Channel function", "{found}");
    assert_eq!(b["choice"], "Brightness sensor 1 .. 100 000 lx", "{found}");
    assert_eq!(b["active"], false, "{found}");
    assert_eq!(b["handle"], "universal-channel-b-function", "{found}");
    assert_eq!(
        b["enable"][0]["key"], "universal-channel-b-enable",
        "{found}"
    );
    assert!(
        dir.path()
            .join(format!(".bussard/lookup/{APP}.json"))
            .is_file(),
        "the lookup caches the product model"
    );

    client.cancel().await?;
    task.abort();
    Ok(())
}
