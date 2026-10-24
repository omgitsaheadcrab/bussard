//! The offline oracle as a gate (issue #347): `scripts/campaign/95-offline-oracle.sh
//! --check` over the owner's rows files, with this build's `bussard`.
//!
//! The captures, the project export and the model are private and never
//! committed, so the rows test is env-gated like the other corpus tests
//! (`docs/testing.md`):
//!
//! ```text
//! BUSSARD_ORACLE_ROWS=captures/campaign/rows/rows-house-16.txt:captures/campaign/rows/rows-secure-s3.txt \
//! BUSSARD_ORACLE_ROOT=/path/to/checkout   # the rows' relative paths resolve here
//! BUSSARD_ORACLE_DIR=/path/to/model-copy  # optional: --dir for every rows file
//! cargo nextest run --profile corpus -p bussard-cli -E 'test(/^offline_oracle::/)'
//! ```
//!
//! Every row whose capture and product exist runs; the script fails the check
//! when a row's verdict is not its expected one (identical unless the row says
//! `expect=...`) or its coverage dropped below the recorded figure. Rows
//! without a capture are skipped. A rows file carries its own `--keyring` and
//! `--dir` as `#@` lines. Nothing touches the bus: the script runs `flash
//! --dry-run` only.
//!
//! The ungated test runs the script's `--check` plumbing on a row whose
//! capture does not exist, so a broken script fails every CI run.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// A self-cleaning temporary directory (no `tempfile` dev-dependency needed).
struct TmpDir(PathBuf);

impl TmpDir {
    fn new(tag: &str) -> std::io::Result<Self> {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "bussard-offline-oracle-{tag}-{}-{n}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path)?;
        Ok(TmpDir(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TmpDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The workspace root (the script resolves `knxtrace` relative to itself).
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Whether `python3` and `bash` run here; the script needs both. The campaign
/// scripts are POSIX shell, so Windows always skips.
fn tools_available() -> bool {
    !cfg!(windows)
        && ["python3", "bash"].iter().all(|tool| {
            Command::new(tool)
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        })
}

/// Runs the oracle script in `cwd` with `args`; returns (success, stdout +
/// stderr). No gateway is ever configured: the script only plans dry runs.
fn oracle(cwd: &Path, args: &[&str]) -> std::io::Result<(bool, String)> {
    let script = workspace_root().join("scripts/campaign/95-offline-oracle.sh");
    let out = Command::new("bash")
        .arg(script)
        .args(args)
        .current_dir(cwd)
        .env("BUSSARD_BIN", env!("CARGO_BIN_EXE_bussard"))
        .env_remove("BUSSARD_ALLOW_REAL_GATEWAY")
        .env_remove("BUSSARD_GATEWAY")
        .stdin(Stdio::null())
        .output()?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok((out.status.success(), text))
}

/// The last `n` lines of `text`, for a failure message.
fn tail(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

#[test]
fn test_offline_oracle_check_skips_a_row_without_its_capture() -> TestResult {
    if !tools_available() {
        eprintln!("SKIP: python3 or bash is not available");
        return Ok(());
    }
    let tmp = TmpDir::new("skip")?;
    let model = tmp.path().join("model");
    std::fs::create_dir_all(&model)?;
    let out = tmp.path().join("out");
    let model_arg = model.to_string_lossy().into_owned();
    let out_arg = out.to_string_lossy().into_owned();
    let (ok, text) = oracle(
        tmp.path(),
        &[
            "--row",
            "9.9.9|no-such-capture.pcapng|no-such.knxprod||a note|80|expect=identical",
            "--dir",
            &model_arg,
            "--out",
            &out_arg,
            "--check",
        ],
    )?;
    assert!(
        ok,
        "the check failed on a skipped row:\n{}",
        tail(&text, 20)
    );
    assert!(text.contains("check passed"), "{}", tail(&text, 20));
    let row = std::fs::read_to_string(out.join("9.9.9/row.json"))?;
    let row: serde_json::Value = serde_json::from_str(&row)?;
    assert_eq!(row["skipped"], "capture not found");
    assert_eq!(row["recorded_coverage"], 80.0);
    Ok(())
}

#[test]
fn test_offline_oracle_rows_pass_the_check() -> TestResult {
    let Some(rows) = std::env::var_os("BUSSARD_ORACLE_ROWS") else {
        eprintln!("SKIP: BUSSARD_ORACLE_ROWS is not set (docs/testing.md)");
        return Ok(());
    };
    if !tools_available() {
        eprintln!("SKIP: python3 or bash is not available");
        return Ok(());
    }
    let root = std::env::var_os("BUSSARD_ORACLE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(workspace_root);
    let model = std::env::var("BUSSARD_ORACLE_DIR").ok();
    let tmp = TmpDir::new("rows")?;
    let mut failures = Vec::new();
    for (n, file) in std::env::split_paths(&rows).enumerate() {
        let path = if file.is_absolute() {
            file.clone()
        } else {
            root.join(&file)
        };
        if !path.is_file() {
            failures.push(format!("{}: no such rows file", path.display()));
            continue;
        }
        let out = tmp.path().join(n.to_string());
        let rows_arg = path.to_string_lossy().into_owned();
        let out_arg = out.to_string_lossy().into_owned();
        let mut args = vec!["--rows", &rows_arg, "--out", &out_arg, "--check"];
        if let Some(dir) = model.as_deref() {
            args.extend(["--dir", dir]);
        }
        let (ok, text) = oracle(&root, &args)?;
        if !ok {
            failures.push(format!("{}:\n{}", path.display(), tail(&text, 40)));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
    Ok(())
}
