//! The read side of the KNXnet/IP parameter object (issue #391): read an IP
//! interface's object type 11 over a management connection, decode it into
//! [`KnxnetFacts`] and keep it with the device facts.
//!
//! `status <ADDRESS>`, `describe` and `adopt` call [`read_knxnet`] on the
//! connection they already hold. It addresses the object by the index the
//! object table or `PID_IO_LIST` gives (plain property reads), else by object
//! type and instance (the extended property services) after one short probe.
//! A device without the object is `None`. Read-only: nothing is written, and
//! no KNXnet/IP Secure property (PID 91 and up) is read.

use bussard_mgmt::knxnet::{
    KnxnetAccess, KnxnetValues, OT_KNXNET_IP_PARAMETER, knxnet_index_from_io_list,
    probe_knxnet_ext, read_knxnet_parameters, with_probe_timeout,
};
use bussard_mgmt::{L4Channel, Layer4Connection, MgmtError};

use crate::facts::{Established, FactsCache};
use bussard_model::knxnet::{
    IpAssignment, KnxnetFacts, capability_names, format_mac, method_names, state_label,
};

/// The index of the KNXnet/IP parameter object in `table` (`(index, object
/// type)` pairs), when the device has one.
pub fn knxnet_object_index(table: &[(u8, u16)]) -> Option<u8> {
    table
        .iter()
        .find(|(_, ot)| *ot == OT_KNXNET_IP_PARAMETER)
        .map(|(index, _)| *index)
}

/// Reads the KNXnet/IP parameter object of the device behind `l4`.
///
/// `table` is the object table in effect (from the device facts),
/// `table_from_io_list` whether it came from `PID_IO_LIST`. The object is
/// found, in this order:
///
/// 1. in `table` (the `PID_OBJECT_TYPE` walk or a checked `PID_IO_LIST`): read
///    by index with the plain property services;
/// 2. in the device object's `PID_IO_LIST` at an index the walk did not reach,
///    when that index answers: read by index;
/// 3. by object type 11, instance 1, with one extended-service probe
///    ([`probe_knxnet_ext`]): read by type and instance. The Jung IPS 300 SREG
///    (mask 0021) is found here: its walk lists a device object and an address
///    table only (live `describe 1.1.200`, 2026-10-26).
///
/// Steps 2 and 3 wait at most [`bussard_mgmt::knxnet::PROBE_RESPONSE_TIMEOUT`]
/// per request, and a device that leaves them unanswered keeps the connection.
/// A device without the object is `None`.
///
/// # Errors
///
/// A transport failure or a malformed response.
pub async fn read_knxnet<Ch: L4Channel>(
    l4: &mut Layer4Connection<Ch>,
    table: &[(u8, u16)],
    table_from_io_list: bool,
) -> Result<Option<KnxnetFacts>, MgmtError> {
    let from_io_list = if table_from_io_list {
        // The table already is the checked `PID_IO_LIST`.
        None
    } else {
        knxnet_index_from_io_list(l4).await?
    };
    let access = match knxnet_object_index(table).or(from_io_list) {
        Some(index) => KnxnetAccess::Index(index),
        None if probe_knxnet_ext(l4, 1).await? => KnxnetAccess::Ext { instance: 1 },
        None => return Ok(None),
    };
    let values = match access {
        KnxnetAccess::Index(_) => read_knxnet_parameters(l4, access).await?,
        // A device found by the probe may leave some of the properties
        // unanswered; each such read waits the probe timeout, not 3 s.
        KnxnetAccess::Ext { .. } => {
            with_probe_timeout(l4, async |l4| read_knxnet_parameters(l4, access).await).await?
        }
    };
    if values == KnxnetValues::default() {
        return Ok(None);
    }
    let index = match access {
        KnxnetAccess::Index(index) => Some(index),
        KnxnetAccess::Ext { .. } => None,
    };
    Ok(Some(facts_from_values(
        &values,
        index,
        bussard_model::facts::now_stamp(),
    )))
}

