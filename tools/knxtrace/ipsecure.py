"""KNXnet/IP Secure sessions in a capture: who, with which credentials, carrying what.

A KNXnet/IP Secure session cannot be decrypted from a capture, with any
keyring. The session key is `SHA-256(X25519 shared secret)[:16]` of an
ephemeral Diffie-Hellman exchange (KNX standard 3/8/9 "KNXnet/IP Secure",
session establishment; docs/knx-secure-spec.md §7.2, after the MIT XKNX
reference `ip_secure.py`). Both private halves stay in ETS and in the
interface; only the public keys travel. The passwords only authenticate:

- SESSION_RESPONSE carries a MAC under the device authentication code's key
  (PBKDF2-HMAC-SHA256, salt `device-authentication-code.1.secure.ip.knx.org`,
  65536 iterations, 16 octets; docs/knx-secure-spec.md §3.4, §7.4). It travels
  in the clear, so a keyring's code can be checked against it: that names the
  credentials the interface held at that moment.
- SESSION_AUTHENTICATE (the user id and a MAC under the user password's key,
  §7.5) travels inside the first SECURE_WRAPPER, encrypted. Which user a
  session ran as is not visible.

What stays visible is the shape: per session, how many wrapped frames went
each way and how long each plain frame was (a wrapper is 16 octets of
security information, the encrypted KNXnet/IP frame, a 16-octet MAC). The
second frame a client wraps is its CONNECT_REQUEST, and its length names the
connection: 26 octets is a tunnel CRI (4 octets) behind two TCP route-back
HPAIs, 24 a device management CRI (2 octets).

Nothing here prints key material: keys are compared, never shown.
"""

from __future__ import annotations

import hmac
import struct
from dataclasses import dataclass, field
from typing import Dict, Iterable, List, Optional, Tuple

from datasecure import BLOCK, Aes128, Keyring, cbc_mac, ctr_keystream

SESSION_REQUEST = 0x0951
SESSION_RESPONSE = 0x0952
SECURE_WRAPPER = 0x0950
PUBLIC_KEY_LEN = 32
MAC_LEN = 16
# counter_0 of the handshake MACs (docs/knx-secure-spec.md §7.4, bussard-secure
# `COUNTER_0_HANDSHAKE`).
COUNTER_0_HANDSHAKE = bytes(14) + b"\xff\x00"

# Plain lengths of the client's CONNECT_REQUEST inside a wrapper, over TCP.
CONNECT_REQUEST_LEN = {26: "tunnel", 24: "device_management"}


def session_response_mac(device_auth_key: bytes, session_id: int, client_pub: bytes, server_pub: bytes) -> bytes:
    """The MAC a SESSION_RESPONSE carries (docs/knx-secure-spec.md §7.4).

    CBC-MAC (zero `block_0`) under the device authentication key over the
    response header `06 10 09 52 00 38`, the session id and the XOR of the two
    public keys, encrypted with the handshake `counter_0`.
    """
    total = 6 + 2 + PUBLIC_KEY_LEN + MAC_LEN
    ad = b"\x06\x10" + struct.pack("!HH", SESSION_RESPONSE, total) + struct.pack("!H", session_id)
    ad += bytes(a ^ b for a, b in zip(client_pub, server_pub))
    aes = Aes128(device_auth_key)
    mac = cbc_mac(aes, ad, b"", bytes(BLOCK))
    stream = ctr_keystream(aes, COUNTER_0_HANDSHAKE, MAC_LEN)
    return bytes(a ^ b for a, b in zip(mac, stream))


@dataclass
class Session:
    """One KNXnet/IP Secure session as the capture shows it."""

    client: str
    server: str
    start: float
    device: str = ""
    session_id: Optional[int] = None
    client_public: bytes = field(default=b"", repr=False)
    server_public: bytes = field(default=b"", repr=False)
    response_mac: bytes = field(default=b"", repr=False)
    # Plain lengths of the wrapped frames, in capture order, signed by
    # direction: positive client -> server, negative server -> client.
    wrapped: List[int] = field(default_factory=list)
    authenticated_by: str = ""

    @property
    def connection(self) -> str:
        """The connection the session carried, from its CONNECT_REQUEST length."""
        from_client = [n for n in self.wrapped if n > 0]
        if len(from_client) < 2:
            return "none (authentication only)"
        return CONNECT_REQUEST_LEN.get(from_client[1], "unknown (%d octets)" % from_client[1])

    def summary(self) -> str:
        to_server = sum(1 for n in self.wrapped if n > 0)
        to_client = sum(1 for n in self.wrapped if n < 0)
        sid = "0x%04x" % self.session_id if self.session_id is not None else "?"
        return "%s session %s %s -> %s: %s, %d wrapped frame(s) to the server, %d back; %s" % (
            self.device or "?",
            sid,
            self.client,
            self.server,
            self.connection,
            to_server,
            to_client,
            self.authenticated_by or "device authentication not checked (no keyring)",
        )

    def shape(self) -> str:
        """The wrapped frames' plain lengths, `>n` to the server, `<n` back."""
        return " ".join((">%d" % n) if n > 0 else ("<%d" % -n) for n in self.wrapped)


