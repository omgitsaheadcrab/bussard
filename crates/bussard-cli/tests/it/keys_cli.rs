//! `bussard keys import|export|show` end to end against the committed
//! SYNTHETIC keyring (issue #241). Never a real export.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// The synthetic keyring the secure tests share.
const SYNTHETIC: &str = include_str!("../../../../knx-sim/examples/secure/synthetic.knxkeys");

/// Its made-up password.
const PASSWORD: &str = "synthetic-keyring-pw";

/// A fresh temp directory holding a model directory and the keyring.
fn setup(tag: &str) -> Result<(PathBuf, PathBuf), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!(
        "bussard-keys-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    let model = root.join("knx");
    std::fs::create_dir_all(&model)?;
    let keyring = root.join("synthetic.knxkeys");
    std::fs::write(&keyring, SYNTHETIC)?;
    Ok((model, keyring))
}

/// Runs `bussard --dir <model> keys <args>` with the synthetic password.
fn keys(model: &Path, args: &[&str]) -> std::io::Result<Output> {
    Command::new(env!("CARGO_BIN_EXE_bussard"))
        .arg("--dir")
        .arg(model)
        .arg("keys")
        .args(args)
        .env("BUSSARD_KEYRING_PASSWORD", PASSWORD)
        .env_remove("BUSSARD_KEYRING")
        .output()
}

/// Asserts success and returns stdout.
fn ok(out: &Output) -> Result<String, Box<dyn std::error::Error>> {
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(String::from_utf8(out.stdout.clone())?)
}

/// No output may carry key bytes or passwords of the fixture.
fn assert_no_secrets(text: &str) {
    for leak in ["tunnel-user-pw", "device-auth-pw", "QkJC", "d3d3", "ERER"] {
        assert!(!text.contains(leak), "leaked {leak}: {text}");
    }
}

#[test]
fn test_keys_import_show_export_round_trip() -> TestResult {
    let (model, keyring) = setup("round")?;
    let path = keyring.to_string_lossy().to_string();

    let first = ok(&keys(&model, &["import", &path])?)?;
    assert!(first.contains("1 added (1.1.10)"), "{first}");
    assert!(first.contains("created the key store"), "{first}");
    assert_no_secrets(&first);
    assert!(model.join("bussard.keys").exists());

    // A second import is a no-op and touches nothing.
    let again = keys(&model, &["--json", "import", &path])?;
    let v: serde_json::Value = serde_json::from_str(&ok(&again)?)?;
    assert_eq!(v["changed"], false);
    assert_eq!(v["written"], false);
    assert_eq!(v["report"]["devices_unchanged"], 1);
    assert!(!model.join("bussard.keys.bak").exists());

    let show = ok(&keys(&model, &["--json", "show"])?)?;
    assert_no_secrets(&show);
    let v: serde_json::Value = serde_json::from_str(&show)?;
    assert_eq!(v["project"], "Synthetic");
    assert_eq!(v["group_keys"], serde_json::json!(["1/2/3"]));
    assert_eq!(v["devices"][0]["address"], "1.1.10");
    assert_eq!(v["devices"][0]["has_tool_key"], true);
    assert_eq!(v["interfaces"][0]["user_id"], 2);

    let text = ok(&keys(&model, &["show"])?)?;
    assert!(text.contains("1.1.10: tool key"), "{text}");
    assert_no_secrets(&text);

    // Export, then import the export into a second store (the reader
    // verifies its signature) and compare the summaries.
    let out = model.parent().ok_or("no parent")?.join("export.knxkeys");
    let out_s = out.to_string_lossy().to_string();
    let exported = ok(&keys(&model, &["export", &out_s])?)?;
    assert!(
        exported.contains("1 device(s), 1 group key(s), 1 interface(s)"),
        "{exported}"
    );
    let second = model.parent().ok_or("no parent")?.join("knx2");
    std::fs::create_dir_all(&second)?;
    ok(&keys(&second, &["import", &out_s])?)?;
    let v: serde_json::Value = serde_json::from_str(&ok(&keys(&second, &["--json", "show"])?)?)?;
    assert_eq!(v["devices"][0]["address"], "1.1.10");
    assert_eq!(v["group_key_count"], 1);
    assert_eq!(v["has_backbone_key"], true);
    assert_eq!(v["interfaces"][0]["has_password"], true);

    // Refuses to overwrite without --force.
    assert!(!keys(&model, &["export", &out_s])?.status.success());
    ok(&keys(&model, &["export", "--force", &out_s])?)?;
    std::fs::remove_dir_all(model.parent().ok_or("no parent")?)?;
    Ok(())
}

#[test]
fn test_keys_wrong_password_is_refused() -> TestResult {
    let (model, keyring) = setup("wrong")?;
    let out = Command::new(env!("CARGO_BIN_EXE_bussard"))
        .arg("--dir")
        .arg(&model)
        .args(["keys", "import"])
        .arg(&keyring)
        .env("BUSSARD_KEYRING_PASSWORD", "not-the-password")
        .output()?;
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("wrong keyring password"), "{stderr}");
    assert!(!model.join("bussard.keys").exists());
    std::fs::remove_dir_all(model.parent().ok_or("no parent")?)?;
    Ok(())
}

#[test]
fn test_keys_show_without_a_store_hints_at_import() -> TestResult {
    let (model, _) = setup("empty")?;
    let out = keys(&model, &["show"])?;
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("bussard keys import"), "{stderr}");
    std::fs::remove_dir_all(model.parent().ok_or("no parent")?)?;
    Ok(())
}

