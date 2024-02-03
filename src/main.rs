use serde::de::{self, Visitor};
use serde::{self, Deserialize, Deserializer, Serialize, Serializer};
use serde_bencode;
use sha1::{self, Digest, Sha1};
use std::{env, fmt};

mod decode;

#[derive(Deserialize, Clone, Debug, Serialize)]
struct Torrent {
    //#[serde(with = "serde_bytes")]
    announce: String,
    info: Info,
}

#[derive(Debug, Clone)]
struct Hashes(Vec<[u8; 20]>);
struct HashesVisitor;

impl<'de> Visitor<'de> for HashesVisitor {
    type Value = Hashes;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("an byte string whose length is a multiple of 20")
    }

    fn visit_bytes<E>(self, value: &[u8]) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        if value.len() % 20 != 0 {
            return Err(E::custom(format!("length is {}", value.len())));
        }

        Ok(Hashes(
            value
                .chunks_exact(20)
                .map(|slice| {
                    slice
                        .try_into()
                        .expect("Conversion error from chunk to serde value")
                })
                .collect(),
        ))
    }
}

impl<'de> Deserialize<'de> for Hashes {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_bytes(HashesVisitor)
    }
}

impl Serialize for Hashes {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let single_slice = self.0.concat();
        serializer.serialize_bytes(&single_slice)
    }
}

#[derive(Deserialize, Clone, Debug, Serialize)]
struct Info {
    name: String,

    /// The number of bytes in each piece the file is split into.
    ///
    /// For the purposes of transfer, files are split into fixed-size pieces which are all the same
    /// élength except for possibly the last one which may be truncated. piece length is almost
    /// always a power of two, most commonly 2^18 = 256K (BitTorrent prior to version 3.2 uses 2

    /// 20 = 1 M as default).
    #[serde(rename = "piece length")]
    piece_length: usize,

    /// Each entry of `pieces` is the SHA1 hash of the piece at the corresponding index.
    pieces: Hashes,

    #[serde(flatten)]
    keys: Keys,
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

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{:02x}", byte)).collect()
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

        let mut hasher = Sha1::new();

        hasher.update(&info_encoded);

        let info_hash = hasher.finalize();

        println!("Info Hash: {}", hex::encode(&info_hash));
        if let Keys::SingleFile { length } = torrent.info.keys {
            println!("Length: {}", length);
        } else {
            unimplemented!();
        }
        println!("Tracker: {}", torrent.announce);
    } else {
        eprintln!("unknown command: {}", args[1]);
    }
    Ok(())
}