def _ip(endpoint: str) -> str:
    return endpoint.rsplit(":", 1)[0]


def sessions(frames: Iterable, ring: Optional[Keyring] = None) -> List[Session]:
    """Collects the secure sessions of a capture, in order of their request.

    A session is keyed by the client's endpoint (over TCP each session has its
    own connection). With a keyring, the SESSION_RESPONSE MAC is checked
    against every device authentication code it holds; `authenticated_by`
    names the one that verifies.
    """
    out: List[Session] = []
    open_by_client: Dict[str, Session] = {}
    servers: Dict[str, str] = {}
    for frame in frames:
        if frame.service in (0x0204, 0x0202, 0x020C) and isinstance(frame.fields.get("ia"), str):
            servers[_ip(frame.src)] = str(frame.fields["ia"])
        if frame.service == SESSION_REQUEST:
            sess = Session(frame.src, frame.dst, frame.ts, device=servers.get(_ip(frame.dst), ""))
            if len(frame.body) >= 8 + PUBLIC_KEY_LEN:
                sess.client_public = bytes(frame.body[8 : 8 + PUBLIC_KEY_LEN])
            open_by_client[frame.src] = sess
            out.append(sess)
        elif frame.service == SESSION_RESPONSE:
            sess = open_by_client.get(frame.dst)
            body = frame.body
            if sess is None or len(body) < 2 + PUBLIC_KEY_LEN + MAC_LEN:
                continue
            sess.session_id = struct.unpack("!H", body[0:2])[0]
            sess.server_public = bytes(body[2 : 2 + PUBLIC_KEY_LEN])
            sess.response_mac = bytes(body[2 + PUBLIC_KEY_LEN : 2 + PUBLIC_KEY_LEN + MAC_LEN])
            if ring is not None:
                sess.authenticated_by = _check_response(sess, ring)
        elif frame.service == SECURE_WRAPPER:
            plain = frame.fields.get("encrypted_len")
            if not isinstance(plain, int):
                continue
            sess = open_by_client.get(frame.src)
            if sess is not None:
                sess.wrapped.append(plain)
                continue
            sess = open_by_client.get(frame.dst)
            if sess is not None:
                sess.wrapped.append(-plain)
    return out


def _check_response(sess: Session, ring: Keyring) -> str:
    if len(sess.client_public) != PUBLIC_KEY_LEN or sess.session_id is None:
        return "device authentication not checked (no SESSION_REQUEST key)"
    device = None
    if sess.device:
        parts = sess.device.split(".")
        if len(parts) == 3 and all(p.isdigit() for p in parts):
            device = (int(parts[0]) << 12) | (int(parts[1]) << 8) | int(parts[2])
    candidates = ring.device_auth_keys(device) or ring.device_auth_keys()
    if not candidates:
        return "device authentication not checked (no code in the keyring)"
    for label, key in candidates:
        mac = session_response_mac(key, sess.session_id, sess.client_public, sess.server_public)
        if hmac.compare_digest(mac, sess.response_mac):
            return "device authentication verified: %s" % label
    return "device authentication matches no code in the keyring"


def report(found: List[Session], shapes: bool = False, base: float = 0.0) -> List[str]:
    """The text the `trace` and `devices` commands print for the sessions.

    `base` is the capture's first timestamp, so the times match the trace.
    """
    if not found:
        return []
    lines = [
        "KNXnet/IP Secure: %d session(s); their content is not decryptable "
        "(ephemeral X25519 session keys, see tools/knxtrace/ipsecure.py)" % len(found)
    ]
    for sess in found:
        lines.append("  %9.4f %s" % (sess.start - base, sess.summary()))
        if shapes and sess.wrapped:
            lines.append("            plain lengths: %s" % sess.shape())
    return lines


def connection_counts(found: List[Session]) -> List[Tuple[str, int]]:
    """How many sessions carried each kind of connection."""
    counts: Dict[str, int] = {}
    for sess in found:
        counts[sess.connection] = counts.get(sess.connection, 0) + 1
    return sorted(counts.items())
