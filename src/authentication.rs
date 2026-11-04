//! Shared EAP handshake decisions for the service and direct CLI.
use crate::protocol::{self, Packet};
use anyhow::Result;

pub enum Exchange {
    Ignore,
    Response { bytes: Vec<u8>, peer_changed: bool },
    Success,
    Failure,
}

pub fn addressed_to(packet: &Packet<'_>, local: [u8; 6]) -> bool {
    packet.source != local
        && packet.source != [0; 6]
        && packet.source[0] & 1 == 0
        && (packet.destination == local || packet.destination == protocol::PAE)
}

pub fn receive(
    peer: &mut Option<[u8; 6]>,
    local: [u8; 6],
    packet: &Packet<'_>,
    username: &[u8],
    password: &[u8],
    online: bool,
) -> Result<Exchange> {
    if !addressed_to(packet, local) {
        return Ok(Exchange::Ignore);
    }
    let changed = peer.is_some_and(|peer| peer != packet.source);
    if (changed && !(online && packet.code == 1)) || (peer.is_none() && packet.code != 1) {
        return Ok(Exchange::Ignore);
    }
    match packet.code {
        1 => {
            let destination = if online { protocol::PAE } else { packet.source };
            let response = match protocol::answer(local, destination, packet, username, password) {
                Err(_) if changed => return Ok(Exchange::Ignore),
                response => response?,
            };
            if let Some(bytes) = response {
                *peer = Some(packet.source);
                Ok(Exchange::Response {
                    bytes,
                    peer_changed: changed,
                })
            } else {
                Ok(Exchange::Ignore)
            }
        }
        3 => Ok(Exchange::Success),
        4 => Ok(Exchange::Failure),
        _ => Ok(Exchange::Ignore),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_request_does_not_pin_the_authenticator() {
        let local = [2; 6];
        let mut peer = None;
        let packet = Packet {
            source: [4; 6],
            destination: local,
            code: 1,
            id: 1,
            kind: Some(25),
            data: &[],
        };
        assert!(matches!(
            receive(&mut peer, local, &packet, b"user", b"password", false).unwrap(),
            Exchange::Ignore
        ));
        assert!(peer.is_none());
        let packet = Packet {
            source: [6; 6],
            kind: Some(1),
            ..packet
        };
        assert!(matches!(
            receive(&mut peer, local, &packet, b"user", b"password", false).unwrap(),
            Exchange::Response { .. }
        ));
        assert_eq!(peer, Some([6; 6]));
    }

    #[test]
    fn probe_and_handshake_reject_invalid_sources() {
        let local = [2; 6];
        for source in [[0; 6], [1; 6], local] {
            let packet = Packet {
                source,
                destination: local,
                code: 1,
                id: 1,
                kind: Some(1),
                data: &[],
            };
            assert!(!addressed_to(&packet, local));
            assert!(matches!(
                receive(&mut None, local, &packet, b"user", b"password", false).unwrap(),
                Exchange::Ignore
            ));
        }
    }
}
