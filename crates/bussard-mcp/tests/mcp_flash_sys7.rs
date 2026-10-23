//! Issue #314: the flash gate on a healthy System 7 device. System 7 exposes
//! no readable application id, so `knx_plan_device` judges the device by its
//! load-state machines and a sample of its code, the verdict `bussard flash`
//! decides on too: the product's code plans and applies a flash without
//! `--force`, code that differs by one octet is refused with the segment
//! named. Against an in-process mock KNXnet/IP gateway on loopback, never a
//! real gateway.
//!
//! The device is the bussard-testkit System 7 fixture
//! ([`bussard_testkit::sys7`]): a `MockDevice` with mask 0705 running a small
//! synthetic application whose parameter bit 1.0 shows com-object 2.

use std::io::Write as _;
use std::path::Path;
use std::time::Duration;

use bussard_mcp::McpConfig;
use bussard_model::IndividualAddress;
use bussard_testkit::sys7::{
    SYS7_APP_XML, SYS7_CODE, SYS7_CODE_BASE, SYS7_PARAM_BASE, system7_device,
};
use bussard_testkit::{MockDevice, MockGateway};
use bussard_transport::ConnectionConfig;
use rmcp::ServiceExt;
use rmcp::model::CallToolRequestParams;
use serde_json::{Value, json};
use sha2::Digest as _;

const CHANNEL: u8 = 0x57;

fn device_ia() -> anyhow::Result<IndividualAddress> {
    Ok("1.1.7".parse()?)
}

/// The fixture device at 1.1.7 with `code` in its code segment.
fn device(code: &[u8]) -> anyhow::Result<MockDevice> {
    Ok(system7_device(device_ia()?, code))
}

/// Packs the synthetic application into a `.knxprod`.
fn knxprod(path: &Path) -> anyhow::Result<()> {
    let mut writer = zip::ZipWriter::new(std::fs::File::create(path)?);
    writer.start_file(
        "M-00FA/M-00FA_A-0007.xml",
        zip::write::SimpleFileOptions::default(),
    )?;
    writer.write_all(SYS7_APP_XML.as_bytes())?;
    writer.finish()?;
    Ok(())
}

/// The model: 1.1.7 runs the app with object 2 on (the change), the archive
/// pinned in `bussard.lock`.
fn write_model(dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir.join("devices"))?;
    std::fs::create_dir_all(dir.join("products"))?;
    let archive = dir.join("products").join("sys7-test.knxprod");
    knxprod(&archive)?;
    let sha: String = sha2::Sha256::digest(std::fs::read(&archive)?)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    std::fs::write(
        dir.join("bussard.lock"),
        format!(
            "version = 3\n\n[[product]]\nsha256 = \"{sha}\"\nfile = \
             \"products/sys7-test.knxprod\"\nfilename = \"sys7-test.knxprod\"\n\
             origin = {{ kind = \"file\", path = \"sys7-test.knxprod\" }}\n\
             applications = [\"M-00FA_A-0007\"]\n\n[[device]]\naddress = \"1.1.7\"\n\
             application = \"M-00FA_A-0007\"\nproduct_sha256 = \"{sha}\"\nmask = \"0705\"\n"
        ),
    )?;
    std::fs::write(
        dir.join("devices").join("1.1.7.toml"),
        "address = \"1.1.7\"\nname = \"Weather station\"\n\n[parameters]\n\
         \"thr@P-0_R-1\" = \"9\"\n\"obj2@P-1_R-2\" = \"1\"\n\n[links]\n1.listen = [\"1/2/0\"]\n",
    )?;
    std::fs::write(
        dir.join("groups.toml"),
        "groups = [\n  { address = \"1/2/0\", name = \"Switch\" },\n]\n",
    )?;
    Ok(())
}

type Client = rmcp::service::RunningService<rmcp::RoleClient, ()>;

/// A programming-tier server on `dir` against `gateway`, answering
/// `knx_apply_device` at once so the job runs under `knx_apply_status`.
async fn serve(
    dir: &Path,
    gateway: &MockGateway,
) -> anyhow::Result<(Client, tokio::task::JoinHandle<()>)> {
    let connection = ConnectionConfig::tunnel(gateway.addr());
    let config = McpConfig {
        dir: dir.to_path_buf(),
        connection: connection.clone(),
        passive: false,
        allow_writes: false,
        no_model_edits: true,
        capture_db: None,
        allow_programming: true,
        allow_remote_gateway: false,
        plan_ttl: Duration::from_secs(600),
        keyring: None,
        allow_home_assistant: false,
    };
    let state = bussard_mcp::build_state(&config)?;
    if let Some(tier) = state.programming.as_ref() {
        tier.set_reply_wait(Duration::ZERO);
    }
    let service = bussard_service::BusService::open(connection, config.write_policy())?;
    if !service.wait_connected(Duration::from_secs(5)).await {
        anyhow::bail!("the mock tunnel did not come up");
    }
    state.bus.wire(service);
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    let server = bussard_mcp::server::BussardMcp::new(state);
    let task = tokio::spawn(async move {
        if let Ok(running) = server.serve(server_io).await {
            let _ = running.waiting().await;
        }
    });
    Ok((().serve(client_io).await?, task))
}

