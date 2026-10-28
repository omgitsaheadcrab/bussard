//! `bussard restart <ADDRESS>`: restart one device and wait until it answers
//! again.
//!
//! The restart, the choice of its form and the checks live in
//! [`bussard_service::restart`]. This module resolves the connection, applies
//! the write gate (a non-loopback gateway needs `--allow-remote-gateway`), asks
//! once naming the device and the gateway, and reports the outcome. The report
//! says "restarted" only for a confirmed restart the device answered with
//! error code 0; a basic restart is reported as sent, with the device answering
//! again afterwards. The exit code is honest: a confirmed restart the device
//! does not accept, or a device that does not come back, exits non-zero.

use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::Context;
use bussard_model::IndividualAddress;
use bussard_service::restart::{RestartWait, restart_choice, restart_device};
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
    /// The restart form sent: `confirmed` (master reset, erase code 1) or
    /// `basic` (a bare `A_Restart`).
    form: &'static str,
    /// Why that form: the application and whether it is secure-capable, or
    /// the missing product data.
    form_reason: String,
    /// Whether the device confirmed the restart itself (`A_Restart_Response`
    /// with error code 0). Always false for the basic form.
    confirmed: bool,
    /// The process time the device reported, in seconds (confirmed form).
    process_time_s: Option<u64>,
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
    // The form ETS uses for this device, from its product data (issue #452).
    let choice = restart_choice(dir, model.as_ref(), target);
    let gateway = gateway_display(&config);

    enforce_write_gate(&config, allow_remote_gateway)?;
    eprintln!("gateway: {gateway}");
    let name = model
        .as_ref()
        .and_then(|m| m.devices.get(&target))
        .map(|d| format!(" ({})", d.device.name))
        .unwrap_or_default();
    eprintln!("restart form: {} ({})", choice.form.name(), choice.why);
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
            let outcome = restart_device(&service, target, &options, choice.form, wait).await;
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
                        form: done.form.name(),
                        form_reason: choice.why,
                        confirmed: done.confirmed,
                        process_time_s: done.process_time.map(|t| t.as_secs()),
                        mask,
                        answered_after_ms: done.answered_after.as_millis(),
                    },
                )?;
            } else if done.confirmed {
                println!(
                    "{target} restarted: it confirmed the restart (A_Restart_Response, process \
                     time {} s) and answered again after {:.1} s (mask {mask})",
                    done.process_time.unwrap_or_default().as_secs(),
                    done.answered_after.as_secs_f64()
                );
            } else {
                println!(
                    "{target}: restart sent (a basic A_Restart, which the device does not \
                     confirm); it answered again after {:.1} s (mask {mask}). The restart itself \
                     is not verified.",
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
