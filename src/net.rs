use peers::Peers;
use serde::{Deserialize, Serialize};

pub const PEER_ID: &'static str = "00112233445566778899"; // This peer_id is artificial, it is used for getting the peer_id's of other peers during handshake.

#[derive(Debug, Serialize)]
pub struct TrackerSend {
    pub peer_id: String,
    pub port: u16,
    pub uploaded: usize,
    pub downloaded: usize,
    pub left: usize,
    pub compact: u8,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct TrackerResponse {
    pub interval: usize,
    pub peers: Peers,
}

#[repr(C)] // Consider a HandShake instance as a byte array for easier writing to peer via TCP connection
pub struct HandShake {
    pub len: u8,
    pub bittorrent: [u8; 19],
    pub reserved: [u8; 8],
    pub sha_hash: [u8; 20],
    pub peer_id: [u8; 20],
}

impl HandShake {
    pub fn new(hash: [u8; 20], peer_id: [u8; 20]) -> Self {
        Self {
            len: 19,
            bittorrent: *b"BitTorrent protocol",
            reserved: [0; 8],
            sha_hash: hash,
            peer_id,
        }
    }
}

pub mod peers {

    use serde::de::{self, Deserialize, Deserializer, Visitor};

    use std::fmt;

    use std::net::{Ipv4Addr, SocketAddrV4};

    #[derive(Debug, Clone)]

    pub struct Peers(pub Vec<SocketAddrV4>); // v4 and not v6 because "The first 4 bytes are the peer's IP address and the last 2 bytes are the peer's port number"

    struct PeersVisitor;

    impl<'de> Visitor<'de> for PeersVisitor {
        type Value = Peers;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("6 bytes, the first 4 bytes are a peer's IP address and the last 2 are a peer's port number")
        }

        fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if v.len() % 6 != 0 {
                return Err(E::custom(format!("length is {}", v.len())));
            }

            let addresses: Vec<SocketAddrV4> = v
                .chunks_exact(6) // First 6 elements are taken
                .map(|chunk_6| {
                    SocketAddrV4::new(
                        Ipv4Addr::new(chunk_6[0], chunk_6[1], chunk_6[2], chunk_6[3]),
                        u16::from_be_bytes([chunk_6[4], chunk_6[5]]),
                    )
                })
                .collect();

            Ok(Peers(addresses))
        }
    }

    impl<'de> Deserialize<'de> for Peers {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_bytes(PeersVisitor)
        }
    }
}

pub fn url_encode(t: &[u8; 20]) -> String {
    let mut encoded = String::with_capacity(3 * t.len());
    for &byte in t {
        encoded.push('%');
        encoded.push_str(&hex::encode(&[byte]));
    }
    encoded
}
