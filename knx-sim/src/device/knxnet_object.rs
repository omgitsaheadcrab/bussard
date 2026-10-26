//! The KNXnet/IP parameter object (interface object type 11) of a simulated
//! KNX IP interface (bussard issue #391), read side.
//!
//! A KNXnet/IP interface or router exposes its IP configuration through this
//! object: its KNX individual address (PID 52), the additional individual
//! addresses its tunnelling connections use (PID 53, an array), the current
//! and configured IP assignment method (PIDs 54, 55: bit 0 manual, bit 1
//! BootP, bit 2 DHCP, bit 3 AutoIP), the current IP address, netmask and
//! default gateway (57 to 59), the configured static ones (60 to 62), the MAC
//! address (64), the multicast addresses (65, 66), the TTL (67), the device
//! capabilities (68) and state (69) and the friendly name (76, 30 octets of
//! ISO 8859-1, NUL-padded). Ids and data types from the KNX master data
//! (`knx_master.xml`, `OT-11`); the encodings from the KNX standard volume
//! 3/8/3 (KNXnet/IP Device Management).
//!
//! The simulator serves the properties read-only with array semantics: a read
//! of start index 0 answers the element count, a read of `count` elements
//! from `start` answers those elements, and anything past the end is refused
//! with the zero-count response. KNXnet/IP Secure PIDs (91 and up) are not
//! modelled.

use super::interface_object::{InterfaceObject, PID_OBJECT_TYPE, Property};

/// The interface-object type of the KNXnet/IP parameter object.
pub const IOT_KNXNET_IP_PARAMETER: u16 = 11;

/// The IP configuration a simulated interface reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnxnetParameters {
    /// PID 52, the interface's individual address (raw).
    pub individual_address: u16,
    /// PID 53, the tunnelling addresses (raw), in slot order.
    pub tunnels: Vec<u16>,
    /// PID 54, the method bits in use.
    pub current_assignment: u8,
    /// PID 55, the configured method bits.
    pub assignment: u8,
    /// PIDs 57 to 59: address, netmask, gateway in use.
    pub current: [[u8; 4]; 3],
    /// PIDs 60 to 62: the configured static address, netmask, gateway.
    pub configured: [[u8; 4]; 3],
    /// PID 64.
    pub mac: [u8; 6],
    /// PID 65.
    pub system_setup_multicast: [u8; 4],
    /// PID 66.
    pub routing_multicast: [u8; 4],
    /// PID 67.
    pub ttl: u8,
    /// PID 68: device management and tunnelling.
    pub capabilities: u16,
    /// PID 69: 0, no fault.
    pub state: u8,
    /// PID 76.
    pub friendly_name: String,
}

impl KnxnetParameters {
    /// A DHCP interface at `individual_address` with `tunnels`.
    pub fn dhcp(individual_address: u16, tunnels: Vec<u16>) -> Self {
        KnxnetParameters {
            individual_address,
            tunnels,
            current_assignment: 0x04,
            assignment: 0x04,
            current: [[192, 168, 1, 74], [255, 255, 255, 0], [192, 168, 1, 1]],
            configured: [[0; 4]; 3],
            mac: [0x00, 0x00, 0x5E, 0x00, 0x53, 0x01],
            system_setup_multicast: [224, 0, 23, 12],
            routing_multicast: [224, 0, 23, 12],
            ttl: 16,
            capabilities: 0x0003,
            state: 0,
            friendly_name: "knx-sim IP interface".to_string(),
        }
    }

    /// The interface object holding these values.
    pub fn object(&self) -> InterfaceObject {
        let mut obj = InterfaceObject::new();
        obj.set_property(
            PID_OBJECT_TYPE,
            Property::read_only(IOT_KNXNET_IP_PARAMETER.to_be_bytes().to_vec()),
        );
        let mut name: Vec<u8> = self
            .friendly_name
            .chars()
            .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
            .take(30)
            .collect();
        name.resize(30, 0);
        let values: [(u8, Vec<u8>); 17] = [
            (52, self.individual_address.to_be_bytes().to_vec()),
            (
                53,
                self.tunnels.iter().flat_map(|t| t.to_be_bytes()).collect(),
            ),
            (54, vec![self.current_assignment]),
            (55, vec![self.assignment]),
            (57, self.current[0].to_vec()),
            (58, self.current[1].to_vec()),
            (59, self.current[2].to_vec()),
            (60, self.configured[0].to_vec()),
            (61, self.configured[1].to_vec()),
            (62, self.configured[2].to_vec()),
            (64, self.mac.to_vec()),
            (65, self.system_setup_multicast.to_vec()),
            (66, self.routing_multicast.to_vec()),
            (67, vec![self.ttl]),
            (68, self.capabilities.to_be_bytes().to_vec()),
            (69, vec![self.state]),
            (76, name),
        ];
        for (pid, value) in values {
            obj.set_property(pid, Property::read_only(value));
        }
        obj
    }
}

