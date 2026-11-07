use std::borrow::Cow;
use std::fmt;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::StoreError;

/// A BLAKE3 digest. Its text form is exactly 64 lower-case hexadecimal characters, and
/// that text is the only thing that ever becomes a path component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ContentHash([u8; 32]);

impl ContentHash {
    /// The hash of `bytes`.
    pub fn of(bytes: &[u8]) -> Self {
        Self(*blake3::hash(bytes).as_bytes())
    }

    pub(crate) fn from_digest(digest: blake3::Hash) -> Self {
        Self(*digest.as_bytes())
    }

    /// Accepts exactly 64 characters from `0-9a-f`; anything else is refused.
    pub fn parse(text: &str) -> Result<Self, StoreError> {
        let bad = || StoreError::BadHash(text.to_owned());
        let bytes = text.as_bytes();
        if bytes.len() != 64 {
            return Err(bad());
        }
        let nibble = |c: u8| match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            _ => None,
        };
        let mut out = [0u8; 32];
        for (byte, pair) in out.iter_mut().zip(bytes.as_chunks::<2>().0) {
            let (hi, lo) = (
                nibble(pair[0]).ok_or_else(bad)?,
                nibble(pair[1]).ok_or_else(bad)?,
            );
            *byte = (hi << 4) | lo;
        }
        Ok(Self(out))
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

impl Serialize for ContentHash {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ContentHash {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for ContentHash {
    fn schema_name() -> Cow<'static, str> {
        "ContentHash".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "pattern": "^[0-9a-f]{64}$",
            "description": "BLAKE3 hash of a stored result, as 64 lower-case hexadecimal characters."
        })
    }
}
