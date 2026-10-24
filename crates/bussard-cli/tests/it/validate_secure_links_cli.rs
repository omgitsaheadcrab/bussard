//! `bussard validate` on a hand-edited model that links a device without KNX
//! Data Secure as sender on a group address a Secure receiver takes secured
//! (E035, issue #330). No keyring, no bus, no network.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

/// The model: secured GA 1/0/4, Secure actuator 1.1.5 listening on it, and
/// the weather station 1.1.202 (no Data Secure) sending on it.
fn model(tag: &str) -> TestResult<PathBuf> {
    let dir = std::env::temp_dir().join(format!(
        "bussard-validate-secure-links-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("devices"))?;
    std::fs::write(
        dir.join("groups.toml"),
        "groups = [\n  { address = \"1/0/4\", name = \"Licht\", dpt = \"1.001\", secure = true },\n]\n",
    )?;
    std::fs::write(
        dir.join("devices").join("1.1.5.toml"),
        "address = \"1.1.5\"\nname = \"Actuator\"\n\n[links]\nschalten.listen = [\"1/0/4\"]\n",
    )?;
    std::fs::write(
        dir.join("devices").join("1.1.202.toml"),
        "address = \"1.1.202\"\nname = \"Weather station\"\n\n[links]\nc18.send = \"1/0/4\"\n",
    )?;
    std::fs::write(
        dir.join("bussard.lock"),
        "version = 3\n\n[[device]]\naddress = \"1.1.5\"\nsecure_capable = true\n\
         objects = [\n  { number = 3, key = \"schalten\", dpt = \"1.001\", flags = \"CWU\", secure = true },\n]\n\n\
         [[device]]\naddress = \"1.1.202\"\n\
         objects = [\n  { number = 18, key = \"c18\", dpt = \"1.001\", flags = \"CRT\" },\n]\n",
    )?;
    Ok(dir)
}

fn validate_json(dir: &Path) -> TestResult<(bool, serde_json::Value)> {
    let out = Command::new(env!("CARGO_BIN_EXE_bussard"))
        .args(["validate", "--json", "--dir"])
        .arg(dir)
        .env_remove("BUSSARD_KEYRING")
        .env_remove("BUSSARD_KEYRING_PASSWORD")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;
    Ok((out.status.success(), serde_json::from_slice(&out.stdout)?))
}

#[test]
fn test_validate_lists_e035_for_plain_sender_on_secured_ga() -> TestResult {
    let dir = model("e035")?;
    let (success, doc) = validate_json(&dir)?;
    assert!(!success, "an E035 model must fail validation: {doc}");
    let e035: Vec<&serde_json::Value> = doc["diagnostics"]
        .as_array()
        .ok_or("validate --json prints a diagnostics array")?
        .iter()
        .filter(|d| d["code"] == "E035")
        .collect();
    assert_eq!(e035.len(), 1, "{doc}");
    assert_eq!(e035[0]["severity"], "error", "{doc}");
    assert_eq!(e035[0]["location"], "links.\"1.1.202\"[0]", "{doc}");
    let message = e035[0]["message"].as_str().unwrap_or_default();
    assert!(message.contains("1.1.5 object 3"), "{message}");
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}
