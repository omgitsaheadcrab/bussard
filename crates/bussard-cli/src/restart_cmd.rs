//! `bussard restart <ADDRESS>`: restart one device and wait until it answers
//! again.
//!
//! The restart and its verification live in [`bussard_service::restart`]. This
//! module resolves the connection, applies the write gate (a non-loopback
//! gateway needs `--allow-remote-gateway`), asks once naming the device and the
//! gateway, and reports the outcome. The exit code is honest: a device that
//! does not come back exits non-zero.

use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::Context;
use bussard_model::IndividualAddress;
use bussard_service::restart::{RestartWait, restart_device};
use bussard_service::{L4Options, SourcePolicy, WritePolicy};

use crate::conn_cmd::{
    ConnOverrides, enforce_write_gate, gateway_display, load_model_required, open_service,
    resolve_config,
};

/// The `restart <ADDRESS> --json` report.
#[derive(Debug, serde::Serialize)]
struct Report {
    /// The device address.
    address: String,
    /// The mask version the device reported before and after the restart.
    mask: String,
    /// How long after the restart the device answered again, in milliseconds.
    answered_after_ms: u128,
}

/// Runs `bussard restart <ADDRESS>`.
#[allow(clippy::too_many_arguments)] // the subcommand's flags, 1:1
pub fn run(
    address: &str,
    timeout: u64,
    yes: bool,
    allow_remote_gateway: bool,
    dir: &Path,
    json: bool,
    tool_key_source: crate::secure_key::ToolKeySource<'_>,
    overrides: ConnOverrides,
) -> anyhow::Result<ExitCode> {
    let target: IndividualAddress = address
        .parse()
        .with_context(|| format!("parsing device address {address:?}"))?;
    let model = load_model_required(dir)?;
    let activated = crate::secure_key::model_activated(model.as_ref(), target);
    let tool_key = crate::secure_key::resolve(target, tool_key_source, activated)?;
    let presented_tool_key = tool_key.is_some();
    let config = resolve_config(model.as_ref(), &overrides)?;
    let gateway = gateway_display(&config);

    enforce_write_gate(&config, allow_remote_gateway)?;
    eprintln!("gateway: {gateway}");
    let name = model
        .as_ref()
        .and_then(|m| m.devices.get(&target))
        .map(|d| format!(" ({})", d.device.name))
        .unwrap_or_default();
    let confirmed = crate::confirm::confirm(
        yes,
        &format!("restart {target}{name} via {gateway}?"),
        &format!("restart {target} via {gateway}"),
    )?;
    if !confirmed {
        eprintln!("aborted; nothing was sent.");
        return Ok(ExitCode::FAILURE);
    }

    let wait = RestartWait {
        timeout: Duration::from_secs(timeout),
        ..RestartWait::default()
    };
    let runtime = tokio::runtime::Runtime::new()?;
    let outcome = runtime
        .block_on(async move {
            let service = open_service(config, WritePolicy::transmit(allow_remote_gateway)).await?;
            let options = L4Options {
                source: SourcePolicy::Check {
                    skip: overrides.skip_address_check,
                },
                tool_key,
                high_water: bussard_secure::SequenceHighWater::new(),
                ..L4Options::default()
            };
            let outcome = restart_device(&service, target, &options, wait).await;
            service.close().await;
            anyhow::Ok(outcome)
        })
        .map_err(|err| crate::secure_key::secure_hint(target, presented_tool_key, err))?;

    match outcome {
        Ok(done) => {
            let mask = format!("{:04X}", done.mask);
            if json {
                crate::output::print(
                    crate::output::schema::RESTART,
                    &Report {
                        address: target.to_string(),
                        mask,
                        answered_after_ms: done.answered_after.as_millis(),
                    },
                )?;
            } else {
                println!(
                    "{target} restarted: it answered again after {:.1} s (mask {mask})",
                    done.answered_after.as_secs_f64()
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        Err(err) => {
            eprintln!("error: {err}");
            Ok(ExitCode::FAILURE)
        }
    }
}
