//! Documentation consistency checks (issue #107).
//!
//! - `docs/explanation/device-support.md`'s "Supported device masks" table is rendered from
//!   [`bussard_mgmt::capability_table`], the same source `plan`, `apply`,
//!   `flash`, `reconstruct` and `audit` use, and must match byte-for-byte.
//! - the pages under `docs/reference/commands/` document every long flag any subcommand's `--help`
//!   prints.

use std::path::{Path, PathBuf};
use std::process::Command;

use bussard_mgmt::{MaskProfile, capability_table};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Marks the start of the generated table in `docs/explanation/device-support.md`.
const TABLE_START: &str = "<!-- mask-table:start -->";
/// Marks the end of the generated table in `docs/explanation/device-support.md`.
const TABLE_END: &str = "<!-- mask-table:end -->";

fn docs() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs")
}

fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

/// Renders the capability table as the Markdown that lives in device-support.md.
fn render_mask_table() -> String {
    let mut out = String::from(
        "| Mask | Family | Medium | `plan` / `apply` | `flash` | `reconstruct` | `describe` | Notes |\n\
         |---|---|---|---|---|---|---|---|\n",
    );
    for (mask, caps) in capability_table() {
        let profile = MaskProfile::from_mask(mask);
        out.push_str(&format!(
            "| `{mask:04X}` | {} | {} | {} | {} | {} | {} | {} |\n",
            profile.family().label(),
            profile.medium().label(),
            yes_no(caps.plan_apply),
            yes_no(caps.flash),
            yes_no(caps.reconstruct),
            yes_no(caps.describe),
            caps.note
        ));
    }
    out
}

#[test]
fn test_safety_mask_table_matches_capability_table() -> TestResult {
    let safety = std::fs::read_to_string(docs().join("explanation/device-support.md"))?;
    let section = safety
        .find("## Supported device masks")
        .ok_or("device-support.md has no \"Supported device masks\" section")?;
    let rest = &safety[section..];
    let start = rest.find(TABLE_START).ok_or("no mask-table start marker")?;
    let end = rest.find(TABLE_END).ok_or("no mask-table end marker")?;
    // The marker line may carry a trailing comment; the table starts on the next line.
    let after_start = &rest[start..end];
    let body = after_start
        .split_once('\n')
        .map(|(_, b)| b)
        .ok_or("empty table block")?;
    let expected = render_mask_table();
    assert_eq!(
        body, expected,
        "docs/explanation/device-support.md's mask table is stale; replace the block between the markers with:\n{expected}"
    );
    Ok(())
}

/// Runs `bussard <args> --help` and returns stdout.
fn help(args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let out = Command::new(env!("CARGO_BIN_EXE_bussard"))
        .args(args)
        .arg("--help")
        .output()?;
    if !out.status.success() {
        return Err(format!("`bussard {} --help` failed", args.join(" ")).into());
    }
    Ok(String::from_utf8(out.stdout)?)
}

/// The subcommand names listed under `Commands:` in the top-level help.
fn subcommands(top: &str) -> Vec<String> {
    top.lines()
        .skip_while(|l| !l.starts_with("Commands:"))
        .skip(1)
        .take_while(|l| l.starts_with("  "))
        .filter_map(|l| l.split_whitespace().next())
        .filter(|name| *name != "help")
        .map(str::to_string)
        .collect()
}

/// Every `--long-flag` token in a help text.
fn long_flags(text: &str) -> Vec<String> {
    let mut flags = Vec::new();
    for (i, _) in text.match_indices("--") {
        let tail = &text[i + 2..];
        let name: String = tail
            .chars()
            .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
            .collect();
        if name.is_empty() || name.starts_with('-') {
            continue;
        }
        // Only flags defined in the option list (start of a help line).
        let line_start = text[..i].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let prefix = text[line_start..i].trim();
        if !prefix.is_empty() && !prefix.ends_with(',') {
            continue;
        }
        flags.push(format!("--{name}"));
    }
    flags.sort();
    flags.dedup();
    flags
}

/// Whether `doc` mentions `flag` as a whole token.
fn mentions(doc: &str, flag: &str) -> bool {
    doc.match_indices(flag).any(|(i, _)| {
        let next = doc[i + flag.len()..].chars().next();
        !matches!(next, Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    })
}

/// The text of a command's `## \`bussard <command>...` (or `###`) section in
/// the concatenated reference pages, up to the next heading of the same or a
/// higher level.
fn command_section<'a>(doc: &'a str, command: &str) -> Option<&'a str> {
    for level in ["## ", "### "] {
        let heading = format!("{level}`bussard {command}");
        let found = doc.match_indices(&heading).find_map(|(i, _)| {
            // Only a heading at the start of a line, and the whole command name
            // (`import` must not match `import-product`).
            let at_line_start = i == 0 || doc.as_bytes()[i - 1] == b'\n';
            let next = doc[i + heading.len()..].chars().next();
            (at_line_start && matches!(next, Some(' ' | '`'))).then_some(i)
        });
        let Some(start) = found else { continue };
        let body = &doc[start + heading.len()..];
        let end = if level == "## " {
            body.find("\n## ")
        } else {
            body.find("\n### ").or_else(|| body.find("\n## "))
        }
        .unwrap_or(body.len());
        return Some(&body[..end]);
    }
    None
}

/// Every page under `docs/reference/commands/` plus `global-options.md`
/// (where the global flags are documented once), concatenated.
fn reference_pages() -> Result<String, Box<dyn std::error::Error>> {
    let dir = docs().join("reference/commands");
    let mut names: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    names.sort();
    names.push(docs().join("reference/global-options.md"));
    let mut out = String::new();
    for path in names {
        out.push_str(&std::fs::read_to_string(path)?);
        out.push('\n');
    }
    Ok(out)
}

#[test]
fn test_reference_documents_every_help_flag() -> TestResult {
    let reference = reference_pages()?;
    let top = help(&[])?;
    let commands = subcommands(&top);
    assert!(
        commands.len() > 10,
        "parsed too few subcommands: {commands:?}"
    );

    // Global flags appear in every subcommand's help; they are documented once.
    let globals = long_flags(&top);

    let mut missing = Vec::new();
    for command in &commands {
        let Some(section) = command_section(&reference, command) else {
            missing.push(format!("section for `bussard {command}`"));
            continue;
        };
        for flag in long_flags(&help(&[command])?) {
            if flag == "--help" || flag == "--version" {
                continue;
            }
            let documented = if globals.contains(&flag) {
                mentions(&reference, &flag)
            } else {
                mentions(section, &flag)
            };
            if !documented {
                missing.push(format!("`bussard {command} {flag}`"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "docs/reference/commands/ does not document:\n  {}",
        missing.join("\n  ")
    );
    Ok(())
}

#[test]
fn test_long_flags_parses_option_lines_only() {
    let text =
        "Options:\n      --dir <DIR>  The dir (see --other in prose)\n  -v, --verbose  Louder\n";
    assert_eq!(long_flags(text), vec!["--dir", "--verbose"]);
}