/// The KNXnet/IP state of the device behind `l4` with the device facts in
/// effect: `None` without a read when the facts record that the device has
/// no KNXnet/IP parameter object (`no_knxnet`, cleared whenever the facts are
/// read again, `--refresh-facts` included); otherwise [`read_knxnet`], whose
/// result (the values, or the absence) is recorded in the facts.
///
/// # Errors
///
/// As [`read_knxnet`].
pub async fn read_knxnet_with_facts<Ch: L4Channel>(
    l4: &mut Layer4Connection<Ch>,
    facts: &FactsCache,
    established: &mut Established,
) -> Result<Option<KnxnetFacts>, MgmtError> {
    if established.record.as_ref().is_some_and(|r| r.no_knxnet) {
        return Ok(None);
    }
    let table = established.object_table();
    let from_io_list = established
        .record
        .as_ref()
        .is_some_and(|r| r.object_table_source == bussard_model::facts::ObjectTableSource::IoList);
    let knxnet = read_knxnet(l4, &table, from_io_list).await?;
    facts.remember_knxnet(established, knxnet.clone());
    Ok(knxnet)
}

/// Decodes the raw property values into the stored facts.
pub fn facts_from_values(
    v: &KnxnetValues,
    object_index: Option<u8>,
    read_at: String,
) -> KnxnetFacts {
    let ip = |q: Option<[u8; 4]>| q.map(std::net::Ipv4Addr::from);
    let names = |bits: Option<u8>| {
        bits.map(|b| {
            method_names(b)
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
    };
    KnxnetFacts {
        read_at,
        object_index,
        individual_address: v.individual_address,
        tunnels: v.additional_addresses.clone(),
        current_assignment: names(v.current_assignment),
        assignment: names(v.assignment),
        assignment_mode: v.assignment.and_then(IpAssignment::from_method_bits),
        current_address: ip(v.current_ip),
        current_netmask: ip(v.current_netmask),
        current_gateway: ip(v.current_gateway),
        address: ip(v.ip),
        netmask: ip(v.netmask),
        gateway: ip(v.gateway),
        mac: v.mac.map(|m| format_mac(&m)),
        system_setup_multicast: ip(v.system_setup_multicast),
        routing_multicast: ip(v.routing_multicast),
        ttl: v.ttl,
        capabilities: v.capabilities.map(|c| {
            capability_names(c)
                .into_iter()
                .map(str::to_string)
                .collect()
        }),
        state: v.state.map(state_label),
        friendly_name: v.friendly_name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_facts_from_values_decodes_every_field() -> Result<(), Box<dyn std::error::Error>> {
        let values = KnxnetValues {
            individual_address: Some("1.1.200".parse()?),
            additional_addresses: vec!["1.1.22".parse()?, "1.1.23".parse()?],
            current_assignment: Some(0x04),
            assignment: Some(0x04),
            current_ip: Some([192, 168, 1, 74]),
            current_netmask: Some([255, 255, 255, 0]),
            current_gateway: Some([192, 168, 1, 1]),
            ip: Some([0, 0, 0, 0]),
            mac: Some([0x00, 0x00, 0x5E, 0x00, 0x53, 0x01]),
            routing_multicast: Some([224, 0, 23, 12]),
            capabilities: Some(0x0003),
            state: Some(0),
            friendly_name: Some("Alpha".to_string()),
            ..KnxnetValues::default()
        };
        let facts = facts_from_values(&values, Some(4), "2026-10-26T10:00:00Z".to_string());
        assert_eq!(facts.assignment_mode, Some(IpAssignment::Dhcp));
        assert_eq!(facts.current_address, Some("192.168.1.74".parse()?));
        assert_eq!(facts.mac.as_deref(), Some("00:00:5E:00:53:01"));
        assert_eq!(facts.state.as_deref(), Some("ok"));
        assert_eq!(
            facts.capabilities,
            Some(vec![
                "device management".to_string(),
                "tunnelling".to_string()
            ])
        );
        let lines = facts.lines();
        assert_eq!(lines[0], ("IP address", "192.168.1.74 (dhcp)".to_string()));
        assert!(
            lines
                .iter()
                .any(|(k, v)| *k == "tunnels" && v == "1.1.22, 1.1.23")
        );
        Ok(())
    }

    #[test]
    fn test_knxnet_object_index_finds_type_11() {
        assert_eq!(knxnet_object_index(&[(0, 0), (1, 1), (4, 11)]), Some(4));
        assert_eq!(knxnet_object_index(&[(0, 0)]), None);
    }
}
