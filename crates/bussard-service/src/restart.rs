//! Restarting one device: the write behind `bussard restart`.
//!
//! A basic restart (`A_Restart` with no erase code) re-runs the device's
//! application, as a power cycle of that one device would: parameters, tables
//! and the individual address stay. It is what ETS's "Restart device" sends,
//! and the last step of every download. A device may transmit its status
//! objects as it starts, so a restart can act on the bus like a bus-voltage
//! return does.
//!
//! The device reboots on the request and drops the connection without
//! acknowledging it ([`bussard_mgmt::DeviceConnection::restart`]). So
//! [`restart_device`] reads the device's mask first, sends the restart, then
//! reconnects until the device answers its descriptor read again with the
//! same mask. Only that answer verifies the restart; a device that stays
//! silent, or comes back as another mask, is an error.

use std::time::{Duration, Instant};

use bussard_model::IndividualAddress;

use crate::{BusService, L4Options, ServiceError};

/// How long [`restart_device`] waits for the device to come back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestartWait {
    /// The pause after the restart before the first reconnect, so the probe
    /// does not reach a device that has not dropped the old connection yet.
    pub settle: Duration,
    /// The pause between two reconnect attempts.
    pub retry: Duration,
    /// How long after the restart the device may take to answer.
    pub timeout: Duration,
}

impl Default for RestartWait {
    /// Two seconds to settle, a retry every second, thirty seconds in all: a
    /// TP1 device answers within a few seconds of its restart.
    fn default() -> Self {
        RestartWait {
            settle: Duration::from_secs(2),
            retry: Duration::from_secs(1),
            timeout: Duration::from_secs(30),
        }
    }
}

/// A verified restart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestartOutcome {
    /// The device's mask, read before the restart and again after it.
    pub mask: u16,
    /// How long after the restart the device answered again.
    pub answered_after: Duration,
}

/// Restarting a device failed.
#[derive(Debug, thiserror::Error)]
pub enum RestartError {
    /// The device could not be reached before the restart, or refused it.
    #[error(transparent)]
    Service(#[from] ServiceError),
    /// The restart was sent, but the device did not answer again in time.
    #[error(
        "{address} did not answer within {}s of the restart (last error: {last})",
        .timeout.as_secs()
    )]
    NoAnswer {
        /// The device.
        address: IndividualAddress,
        /// How long bussard waited.
        timeout: Duration,
        /// The last reconnect's error.
        last: String,
    },
    /// The device answered again, but with another mask.
    #[error("{address} answered with mask {after:04X} after the restart, {before:04X} before it")]
    MaskChanged {
        /// The device.
        address: IndividualAddress,
        /// The mask before the restart.
        before: u16,
        /// The mask after it.
        after: u16,
    },
}

/// Restarts `target` and waits until it answers again (see the module docs).
///
/// # Errors
///
/// [`RestartError::Service`] when the device cannot be reached or the restart
/// cannot be sent, [`RestartError::NoAnswer`] when it does not come back
/// within `wait.timeout`, [`RestartError::MaskChanged`] when it comes back as
/// another mask.
pub async fn restart_device(
    service: &BusService,
    target: IndividualAddress,
    options: &L4Options,
    wait: RestartWait,
) -> Result<RestartOutcome, RestartError> {
    let before = service
        .with_device(target, options, async |dev| {
            let mask = dev.device_descriptor().await?;
            dev.restart().await?;
            Ok::<u16, ServiceError>(mask)
        })
        .await?;
    let started = Instant::now();
    let after = wait_for_answer(target, wait, async || {
        service
            .with_device(target, options, async |dev| {
                Ok::<u16, ServiceError>(dev.device_descriptor().await?)
            })
            .await
            .map_err(|err| err.to_string())
    })
    .await?;
    if after != before {
        return Err(RestartError::MaskChanged {
            address: target,
            before,
            after,
        });
    }
    Ok(RestartOutcome {
        mask: before,
        answered_after: started.elapsed(),
    })
}

/// Runs `probe` after `wait.settle`, then every `wait.retry`, until it answers
/// or `wait.timeout` has passed since the call.
async fn wait_for_answer<F>(
    target: IndividualAddress,
    wait: RestartWait,
    mut probe: F,
) -> Result<u16, RestartError>
where
    F: AsyncFnMut() -> Result<u16, String>,
{
    let deadline = Instant::now() + wait.timeout;
    tokio::time::sleep(wait.settle).await;
    loop {
        match probe().await {
            Ok(mask) => return Ok(mask),
            Err(last) if Instant::now() + wait.retry >= deadline => {
                return Err(RestartError::NoAnswer {
                    address: target,
                    timeout: wait.timeout,
                    last,
                });
            }
            Err(err) => {
                tracing::debug!("{target} not back yet after the restart: {err}");
                tokio::time::sleep(wait.retry).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quick() -> RestartWait {
        RestartWait {
            settle: Duration::from_millis(1),
            retry: Duration::from_millis(1),
            timeout: Duration::from_millis(200),
        }
    }

    #[tokio::test]
    async fn test_wait_for_answer_retries_until_the_device_is_back()
    -> Result<(), Box<dyn std::error::Error>> {
        let target: IndividualAddress = "1.1.39".parse()?;
        let mut calls = 0;
        let mask = wait_for_answer(target, quick(), async || {
            calls += 1;
            if calls < 3 {
                Err("no response".to_string())
            } else {
                Ok(0x0705)
            }
        })
        .await?;
        assert_eq!(mask, 0x0705);
        assert_eq!(calls, 3);
        Ok(())
    }

    #[tokio::test]
    async fn test_wait_for_answer_gives_up_at_the_timeout() -> Result<(), Box<dyn std::error::Error>>
    {
        let target: IndividualAddress = "1.1.39".parse()?;
        let err = wait_for_answer(target, quick(), async || Err("no response".to_string()))
            .await
            .err()
            .ok_or("a device that never answers is an error")?;
        assert!(
            matches!(&err, RestartError::NoAnswer { last, .. } if last == "no response"),
            "{err}"
        );
        Ok(())
    }
}
