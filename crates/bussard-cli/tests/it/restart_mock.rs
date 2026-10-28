//! `bussard restart <ADDRESS>` against a mock device: one `A_Restart`, then a
//! reconnect that reads the same mask back; refused without consent. The
//! model pins no product data, so the form is basic and the report says the
//! restart was sent, not that it was verified (issue #452). The confirmed form
//! is tested in `bussard_service::restart`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use bussard_testkit::{MockDevice, MockGateway};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const CHANNEL: u8 = 0x24;

/// A scratch model directory with one device file, removed on drop.
struct TempModel(PathBuf);

impl TempModel {
    fn new(name: &str) -> std::io::Result<Self> {
        let root = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("knx");
        std::fs::create_dir_all(dir.join("devices"))?;
        std::fs::write(
            dir.join("devices").join("1.1.39.toml"),
            "address = \"1.1.39\"\nname = \"Kids room thermostat\"\n",
        )?;
        Ok(TempModel(root))
    }

    fn dir(&self) -> PathBuf {
        self.0.join("knx")
    }
}

impl Drop for TempModel {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Runs `bussard <args> --dir <dir> --gateway <mock>` against a System 7
/// device at 1.1.39 and returns the output and the restarts it saw.
fn run_against(dir: &Path, args: &[&str]) -> TestResult<(Output, usize)> {
    let rt = tokio::runtime::Runtime::new()?;
    let gw = rt.block_on(
        MockGateway::builder()
            .channel(CHANNEL)
            .idle_timeout(Duration::from_secs(30))
            .device(MockDevice::new("1.1.39".parse()?).with_mask(0x0705))
            .start(),
    )?;
    let gateway = format!("127.0.0.1:{}", gw.port());
    let dir_s = dir.to_str().ok_or("non-UTF-8 temp dir")?;
    let mut all = args.to_vec();
    all.extend_from_slice(&["--dir", dir_s, "--gateway", &gateway]);
    let output = Command::new(env!("CARGO_BIN_EXE_bussard"))
        .env("BUSSARD_MGMT_TIMEOUT_MS", "300")
        .env("BUSSARD_ADDRESS_PROBE_MS", "200")
        .env_remove("BUSSARD_KEYRING")
        .args(&all)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;
    let restarts = gw
        .devices()?
        .first()
        .map(|d| d.restarts)
        .unwrap_or_default();
    drop(gw);
    Ok((output, restarts))
}

#[test]
fn test_restart_sends_one_restart_and_waits_for_the_device() -> TestResult {
    let model = TempModel::new("bussard-restart-ok")?;
    let (output, restarts) = run_against(&model.dir(), &["restart", "1.1.39", "--yes"])?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "stdout: {stdout}\nstderr: {stderr}"
    );
    assert_eq!(restarts, 1, "exactly one A_Restart");
    assert!(
        stdout.contains("1.1.39: restart sent (a basic A_Restart")
            && stdout.contains("it answered again after")
            && stdout.contains("(mask 0705)")
            && stdout.contains("The restart itself is not verified."),
        "{stdout}"
    );
    assert!(!stdout.contains("restarted"), "{stdout}");
    assert!(stderr.contains("gateway: 127.0.0.1:"), "{stderr}");
    assert!(
        stderr.contains("restart form: basic (bussard.lock pins no product data for 1.1.39)"),
        "{stderr}"
    );
    Ok(())
}

#[test]
fn test_restart_json_reports_the_mask_and_the_wait() -> TestResult {
    let model = TempModel::new("bussard-restart-json")?;
    let (output, restarts) = run_against(&model.dir(), &["restart", "1.1.39", "--yes", "--json"])?;
    assert!(output.status.success(), "{output:?}");
    assert_eq!(restarts, 1);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(report["address"], "1.1.39");
    assert_eq!(report["mask"], "0705");
    assert_eq!(report["form"], "basic");
    assert_eq!(report["confirmed"], false);
    assert!(report["process_time_s"].is_null(), "{report}");
    assert!(report["form_reason"].is_string(), "{report}");
    assert!(report["answered_after_ms"].as_u64().is_some(), "{report}");
    Ok(())
}

#[test]
fn test_restart_without_consent_sends_nothing() -> TestResult {
    let model = TempModel::new("bussard-restart-refused")?;
    let (output, restarts) = run_against(&model.dir(), &["restart", "1.1.39"])?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert_eq!(restarts, 0, "no A_Restart without --yes or a terminal");
    assert!(
        stderr.contains("refusing to restart 1.1.39 via 127.0.0.1:")
            && stderr.contains("pass --yes"),
        "{stderr}"
    );
    Ok(())
}
