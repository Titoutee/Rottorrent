use crate::net::Request;
use anyhow::Context;
use futures_util::{StreamExt, SinkExt};
use reqwest;
use clap::{self, Parser, Subcommand};
use serde_bencode;
use serde_urlencoded;
use serde::{self, Deserialize, Serialize};
use std::{net::{Ipv4Addr, SocketAddrV4}, path::PathBuf, str::FromStr};
use tokio::{self, io::{AsyncReadExt, AsyncWriteExt}};
use tokio::net::TcpStream;
use sha1::{Digest, Sha1};

mod decode;
mod hash;
mod net;
mod message;

use hash::Hashes;
use net::{url_encode, HandShake, TrackerResponse, TrackerSend, PEER_ID};
use message::{Message, MessageTag, MessageFramer};

const BLOCK_MAX: u32 = 1 << 14;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[allow(unused)]
#[derive(Subcommand, Debug)]
enum Command {
    #[command(about = "Decode a bencoded char sequence")]
    Decode { encoded: String },
    #[command(about = "Get info about a torrent file")]
    Info { torrent: PathBuf },
    #[command(about = "Get peers following tracker present in the torrent file")]
    Peers { torrent: PathBuf },
    #[command(about = "Perform handshake with a given torrent file and peer address")]
    Handshake { torrent: PathBuf, peer: String },
    #[command(about = "Perform handshake with a given torrent file and peer address")]
    #[command(rename_all="snake_case")]
    DownloadPiece {
        #[arg(short)]
        output: PathBuf, 
        torrent: PathBuf, 
        piece: u32,
    },
}

#[derive(Deserialize, Clone, Debug, Serialize)]
struct Torrent {
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
    path: Vec<String>, // !!! Not implemented !!!
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let arg = Args::parse();

    match arg.command {
        Command::Decode { encoded } => {
            let value = decode::decode_bencoded_value(&encoded).0;
            println!("{value}");
        }

        Command::Info { torrent } => {
            let content = std::fs::read(torrent).expect("Content reading error");
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
        }

        Command::Peers { torrent } => {
            let content = std::fs::read(torrent).expect("Content reading error");
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

            let tracker_response = reqwest::get(tracker_url).await.expect("Request failed at sending...");
            let tracker_response = tracker_response.bytes().await.context("Tracker response")?;
            let tracker_response: TrackerResponse = serde_bencode::from_bytes(&tracker_response).context("Parse to tracker response")?;
    
            println!("{}", tracker_response.interval);
            for peer in tracker_response.peers.0 {
                println!("{:?}", peer);
            }
        }

        Command::Handshake { torrent, peer } => {
            let content = std::fs::read(torrent).expect("Content reading error");
            let torrent: Torrent = serde_bencode::from_bytes(&content).expect("Deserializing error");
    
            let info_hash = torrent.info_hash();
            let (ip, port): (String, u16) = peer.split_once(':').map(|(x, y)| (String::from(x), str::parse(y).unwrap())).expect("Parsing peer ip and port from argument");
            // Use parse_ascii() when stable
            let peer = SocketAddrV4::new(Ipv4Addr::from_str(&ip).context("Parse ip in socket address")?, port);
            let mut stream = TcpStream::connect(peer).await.context("TCP connection to peer")?;

            let mut handshake = HandShake::new(info_hash, *b"00112233445566778899");
            let handshake_bytes = handshake.as_bytes_mut();

            stream.write_all(handshake_bytes).await.context("writing handshake via TCP to peer")?;
            stream.read_exact(handshake_bytes).await.context("reading handshake")?;
            assert!(handshake.len == 19);
            //assert!(handshake.reserved == [0; 8]);
            assert!(&handshake.bittorrent == b"BitTorrent protocol");
            println!("Peer_id of handshake (hex): {}", hex::encode(handshake.peer_id));
        }

        Command::DownloadPiece { output, torrent, piece } => {
            let content = std::fs::read(torrent).expect("Content reading error");
            let torrent: Torrent = serde_bencode::from_bytes(&content).expect("Deserializing error");
            let length = if let Keys::SingleFile { length } = torrent.info.keys {
                length
            } else {
                todo!();
            };

            let info_hash = torrent.info_hash();
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

            let tracker_response = reqwest::get(tracker_url).await.expect("Request failed at sending...");
            let tracker_response = tracker_response.bytes().await.context("Tracker response")?;
            let tracker_response: TrackerResponse = serde_bencode::from_bytes(&tracker_response).context("Parse to tracker response")?;

            let peer = &tracker_response.peers.0[0];
            let mut peer = TcpStream::connect(peer).await.context("TCP connection to peer")?;

            let mut handshake = HandShake::new(info_hash, *b"00112233445566778899");
            let handshake_bytes = handshake.as_bytes_mut();

            peer.write_all(handshake_bytes).await.context("writing handshake via TCP to peer")?;
            peer.read_exact(handshake_bytes).await.context("reading handshake")?;
            assert!(handshake.len == 19);
            //assert!(handshake.reserved == [0; 8]);
            assert!(&handshake.bittorrent == b"BitTorrent protocol");
            println!("Peer_id of handshake (hex): {}", hex::encode(handshake.peer_id));

            let mut peer = tokio_util::codec::Framed::new(peer, MessageFramer); // Peer is framed

            // #1: Wait for bitfield from peer(s)
            let bitfield = peer.next().await.expect("Peer always first sends a bitfield").expect("Bitfield was invalid");
            assert_eq!(bitfield.tag, MessageTag::Bitfield);

            // #2: Send Interested
            peer.send(Message {
                length: 1,
                tag: MessageTag::Interested,
                payload: Vec::new()
            }).await.context("Send Interested")?;

            // #3: Wait for unchoke from peer(s)
            let unchoke = peer.next().await.expect("Peer always first sends a bitfield").expect("Bitfield was invalid");
            assert_eq!(unchoke.tag, MessageTag::Unchoke);
            assert!(unchoke.payload.is_empty()); // Should be the case if pervious assertions were passed according to the protocol

            // #4: Send Request for all blocks of a file piece
            let piece_hash = torrent.info.pieces.at(piece as usize).context("Access piece hash of corresponding piece")?;
            let piece_size = if piece as usize == torrent.info.pieces.0.len() - 1 {
                length % torrent.info.piece_length
            } else {
                torrent.info.piece_length
            };

            let nblocks = (piece_size + (BLOCK_MAX - 1)) / BLOCK_MAX;
            for block_i in 0..nblocks {
                let block_size = if block_i == nblocks - 1 {
                    piece_size % BLOCK_MAX
                } else {
                    BLOCK_MAX
                };
                let request = Request::new(piece, block_i as u32 * BLOCK_MAX, block_size as u32);
            }
            

        }
    }
    
    Ok(())
}
