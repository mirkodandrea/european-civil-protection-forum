//! Grid arrays as base64 of little-endian bytes: about a third of the size of
//! a JSON number list, and cheap to decode in a browser too.

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{de::Error, Deserialize, Deserializer, Serializer};

fn decode<'de, D: Deserializer<'de>, const N: usize>(d: D) -> Result<Vec<[u8; N]>, D::Error> {
    let text = String::deserialize(d)?;
    let bytes = STANDARD.decode(text).map_err(D::Error::custom)?;
    if bytes.len() % N != 0 {
        return Err(D::Error::custom("packed array has a partial element"));
    }
    Ok(bytes
        .chunks_exact(N)
        .map(|c| c.try_into().expect("chunk size"))
        .collect())
}

pub mod bytes {
    use super::*;
    pub fn serialize<S: Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(v))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        Ok(decode::<D, 1>(d)?.into_iter().map(|b| b[0]).collect())
    }
}

pub mod f32s {
    use super::*;
    pub fn serialize<S: Serializer>(v: &[f32], s: S) -> Result<S::Ok, S::Error> {
        let raw: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
        s.serialize_str(&STANDARD.encode(raw))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<f32>, D::Error> {
        Ok(decode::<D, 4>(d)?.into_iter().map(f32::from_le_bytes).collect())
    }
}

pub mod i32s {
    use super::*;
    pub fn serialize<S: Serializer>(v: &[i32], s: S) -> Result<S::Ok, S::Error> {
        let raw: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
        s.serialize_str(&STANDARD.encode(raw))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<i32>, D::Error> {
        Ok(decode::<D, 4>(d)?.into_iter().map(i32::from_le_bytes).collect())
    }
}