/// Runs `bussard --dir <model> <args>` with the synthetic password.
fn bussard(model: &Path, args: &[&str]) -> std::io::Result<Output> {
    Command::new(env!("CARGO_BIN_EXE_bussard"))
        .arg("--dir")
        .arg(model)
        .args(args)
        .env("BUSSARD_KEYRING_PASSWORD", PASSWORD)
        .env_remove("BUSSARD_KEYRING")
        .output()
}

/// Issue #331: `bussard groups secure 1/2/3 off` takes the group key out of
/// the store and clears the group's and the linked object's flags; the
/// export then lacks the key, re-importing that export keeps it out, and
/// `bussard undo` restores the flags and the very key.
#[test]
fn test_groups_secure_off_export_reimport_and_undo() -> TestResult {
    let (model, keyring) = setup("unsecure")?;
    let path = keyring.to_string_lossy().to_string();
    ok(&keys(&model, &["import", &path])?)?;
    std::fs::create_dir_all(model.join("devices"))?;
    std::fs::write(
        model.join("bussard.toml"),
        "[connection]\ntransport = \"tunnel\"\ngateway = \"192.0.2.1:3671\"\n",
    )?;
    std::fs::write(
        model.join("groups.toml"),
        "groups = [\n  { address = \"1/2/3\", name = \"Secured level\", dpt = \"5.001\", \
         secure = true },\n]\n",
    )?;
    std::fs::write(
        model.join("devices").join("1.1.10.toml"),
        "address = \"1.1.10\"\nname = \"Secure actuator\"\n\n[security]\nactivated = true\n\n\
         [links]\n22.listen = [\"1/2/3\"]\n",
    )?;
    // A plain push button (no Data Secure) sends on 1/2/3: E035 (#330).
    std::fs::write(
        model.join("devices").join("1.1.40.toml"),
        "address = \"1.1.40\"\nname = \"Meteodata\"\n\n[links]\n1.send = \"1/2/3\"\n",
    )?;
    std::fs::write(
        model.join("bussard.lock"),
        "version = 3\n\n[[device]]\naddress = \"1.1.10\"\nsecure_capable = true\nobjects = \
         [\n  { number = 22, flags = \"CW\", secure = true },\n]\n",
    )?;
    let codes = |model: &Path| -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let out = bussard(model, &["--json", "validate"])?;
        let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;
        Ok(v["diagnostics"]
            .as_array()
            .ok_or("no diagnostics")?
            .iter()
            .filter_map(|d| d["code"].as_str().map(str::to_string))
            .collect())
    };
    assert!(codes(&model)?.contains(&"E035".to_string()));
    // The key before the edit, as the export carries it.
    let before = model.join("before.knxkeys");
    ok(&keys(&model, &["export", &before.to_string_lossy()])?)?;
    let original = bussard_project::parse_keyring(&std::fs::read_to_string(&before)?, PASSWORD)?;
    let ga: bussard_model::GroupAddress = "1/2/3".parse()?;
    let key = original
        .group_keys
        .get(&ga)
        .cloned()
        .ok_or("no key before")?;

    let out = ok(&bussard(&model, &["groups", "secure", "1/2/3", "off"])?)?;
    assert_no_secrets(&out);
    assert!(
        out.contains("Secured level (1/2/3) now runs plain"),
        "{out}"
    );
    assert!(out.contains("object 22 now runs plain"), "{out}");
    assert!(
        out.contains("no longer holds a group key for 1/2/3"),
        "{out}"
    );
    assert!(out.contains("Home Assistant"), "{out}");
    assert!(
        out.contains("`bussard plan <ia>` and `bussard apply <ia>`"),
        "{out}"
    );
    let groups = std::fs::read_to_string(model.join("groups.toml"))?;
    assert!(!groups.contains("secure = true"), "{groups}");
    let lock = std::fs::read_to_string(model.join("bussard.lock"))?;
    assert!(!lock.contains("secure = true"), "{lock}");
    // The plain sender is fine once the GA runs plain.
    assert!(!codes(&model)?.contains(&"E035".to_string()));

    // The export lacks the key; re-importing it does not bring it back.
    let after = model.join("after.knxkeys");
    ok(&keys(&model, &["export", &after.to_string_lossy()])?)?;
    let exported = bussard_project::parse_keyring(&std::fs::read_to_string(&after)?, PASSWORD)?;
    assert!(!exported.group_keys.contains_key(&ga));
    ok(&keys(&model, &["import", &after.to_string_lossy()])?)?;
    let store = bussard_project::KeyStore::load(&model, PASSWORD)?.ok_or("no store")?;
    assert!(store.group_key(ga).is_none());

    // One undo restores all three signals, the very key included.
    let undo = ok(&bussard(&model, &["undo"])?)?;
    assert_no_secrets(&undo);
    assert!(
        undo.contains("holds the group key of 1/2/3 again"),
        "{undo}"
    );
    let store = bussard_project::KeyStore::load(&model, PASSWORD)?.ok_or("no store")?;
    assert_eq!(store.group_key(ga), Some(&key));
    let groups = std::fs::read_to_string(model.join("groups.toml"))?;
    assert!(groups.contains("secure = true"), "{groups}");
    let lock = std::fs::read_to_string(model.join("bussard.lock"))?;
    assert!(lock.contains("secure = true"), "{lock}");
    let _ = std::fs::remove_dir_all(model.parent().ok_or("no parent")?);
    Ok(())
}