/// Calls one tool and returns its structured result.
async fn call(client: &Client, tool: &str, args: Value) -> anyhow::Result<Value> {
    let Value::Object(map) = args else {
        anyhow::bail!("tool arguments must be an object");
    };
    let res = tokio::time::timeout(
        Duration::from_secs(30),
        client.call_tool(CallToolRequestParams::new(tool.to_string()).with_arguments(map)),
    )
    .await??;
    res.structured_content
        .ok_or_else(|| anyhow::anyhow!("{tool} returned no structured content"))
}

/// A gateway serving `device`.
async fn gateway(device: MockDevice) -> anyhow::Result<MockGateway> {
    Ok(MockGateway::builder()
        .channel(CHANNEL)
        .keep_serving()
        .idle_timeout(Duration::from_secs(120))
        .device(device)
        .start()
        .await?)
}

#[tokio::test]
async fn test_knx_plan_device_flashes_a_healthy_system7_device_without_force() -> anyhow::Result<()>
{
    let gateway = gateway(device(&SYS7_CODE)?).await?;
    let dir = tempfile::tempdir()?;
    write_model(dir.path())?;
    let (client, task) = serve(dir.path(), &gateway).await?;

    let plan = call(&client, "knx_plan_device", json!({"address": "1.1.7"})).await?;
    assert_eq!(plan["ok"], true, "plan: {plan}");
    assert_eq!(plan["method"], "flash", "plan: {plan}");
    assert_eq!(plan["system_type"], "System 7", "plan: {plan}");
    let flash = &plan["flash"];
    let identity = flash["identity"].as_str().unwrap_or_default();
    assert!(identity.contains("matches bussard.lock"), "{flash}");
    let resident = flash["resident"].as_str().unwrap_or_default();
    assert!(
        resident.contains("runs this application") && resident.contains("the code matches"),
        "{flash}"
    );
    let text = plan.to_string();
    assert!(!text.contains("unidentified"), "{text}");
    let digest = plan["plan_digest"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("no plan_digest in {plan}"))?
        .to_string();

    let started = call(
        &client,
        "knx_apply_device",
        json!({"address": "1.1.7", "plan_digest": digest}),
    )
    .await?;
    assert_eq!(started["refused"], Value::Null, "apply: {started}");
    let job = started["job"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("no job in {started}"))?
        .to_string();
    let deadline = std::time::Instant::now() + Duration::from_secs(90);
    let done = loop {
        let status = call(&client, "knx_apply_status", json!({"job": job})).await?;
        if status["done"] == true {
            break status;
        }
        if std::time::Instant::now() > deadline {
            anyhow::bail!("job {job} did not finish: {status}");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert_eq!(done["state"], "done", "{done}");
    assert_eq!(done["result"]["ok"], true, "{done}");
    assert_eq!(done["result"]["method"], "flash", "{done}");

    let (code, params) = gateway.with_device(device_ia()?, |dev| {
        (
            dev.read_mem(SYS7_CODE_BASE, 4),
            dev.read_mem(SYS7_PARAM_BASE, 2),
        )
    })?;
    assert_eq!(code, SYS7_CODE);
    assert_eq!(params, [9, 0x80], "threshold 9, object 2 on");

    client.cancel().await?;
    task.abort();
    Ok(())
}

#[tokio::test]
async fn test_knx_plan_device_refuses_a_system7_device_whose_code_differs() -> anyhow::Result<()> {
    let mut code = SYS7_CODE;
    code[2] = 0xEE;
    let gateway = gateway(device(&code)?).await?;
    let dir = tempfile::tempdir()?;
    write_model(dir.path())?;
    let (client, task) = serve(dir.path(), &gateway).await?;

    let plan = call(&client, "knx_plan_device", json!({"address": "1.1.7"})).await?;
    let text = plan.to_string();
    assert_ne!(plan["ok"], true, "plan: {plan}");
    assert!(
        text.contains("M-00FA_A-0007_AS-4") && text.contains("offset 2"),
        "the refusal names the segment and the offset: {text}"
    );
    assert!(text.contains("bussard flash --force 1.1.7"), "{text}");
    assert!(!text.contains("unidentified"), "{text}");
    let writes = gateway.with_device(device_ia()?, |dev| dev.writes)?;
    assert_eq!(writes, 0, "planning must not write");

    client.cancel().await?;
    task.abort();
    Ok(())
}
