//! The read side of the KNXnet/IP parameter object (issue #391): read an IP
//! interface's object type 11 over a management connection, decode it into
//! [`KnxnetFacts`] and keep it with the device facts.
//!
//! `status <ADDRESS>`, `describe` and `adopt` call [`read_knxnet`] on the
//! connection they already hold. It addresses the object by the index the
//! object table gives (plain property reads); a device whose object table is
//! unknown is read with the extended property services by object type and
//! instance. A device without the object is `None`, at no cost beyond the
//! object table the command read anyway. Read-only: nothing is written, and
//! no KNXnet/IP Secure property (PID 91 and up) is read.

use bussard_mgmt::knxnet::{
    KnxnetAccess, KnxnetValues, OT_KNXNET_IP_PARAMETER, read_knxnet_parameters,
};
use bussard_mgmt::{L4Channel, Layer4Connection, MgmtError};
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
/// `table` is the object table in effect (from the device facts). With the
/// object in it, the object is read by index; without, `None` is returned
/// unless the table is empty (unknown), in which case instance 1 of object
/// type 11 is tried with the extended property services and a device that
/// refuses them is `None` as well.
///
/// # Errors
///
/// A transport failure or a malformed response.
pub async fn read_knxnet<Ch: L4Channel>(
    l4: &mut Layer4Connection<Ch>,
    table: &[(u8, u16)],
) -> Result<Option<KnxnetFacts>, MgmtError> {
    let access = match knxnet_object_index(table) {
        Some(index) => KnxnetAccess::Index(index),
        None if table.is_empty() => KnxnetAccess::Ext { instance: 1 },
        None => return Ok(None),
    };
    let values = match read_knxnet_parameters(l4, access).await {
        Ok(values) => values,
        Err(MgmtError::ServiceRejected { .. }) if matches!(access, KnxnetAccess::Ext { .. }) => {
            return Ok(None);
        }
        Err(err) => return Err(err),
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
