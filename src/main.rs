use anyhow::Context;
use reqwest;
use serde::{self, Deserialize, Serialize};
use serde_bencode;
use std::{env, net::{Ipv4Addr, SocketAddrV4}, str::FromStr};
use tokio::{self, io::{AsyncReadExt, AsyncWriteExt}};
use serde_urlencoded;
use tokio::net::{TcpListener, TcpStream};

mod decode;
mod hash;
mod net;

use hash::Hashes;
use net::{peers::Peers, url_encode, HandShake, TrackerResponse, TrackerSend, PEER_ID};
use sha1::{Digest, Sha1};

#[derive(Deserialize, Clone, Debug, Serialize)]
struct Torrent {
    //#[serde(with = "serde_bytes")]
    announce: String,
    info: Info,
}

impl Torrent {
    /// Get the info hash as 20 bytes.
    pub fn info_hash(&self) -> [u8; 20] {
        let info_encoded = serde_bencode::to_bytes(&self.info).expect("re-encode info section");
        let mut hasher = Sha1::new();
        hasher.update(&info_encoded);
        hasher
            .finalize()
            .try_into()
            .expect("Supposed to be a GenericArray cast-able to [u8; 20]")
    }
}

#[derive(Deserialize, Clone, Debug, Serialize)]
struct Info {
    name: String,

    /// The number of bytes in each piece the file is split into.
    ///
    /// For the purposes of transfer, files are split into fixed-size pieces which are all the same
    /// length except for possibly the last one which may be truncated. piece length is almost
    /// always a power of two, most commonly 2^18 = 256K (BitTorrent prior to version 3.2 uses 2

    /// 20 = 1 M as default).
    #[serde(rename = "piece length")]
    piece_length: usize,

    /// Each entry of `pieces` is the SHA1 hash of the piece at the corresponding index.
    pieces: Hashes,

    #[serde(flatten)]
    keys: Keys,
}

#[allow(unused)]
impl Info {
    #[allow(unused)]
    fn hashes(&self) -> &Vec<[u8; 20]> {
        &self.pieces.0
    }
    fn hashes_refs(&self) -> Vec<&[u8]> {
        self.pieces.0.iter().map(|arr| arr.as_ref()).collect()
    }
}

#[derive(Deserialize, Clone, Debug, Serialize)]
#[serde(untagged)]
enum Keys {
    SingleFile { length: usize },
    MultiFile { file: File },
}

#[derive(Deserialize, Clone, Debug, Serialize)]
struct File {
    length: usize,
    path: Vec<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();
    let command = &args[1];

    if command == "decode" {
        let encoded = &args[2];
        let value = decode::decode_bencoded_value(&encoded).0;
        println!("{value}");
    } else if command == "info" {
        let path = args[2].as_str();
        let content = std::fs::read(path).expect("Content reading error");
        let torrent: Torrent = serde_bencode::from_bytes(&content).expect("Deserializing error");

        if let Keys::SingleFile { length } = torrent.info.keys {
            println!("Length: {}", length);
        } else {
            unimplemented!();
        }

        println!("Tracker: {}", torrent.announce);
        println!("Piece Hashes:");
        for hash_piece in torrent.info.pieces.0 {
            println!("{}", hex::encode(hash_piece));
        }
    } else if command == "peers" {
        let path = args[2].as_str();
        let content = std::fs::read(path).expect("Content reading error");
        let torrent: Torrent = serde_bencode::from_bytes(&content).expect("Deserializing error");
        let length = if let Keys::SingleFile { length } = torrent.info.keys {
            length
        } else {
            todo!();
        };

        let tracker_send = TrackerSend {
            peer_id: String::from(PEER_ID),
            port: 6881,
            downloaded: 0,
            uploaded: 0,
            left: length,
            compact: 1,
        };
        
        let request_params_url = serde_urlencoded::to_string(&tracker_send).context("Url-encode the tracker params")?;
        let tracker_url = format!("{}?{}&info_hash={}", torrent.announce, request_params_url, &url_encode(&torrent.info_hash()));
        //tracker_url.set_query(Some(&request_params_url));
        //tracker_url.query_pairs_mut().append_pair("info_hash", &url_encode(&torrent.info_hash()));
        //eprintln!("{}", tracker_url);
        let tracker_response = reqwest::get(tracker_url).await.expect("Request failed at sending...");
        let tracker_response = tracker_response.bytes().await.context("Tracker response")?;
        let tracker_response: TrackerResponse = serde_bencode::from_bytes(&tracker_response).context("Parse to tracker response")?;

        println!("{}", tracker_response.interval);
        for peer in tracker_response.peers.0 {
            println!("{:?}", peer);
        }

    } else if command == "handshake" {
        let path = args[2].as_str();
        let peer_address = args[3].as_str();
        let content = std::fs::read(path).expect("Content reading error");
        let torrent: Torrent = serde_bencode::from_bytes(&content).expect("Deserializing error");

        let info_hash = torrent.info_hash();
        let (ip, port): (String, u16) = peer_address.split_once(':').map(|(x, y)| (String::from(x), str::parse(y).unwrap())).expect("Parsing peer ip and port from argument");
        // Use parse_ascii() when stable
        let peer = SocketAddrV4::new(Ipv4Addr::from_str(&ip).context("Parse ip in socket address")?, port);
        let mut stream = TcpStream::connect(peer).await.context("TCP connection to peer")?;
        let mut handshake = HandShake::new(info_hash, *b"00112233445566778899");
        let handshake_bytes = &mut handshake as *mut HandShake as *mut [u8; std::mem::size_of::<HandShake>()];
        let handshake_bytes: &mut [u8; std::mem::size_of::<HandShake>()] = unsafe {
            &mut *handshake_bytes
        }; // Not a raw pointer anymore
        // Future: remove this unsafe block
        stream.write_all(handshake_bytes).await.context("writing handshake via TCP to peer")?;
        stream.read_exact(handshake_bytes).await.context("reading handshake")?;
        assert!(handshake.len == 19);
        //assert!(handshake.reserved == [0; 8]);
        assert!(&handshake.bittorrent == b"BitTorrent protocol");
        println!("Peer_id of handshake (hex): {}", hex::encode(handshake.peer_id));

    } else {
        eprintln!("unknown command: {}", args[1]);
    }
    Ok(())
}
