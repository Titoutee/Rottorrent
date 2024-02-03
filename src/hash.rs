use serde::de::{self, Visitor};
use serde::{self, Deserialize, Deserializer, Serialize, Serializer};
use sha1::{Digest, Sha1};
use std::fmt;

#[derive(Debug, Clone)]
pub struct Hashes(pub Vec<[u8; 20]>);
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
            return Err(E::custom(format!(
                "length is {} but a multiple of 20 is expected",
                value.len()
            )));
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

pub fn hashing(bytes: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(bytes);
    hex::encode(&hasher.finalize())
}

pub fn mul_hashing(bytes_mul: Vec<&[u8]>) -> String {
    let mut hasher = Sha1::new();
    let mut result = String::new();
    for bytes in bytes_mul {
        hasher.update(bytes);
        result.push_str(&format!("{}\n", &hex::encode(&hasher.clone().finalize())));
    }
    result
}