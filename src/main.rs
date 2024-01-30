use serde::{self, Deserialize, Deserializer};
use serde_bencode;
use std::env;
use std::fmt;
use serde::de::{self, Visitor};

#[derive(Deserialize, Clone, Debug)]
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

        let values = Vec::with_capacity(value.len()/20);
        Ok(Hashes(values.chunks_exact(20).map(|slice| {
            slice.try_into().expect("Conversion error from chunk to serde value")
        }).collect()))
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

#[derive(Deserialize, Clone, Debug)]
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

#[derive(Deserialize, Clone, Debug)]
#[serde(untagged)]
enum Keys {
    SingleFile { length: usize },
    MultiFile { file: File}
}

#[derive(Deserialize, Clone, Debug)]
struct File {
    length: usize,

    path: Vec<String>,
}

fn decode_bencoded_value(encoded_value: &str) -> (serde_json::Value, &str) {
    match encoded_value.chars().next() {
        Some('i') => {
            if let Some((n, rest)) =
                encoded_value
                    .split_at(1)
                    .1
                    .split_once('e')
                    .and_then(|(digits, rest)| {
                        let n = digits.parse::<i64>().ok()?;

                        Some((n, rest))
                    })
            {
                return (n.into(), rest);
            }
        }

        Some('l') => {
            let mut values = Vec::new();

            let mut rest = encoded_value.split_at(1).1;

            while !rest.is_empty() && !rest.starts_with('e') {
                let (v, remainder) = decode_bencoded_value(rest);

                values.push(v);

                rest = remainder;
            }

            return (values.into(), &rest[1..]);
        }

        Some('d') => {
            let mut dict = serde_json::Map::new();

            let mut rest = encoded_value.split_at(1).1;

            while !rest.is_empty() && !rest.starts_with('e') {
                let (k, remainder) = decode_bencoded_value(rest);

                let k = match k {
                    serde_json::Value::String(k) => k,

                    k => {
                        panic!("dict keys must be strings, not {k:?}");
                    }
                };

                let (v, remainder) = decode_bencoded_value(remainder);

                dict.insert(k, v);

                rest = remainder;
            }

            return (dict.into(), &rest[1..]);
        }

        Some('0'..='9') => {
            if let Some((len, rest)) = encoded_value.split_once(':') {
                if let Ok(len) = len.parse::<usize>() {
                    return (rest[..len].to_string().into(), &rest[len..]);
                }
            }
        }

        _ => {}
    }

    panic!("Unhandled encoded value: {}", encoded_value);
}


fn main() -> anyhow::Result<(), ()> {
    let args: Vec<String> = env::args().collect();
    let command = &args[1];

    if command == "decode" {
        let encoded = &args[2];
        let value = decode_bencoded_value(encoded).0;
        println!("{value}");
    } else if command == "info" {
        let path = args[2].as_str();
        let content = std::fs::read(path).expect("Content reading error");
        let torrent: Torrent = serde_bencode::from_bytes(&content).expect("Deserializing error");
        println!("Tracker URL: {}", torrent.announce);
        if let Keys::SingleFile { length } = torrent.info.keys {
            println!("Length: {}", length);
        } else {
            unimplemented!();
        }
    } else {
        eprintln!("unknown command: {}", args[1]);
    }
    Ok(())
}
