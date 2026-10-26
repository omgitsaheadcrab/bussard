//! Integration tests for [`bussard_mgmt::knxnet`] (issue #391): the KNXnet/IP
//! parameter object of a mock IP interface, read by object index with the
//! plain property services and by object type and instance with the extended
//! ones, with the same result.

use std::time::Duration;

use bussard_mgmt::knxnet::{
    FIRST_SECURE_PID, KnxnetAccess, read_knxnet_parameters, read_knxnet_property,
};
use bussard_mgmt::{Layer4Connection, Timeouts};
use bussard_model::IndividualAddress;
use bussard_testkit::{BoxError, MockDevice, MockGateway, MockKnxnet, TestResult};
use bussard_transport::{ConnectionConfig, Transport};

const CHANNEL: u8 = 0x32;

fn knxnet() -> Result<MockKnxnet, BoxError> {
    Ok(MockKnxnet {
        tunnels: (22..=29)
            .map(|n| format!("1.1.{n}").parse())
            .collect::<Result<Vec<IndividualAddress>, _>>()?,
        friendly_name: "Bocklisch Alpha".to_string(),
        ..MockKnxnet::default()
    })
}

/// Reads the whole object through `access` from a mock interface at 1.1.200
/// (device object at index 0, the KNXnet/IP object at index 1).
async fn read_all(
    access: KnxnetAccess,
    max_apdu: Option<u16>,
) -> Result<(bussard_mgmt::knxnet::KnxnetValues, usize), BoxError> {
    let target: IndividualAddress = "1.1.200".parse()?;
    let device = MockDevice::new(target)
        .with_mask(0x0021)
        .with_knxnet(&knxnet()?);
    let gw = MockGateway::builder()
        .channel(CHANNEL)
        .idle_timeout(Duration::from_secs(30))
        .device(device)
        .start()
        .await?;
    let mut bus = Transport::connect(&ConnectionConfig::tunnel(gw.addr())).await?;
    let source: IndividualAddress = "0.0.255".parse()?;
    let mut l4 =
        Layer4Connection::connect_with(&mut bus, target, source, Timeouts::discovery()).await?;
    l4.set_max_apdu(max_apdu);
    let values = read_knxnet_parameters(&mut l4, access).await?;
    let refused = read_knxnet_property(&mut l4, access, FIRST_SECURE_PID + 1, 1, false).await;
    assert!(refused.is_err(), "a KNXnet/IP Secure PID is never read");
    let _ = l4.disconnect().await;
    let writes = gw.devices()?.first().map(|d| d.writes).unwrap_or_default();
    Ok((values, writes))
}

#[tokio::test]
async fn test_read_knxnet_parameters_by_index() -> TestResult {
    let (v, writes) = read_all(KnxnetAccess::Index(1), None).await?;
    assert_eq!(writes, 0, "read-only");
    assert_eq!(v.individual_address, Some("1.1.200".parse()?));
    assert_eq!(v.additional_addresses.len(), 8);
    assert_eq!(v.additional_addresses[0], "1.1.22".parse()?);
    assert_eq!(v.additional_addresses[7], "1.1.29".parse()?);
    assert_eq!(v.current_assignment, Some(0x04));
    assert_eq!(v.current_ip, Some([192, 168, 1, 74]));
    assert_eq!(v.mac, Some([0x00, 0x00, 0x5E, 0x00, 0x53, 0x01]));
    assert_eq!(v.capabilities, Some(0x0003));
    assert_eq!(v.friendly_name.as_deref(), Some("Bocklisch Alpha"));
    Ok(())
}

#[tokio::test]
async fn test_read_knxnet_parameters_by_type_and_instance_matches_index() -> TestResult {
    let (by_index, _) = read_all(KnxnetAccess::Index(1), Some(55)).await?;
    let (by_type, writes) = read_all(KnxnetAccess::Ext { instance: 1 }, Some(55)).await?;
    assert_eq!(writes, 0);
    assert_eq!(by_type, by_index);
    Ok(())
}

#[tokio::test]
async fn test_read_knxnet_parameters_on_another_instance_is_empty() -> TestResult {
    let (v, _) = read_all(KnxnetAccess::Ext { instance: 2 }, None).await?;
    assert_eq!(v, bussard_mgmt::knxnet::KnxnetValues::default());
    Ok(())
}
