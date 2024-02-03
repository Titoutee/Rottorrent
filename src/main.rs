use reqwest;
use serde::{self, Deserialize, Serialize};
use serde_bencode;
use std::env;

mod decode;
mod hash;
use hash::{hashing, Hashes};

#[allow(unused)]
use crate::hash::mul_hashing;

#[derive(Deserialize, Clone, Debug, Serialize)]
struct Torrent {
    //#[serde(with = "serde_bytes")]
    announce: String,
    info: Info,
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

fn main() -> anyhow::Result<(), ()> {
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

        let info_encoded = serde_bencode::to_bytes(&torrent.info).expect("re-encode info section");

        if let Keys::SingleFile { length } = torrent.info.keys {
            println!("Length: {}", length);
        } else {
            unimplemented!();
        }

        println!("Tracker: {}", torrent.announce);
        println!("Info Hash: {}", hashing(&info_encoded));
        println!("Piece Hashes:");
        for hash_piece in torrent.info.pieces.0 {
            println!("{}", hex::encode(hash_piece));
        }
    } else {
        eprintln!("unknown command: {}", args[1]);
    }
    Ok(())
}