/// The element size in octets of a KNXnet/IP parameter object property.
pub fn element_size(pid: u8) -> usize {
    match pid {
        52 | 53 | 68 => 2,
        57..=62 | 65 | 66 => 4,
        64 => 6,
        _ => 1,
    }
}

/// The answer to `A_PropertyValue_Read(count, start)` of a property holding
/// `value` (`pid` decides the element size): `(count, octets)`, or `None` for
/// the zero-count refusal. Start 0 is the element count; a range past the end
/// is refused whole, as a real device does.
pub fn array_read(pid: u8, value: &[u8], count: u8, start: u16) -> Option<(u8, Vec<u8>)> {
    let size = element_size(pid);
    let total = value.len() / size;
    if start == 0 {
        let n = u16::try_from(total).ok()?;
        return Some((1, n.to_be_bytes().to_vec()));
    }
    let first = usize::from(start);
    let last = first + usize::from(count).max(1) - 1;
    if count == 0 || last > total {
        return None;
    }
    Some((count, value[(first - 1) * size..last * size].to_vec()))
}

/// The `A_PropertyExtValue_Response` payload for an `A_PropertyExtValue_Read`
/// (`[type:16][instance:12|pid:12][count][start:16]`) of the KNXnet/IP
/// parameter object `obj`: the echoed header, the served count, the start and
/// the elements, with count 0 for an unknown property, another instance or a
/// range past the end. `None` when the request addresses another object type
/// (the security object's handler takes it) or is too short.
pub fn ext_read(obj: &InterfaceObject, request: &[u8]) -> Option<Vec<u8>> {
    let h = request.get(..8)?;
    let object_type = u16::from_be_bytes([h[0], h[1]]);
    if object_type != IOT_KNXNET_IP_PARAMETER {
        return None;
    }
    let packed = (u32::from(h[2]) << 16) | (u32::from(h[3]) << 8) | u32::from(h[4]);
    let instance = packed >> 12;
    let pid = u8::try_from(packed & 0x0FFF).ok();
    let count = h[5];
    let start = u16::from_be_bytes([h[6], h[7]]);
    let answer = match (instance, pid) {
        (1, Some(pid)) if pid != PID_OBJECT_TYPE => obj
            .property(pid)
            .and_then(|p| array_read(pid, &p.value, count, start)),
        _ => None,
    };
    let (n, bytes) = answer.unwrap_or((0, Vec::new()));
    let mut out = h[..5].to_vec();
    out.push(n);
    out.extend_from_slice(&start.to_be_bytes());
    out.extend_from_slice(&bytes);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_array_read_count_elements_and_refusal() {
        let p = KnxnetParameters::dhcp(0x11C8, vec![0x1116, 0x1117, 0x1118]);
        let obj = p.object();
        let tunnels = obj
            .property(53)
            .map(|p| p.value.clone())
            .unwrap_or_default();
        assert_eq!(array_read(53, &tunnels, 1, 0), Some((1, vec![0, 3])));
        assert_eq!(
            array_read(53, &tunnels, 2, 2),
            Some((2, vec![0x11, 0x17, 0x11, 0x18]))
        );
        assert_eq!(array_read(53, &tunnels, 2, 3), None, "past the end");
        let name = obj
            .property(76)
            .map(|p| p.value.clone())
            .unwrap_or_default();
        assert_eq!(name.len(), 30);
        assert_eq!(array_read(76, &name, 1, 0), Some((1, vec![0, 30])));
        let ip = obj
            .property(57)
            .map(|p| p.value.clone())
            .unwrap_or_default();
        assert_eq!(array_read(57, &ip, 1, 1), Some((1, vec![192, 168, 1, 74])));
        assert_eq!(
            obj.property(PID_OBJECT_TYPE).map(|p| p.value.clone()),
            Some(vec![0, 11])
        );
    }
}
