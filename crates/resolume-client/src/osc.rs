//! OSC sender (notes §6). One UDP socket; sends never wait on the network.
//!
//! - Clip connect: `/composition/layers/{L}/clips/{C}/connect` int 1 only. A release (int 0)
//!   would disconnect clips whose trigger style is Piano.
//! - Layer clear: `/composition/layers/{L}/clear` int 1 then int 0. The clear button is
//!   edge-triggered; without the release the next clear is ignored.
//! - Deck select: `/composition/decks/{n}/select` int 1.
//!
//! Indices are 1-based, the same as REST.

use std::io;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};

use rosc::{OscMessage, OscPacket, OscType};

pub struct OscSender {
    socket: UdpSocket,
    target: SocketAddr,
}

impl OscSender {
    pub fn new(target: &str) -> io::Result<Self> {
        let target = target
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no address"))?;
        let bind = if target.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        };
        let socket = UdpSocket::bind(bind)?;
        Ok(Self { socket, target })
    }

    pub fn target(&self) -> SocketAddr {
        self.target
    }

    /// Sends one message with int arguments. Returns the address for logging.
    pub fn send(&self, addr: &str, ints: &[i32]) -> io::Result<()> {
        let packet = OscPacket::Message(OscMessage {
            addr: addr.to_owned(),
            args: ints.iter().map(|&i| OscType::Int(i)).collect(),
        });
        let bytes = rosc::encoder::encode(&packet)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        self.socket.send_to(&bytes, self.target)?;
        Ok(())
    }

    pub fn connect_clip(&self, layer: usize, column: usize) -> io::Result<String> {
        let addr = connect_address(layer, column);
        self.send(&addr, &[1])?;
        Ok(addr)
    }

    pub fn clear_layer(&self, layer: usize) -> io::Result<String> {
        let addr = clear_address(layer);
        self.send(&addr, &[1])?;
        self.send(&addr, &[0])?;
        Ok(addr)
    }

    pub fn select_deck(&self, deck: usize) -> io::Result<String> {
        let addr = format!("/composition/decks/{deck}/select");
        self.send(&addr, &[1])?;
        Ok(addr)
    }
}

pub fn connect_address(layer: usize, column: usize) -> String {
    format!("/composition/layers/{layer}/clips/{column}/connect")
}

pub fn clear_address(layer: usize) -> String {
    format!("/composition/layers/{layer}/clear")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sends_decodable_messages() {
        let rx = UdpSocket::bind("127.0.0.1:0").unwrap();
        let osc = OscSender::new(&rx.local_addr().unwrap().to_string()).unwrap();
        assert_eq!(
            osc.connect_clip(2, 3).unwrap(),
            "/composition/layers/2/clips/3/connect"
        );
        osc.clear_layer(4).unwrap();
        let mut got = Vec::new();
        let mut buf = [0u8; 1024];
        for _ in 0..3 {
            let n = rx.recv(&mut buf).unwrap();
            let (_, OscPacket::Message(m)) = rosc::decoder::decode_udp(&buf[..n]).unwrap() else {
                panic!("expected a message")
            };
            got.push((m.addr, m.args));
        }
        assert_eq!(
            got,
            vec![
                (
                    "/composition/layers/2/clips/3/connect".into(),
                    vec![OscType::Int(1)]
                ),
                ("/composition/layers/4/clear".into(), vec![OscType::Int(1)]),
                ("/composition/layers/4/clear".into(), vec![OscType::Int(0)]),
            ]
        );
    }
}
