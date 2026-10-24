//! Mutation tests of the Data Secure invariants (issue #348): the fixture
//! `tests/fixtures/secure-invariants` validates clean with the synthetic
//! keyring as its key store; each test breaks one invariant the way a user
//! would and asserts the code that names it. No bus, no network, no key is
//! printed.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

/// The synthetic keyring's made-up password.
const PASSWORD: &str = "synthetic-keyring-pw";

/// The committed SYNTHETIC keyring: the tool key of 1.1.10, the group key of
/// 1/2/3, the tunnelling address 1.1.200 assigned 1/2/3.
fn keyring_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../knx-sim/examples/secure/synthetic.knxkeys")
}

/// A copy of the fixture with the synthetic keyring imported as its key store.
fn fixture(tag: &str) -> TestResult<PathBuf> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/secure-invariants");
    let dir = std::env::temp_dir().join(format!(
        "bussard-invariants-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("devices"))?;
    for file in ["bussard.toml", "groups.toml", "bussard.lock"] {
        std::fs::copy(src.join(file), dir.join(file))?;
    }
    for entry in std::fs::read_dir(src.join("devices"))? {
        let entry = entry?;
        std::fs::copy(entry.path(), dir.join("devices").join(entry.file_name()))?;
    }
    let out = bussard(&dir, &["keys", "import"], &[keyring_path().as_path()])?;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(dir)
}

/// Runs `bussard <args> <paths> --dir <dir>` with the keyring password.
fn bussard(dir: &Path, args: &[&str], paths: &[&Path]) -> TestResult<Output> {
    Ok(Command::new(env!("CARGO_BIN_EXE_bussard"))
        .args(args)
        .args(paths)
        .arg("--dir")
        .arg(dir)
        .env_remove("BUSSARD_KEYRING")
        .env_remove("BUSSARD_DIR")
        .env("BUSSARD_KEYRING_PASSWORD", PASSWORD)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?)
}

/// `(code, location, message)` of every error and warning of `validate`.
fn problems(dir: &Path) -> TestResult<Vec<(String, String, String)>> {
    let out = bussard(dir, &["validate", "--json"], &[])?;
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    Ok(doc["diagnostics"]
        .as_array()
        .ok_or("validate --json prints a diagnostics array")?
        .iter()
        .filter(|d| d["severity"] != "info")
        .map(|d| {
            (
                d["code"].as_str().unwrap_or_default().to_string(),
                d["location"].as_str().unwrap_or_default().to_string(),
                d["message"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect())
}

/// Replaces `from` by `to` in the model file `file`, which must contain it.
fn edit(dir: &Path, file: &str, from: &str, to: &str) -> TestResult {
    let path = dir.join(file);
    let text = std::fs::read_to_string(&path)?;
    assert!(text.contains(from), "{file} does not contain {from:?}");
    std::fs::write(&path, text.replacen(from, to, 1))?;
    Ok(())
}

/// The codes of `problems`.
fn codes(problems: &[(String, String, String)]) -> Vec<&str> {
    problems.iter().map(|p| p.0.as_str()).collect()
}

#[test]
fn test_validate_invariants_fixture_is_clean() -> TestResult {
    let dir = fixture("clean")?;
    let found = problems(&dir)?;
    assert!(found.is_empty(), "{found:?}");
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

/// W037: the device on the secured GA is not activated.
#[test]
fn test_validate_invariants_not_activated_is_w037() -> TestResult {
    let dir = fixture("w037")?;
    edit(
        &dir,
        "devices/1.1.10.toml",
        "activated = true",
        "activated = false",
    )?;
    let found = problems(&dir)?;
    let w037: Vec<_> = found.iter().filter(|p| p.0 == "W037").collect();
    assert_eq!(w037.len(), 1, "{found:?}");
    assert_eq!(w037[0].1, "devices/1.1.10.toml security");
    assert!(w037[0].2.contains("1/2/3"), "{}", w037[0].2);
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

/// W039: the sending Secure device lost its sequence number in the lock.
#[test]
fn test_validate_invariants_sender_without_sequence_is_w039() -> TestResult {
    let dir = fixture("w039")?;
    edit(&dir, "bussard.lock", "sequence_number = 5\n", "")?;
    let found = problems(&dir)?;
    assert_eq!(codes(&found), vec!["W039"], "{found:?}");
    assert_eq!(found[0].1, "bussard.lock device 1.1.10");
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

/// W041: an object on the secured GA lost its `secure` flag; the message
/// names the command that sets all three signals.
#[test]
fn test_validate_invariants_object_flag_disagrees_is_w041() -> TestResult {
    let dir = fixture("w041")?;
    edit(
        &dir,
        "bussard.lock",
        "flags = \"CWU\", secure = true",
        "flags = \"CWU\"",
    )?;
    let found = problems(&dir)?;
    assert_eq!(codes(&found), vec!["W041"], "{found:?}");
    assert_eq!(found[0].1, "groups.toml 1/2/3");
    assert!(
        found[0].2.contains("1.1.10 object 22")
            && found[0].2.contains("`bussard groups secure 1/2/3 on`"),
        "{}",
        found[0].2
    );
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

/// W029 carries the same fix: the group flag dropped, the key still stored.
#[test]
fn test_validate_invariants_flag_against_key_store_suggests_the_fix() -> TestResult {
    let dir = fixture("w029")?;
    edit(
        &dir,
        "groups.toml",
        "dpt = \"1.001\", secure = true",
        "dpt = \"1.001\"",
    )?;
    let found = problems(&dir)?;
    let w029: Vec<_> = found.iter().filter(|p| p.0 == "W029").collect();
    assert_eq!(w029.len(), 1, "{found:?}");
    assert!(
        w029[0].2.contains("`bussard groups secure 1/2/3 off`"),
        "{}",
        w029[0].2
    );
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

/// W040: a keyring export older than the key store's last change, and Home
/// Assistant configured without a newer export.
#[test]
fn test_validate_invariants_stale_export_is_w040() -> TestResult {
    let dir = fixture("w040")?;
    let export = dir.join("ha.knxkeys");
    let out = bussard(&dir, &["keys", "export"], &[export.as_path()])?;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(problems(&dir)?.is_empty());

    // The user switches 1/2/3 plain: the key store changes after the export.
    // The meta file records seconds, so wait for the next one.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let out = bussard(&dir, &["groups", "secure", "1/2/3", "off"], &[])?;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    std::fs::write(
        dir.join("bussard.toml"),
        "[connection]\ntransport = \"routing\"\n\n[home_assistant]\nurl = \
         \"http://ha.local:8123\"\n",
    )?;
    let found = problems(&dir)?;
    let w040: Vec<&str> = found
        .iter()
        .filter(|p| p.0 == "W040")
        .map(|p| p.1.as_str())
        .collect();
    assert_eq!(
        w040,
        vec!["bussard.toml [home_assistant]", "ha.knxkeys"],
        "{found:?}"
    );

    // A fresh export clears both.
    let out = bussard(&dir, &["keys", "export", "--force"], &[export.as_path()])?;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let found = problems(&dir)?;
    assert!(!found.iter().any(|p| p.0 == "W040"), "{found:?}");
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

/// W042: an older keyring imported after the user switched 1/2/3 plain puts
/// its key back; `keys import` names the device the next apply secures.
#[test]
fn test_keys_import_of_a_removed_group_key_is_w042() -> TestResult {
    let dir = fixture("w042")?;
    let out = bussard(&dir, &["groups", "secure", "1/2/3", "off"], &[])?;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = bussard(&dir, &["keys", "import"], &[keyring_path().as_path()])?;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("warning[W042]") && stderr.contains("1/2/3") && stderr.contains("1.1.10"),
        "{stderr}"
    );
    // The JSON form carries it too.
    let out = bussard(&dir, &["groups", "secure", "1/2/3", "off"], &[])?;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = bussard(
        &dir,
        &["keys", "import", "--json"],
        &[keyring_path().as_path()],
    )?;
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    assert_eq!(doc["warnings"][0]["code"], "W042", "{doc}");
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}
