//! The KNXnet/IP device management endpoint of a simulated KNX IP interface
//! (bussard issue #398), over a real UDP socket: a device management CONNECT,
//! `DEVICE_CONFIGURATION_REQUEST`s carrying cEMI `M_PropWrite.req` /
//! `M_PropRead.req` on the KNXnet/IP parameter object and its Secure
//! properties, and `T_Data_Connected.req` (as ETS sends them to the Jung
//! interface) answered with `T_Data_Connected.ind`. Synthetic keys only.

use std::net::UdpSocket;
use std::sync::Arc;
use std::time::Duration;

use knx_sim::bus::Bus;
use knx_sim::bus::event::TracingSink;
use knx_sim::device::knxnet_object::KnxnetParameters;
use knx_sim::device::{Device, LoadState};
use knx_sim::net::KnxnetIpServer;
use knx_sim::wire::IndividualAddress;
use knx_sim::wire::knxnetip::{KnxnetIpFrame, service};

const INTERFACE: u16 = 0x11C8; // 1.1.200

fn server() -> KnxnetIpServer {
    let sink = Arc::new(TracingSink);
    let product = knx_sim::testfixtures::synthetic_system_b_product();
    let device = Device::from_product(
        IndividualAddress(INTERFACE),
        &product,
        LoadState::Loaded,
        sink.clone(),
    )
    .with_knxnet(
        &KnxnetParameters::dhcp(INTERFACE, vec![0x1116, 0x1117]),
        false,
    );
    let mut bus = Bus::new(sink);
    bus.add_device(device);
    let mut server = KnxnetIpServer::bind("127.0.0.1:0".parse().expect("addr"), bus).expect("bind");
    server.set_interface_device(INTERFACE);
    server
}

/// Sends `frame`, lets the server handle one datagram, returns every answer.
fn exchange(client: &UdpSocket, server: &mut KnxnetIpServer, frame: &[u8]) -> Vec<KnxnetIpFrame> {
    let addr = server.local_addr().expect("addr");
    client.send_to(frame, addr).expect("send");
    server.serve_n(1).expect("serve");
    let mut out = Vec::new();
    let mut buf = [0u8; 512];
    while let Ok((n, _)) = client.recv_from(&mut buf) {
        out.push(KnxnetIpFrame::decode(&buf[..n]).expect("frame"));
    }
    out
}

fn config_request(channel: u8, seq: u8, cemi: &[u8]) -> Vec<u8> {
    let mut body = vec![0x04, channel, seq, 0x00];
    body.extend_from_slice(cemi);
    KnxnetIpFrame::encode(service::DEVICE_CONFIGURATION_REQUEST, &body)
}

/// The cEMI frames of the DEVICE_CONFIGURATION_REQUESTs among `frames`.
fn cemis(frames: &[KnxnetIpFrame]) -> Vec<Vec<u8>> {
    frames
        .iter()
        .filter(|f| f.service == service::DEVICE_CONFIGURATION_REQUEST)
        .map(|f| f.body[4..].to_vec())
        .collect()
}

#[test]
fn test_device_management_reaches_the_interface_device() {
    let mut server = server();
    let client = UdpSocket::bind("127.0.0.1:0").expect("client");
    client
        .set_read_timeout(Some(Duration::from_millis(50)))
        .expect("timeout");
    let route_back = [0x08, 0x01, 127, 0, 0, 1, 0, 0];
    let mut connect = route_back.to_vec();
    connect.extend_from_slice(&route_back);
    connect.extend_from_slice(&[0x02, 0x03]);
    let answers = exchange(
        &client,
        &mut server,
        &KnxnetIpFrame::encode(service::CONNECT_REQUEST, &connect),
    );
    let resp = answers
        .iter()
        .find(|f| f.service == service::CONNECT_RESPONSE)
        .expect("CONNECT_RESPONSE");
    assert_eq!(resp.body[1], 0x00, "connected");
    assert_eq!(&resp.body[10..12], &[0x02, 0x03], "a device management CRD");
    let channel = resp.body[0];

    // M_PropWrite.req of PID_DEVICE_AUTHENTICATION_CODE (type 11, 16 octets).
    let mut write = vec![0xF6, 0x00, 0x0B, 0x01, 92, 0x10, 0x01];
    write.extend_from_slice(&[0x5A; 16]);
    let answers = exchange(&client, &mut server, &config_request(channel, 0, &write));
    assert!(
        answers
            .iter()
            .any(|f| f.service == service::DEVICE_CONFIGURATION_ACK)
    );
    assert_eq!(
        cemis(&answers),
        vec![vec![0xF5, 0x00, 0x0B, 0x01, 92, 0x10, 0x01]]
    );

    // PID_TUNNELLING_USERS: count 1, then the entry, then read back.
    exchange(
        &client,
        &mut server,
        &config_request(
            channel,
            1,
            &[0xF6, 0x00, 0x0B, 0x01, 97, 0x10, 0x00, 0x00, 0x01],
        ),
    );
    exchange(
        &client,
        &mut server,
        &config_request(
            channel,
            2,
            &[0xF6, 0x00, 0x0B, 0x01, 97, 0x10, 0x01, 0x02, 0x01],
        ),
    );
    let answers = exchange(
        &client,
        &mut server,
        &config_request(channel, 3, &[0xFC, 0x00, 0x0B, 0x01, 97, 0x10, 0x01]),
    );
    assert_eq!(
        cemis(&answers),
        vec![vec![0xFB, 0x00, 0x0B, 0x01, 97, 0x10, 0x01, 0x02, 0x01]]
    );
    // A write-only key reads back as an error, never its value.
    let answers = exchange(
        &client,
        &mut server,
        &config_request(channel, 4, &[0xFC, 0x00, 0x0B, 0x01, 92, 0x10, 0x01]),
    );
    assert_eq!(
        cemis(&answers),
        vec![vec![0xFB, 0x00, 0x0B, 0x01, 92, 0x00, 0x01, 0x07]]
    );

    // T_Data_Connected.req A_DeviceDescriptor_Read, as ETS sends it.
    let read = [0x41, 0x00, 0, 0, 0, 0, 0, 0, 0x01, 0x03, 0x00];
    let answers = exchange(&client, &mut server, &config_request(channel, 5, &read));
    let ind = cemis(&answers);
    assert_eq!(ind.len(), 1, "{ind:?}");
    assert_eq!(ind[0][0], 0x89, "T_Data_Connected.ind");
    assert_eq!(&ind[0][9..11], &[0x03, 0x40], "A_DeviceDescriptor_Response");

    let device = server
        .bus()
        .device(IndividualAddress(INTERFACE))
        .expect("device");
    assert_eq!(
        device.ip_secure().map(|ip| ip.users().to_vec()),
        Some(vec![(2, 1)])
    );
}
