//! Canonical (deterministic) CBOR serialization — spec §5.1.1.
//!
//! Signature verification requires byte-for-byte determinism: the same logical event must
//! always produce the same bytes, on any conforming implementation. We achieve this with
//! RFC 8949 *Core Deterministic Encoding*:
//!
//! 1. **Map keys are sorted** by the bytewise lexicographic order of their encoded form.
//! 2. **Absent optional fields are omitted** entirely (via `skip_serializing_if` on the
//!    types) rather than encoded as `null`.
//! 3. **No floating-point values** appear in the schema — all numbers are integers or
//!    fixed-point (e.g. confidence is a `u16` of basis points).
//!
//! Rust structs serialize (via `ciborium`) to CBOR maps with text-string keys in field
//! declaration order, and `BTreeMap` keys are already ordered — but to be robust against
//! `HashMap`-like sources and to guarantee cross-implementation determinism, we re-sort
//! every map after serialization. The procedure: serialize to a [`ciborium::value::Value`]
//! tree, recursively sort all maps by encoded-key bytes, then write the tree out.

use ciborium::value::Value;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::error::{Error, Result};

/// Serialize `value` to canonical CBOR bytes.
///
/// Implemented with only `into_writer`/`from_reader` (ciborium's most stable surface): encode
/// the value, parse it back into a [`Value`] tree, canonicalize that tree (sort every map by
/// encoded-key bytes), then re-encode.
pub fn to_vec<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>> {
    let mut intermediate = Vec::new();
    ciborium::ser::into_writer(value, &mut intermediate)
        .map_err(|e| Error::Serialization(e.to_string()))?;
    let tree: Value = ciborium::de::from_reader(&intermediate[..])
        .map_err(|e| Error::Serialization(e.to_string()))?;
    let canonical = canonicalize(tree)?;
    let mut out = Vec::new();
    ciborium::ser::into_writer(&canonical, &mut out)
        .map_err(|e| Error::Serialization(e.to_string()))?;
    Ok(out)
}

/// Deserialize a value from CBOR bytes. (Decoding does not require canonical input.)
pub fn from_slice<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    ciborium::de::from_reader(bytes).map_err(|e| Error::Deserialization(e.to_string()))
}

/// Recursively rewrite a CBOR value tree into canonical form: every map is sorted by the
/// encoded bytes of its keys, and children are canonicalized first.
fn canonicalize(value: Value) -> Result<Value> {
    match value {
        Value::Map(entries) => {
            // Canonicalize each child, then sort by the encoded form of the (canonicalized) key.
            let mut canon: Vec<(Vec<u8>, Value, Value)> = Vec::with_capacity(entries.len());
            for (k, v) in entries {
                let ck = canonicalize(k)?;
                let cv = canonicalize(v)?;
                let key_bytes = encode_value(&ck)?;
                canon.push((key_bytes, ck, cv));
            }
            // RFC 8949 §4.2.1: sort by bytewise lexicographic order of the encoded keys.
            canon.sort_by(|a, b| a.0.cmp(&b.0));
            Ok(Value::Map(canon.into_iter().map(|(_, k, v)| (k, v)).collect()))
        }
        Value::Array(items) => {
            // Arrays are positional: preserve order, canonicalize elements.
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(canonicalize(item)?);
            }
            Ok(Value::Array(out))
        }
        Value::Tag(tag, inner) => Ok(Value::Tag(tag, Box::new(canonicalize(*inner)?))),
        other => Ok(other),
    }
}

/// Encode a single CBOR value to bytes (used to derive a sort key for map keys).
fn encode_value(value: &Value) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    ciborium::ser::into_writer(value, &mut buf).map_err(|e| Error::Serialization(e.to_string()))?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn map_key_order_is_deterministic_regardless_of_insertion_order() {
        // Build the same logical map two different ways and confirm identical bytes.
        let mut a: BTreeMap<String, u32> = BTreeMap::new();
        a.insert("zeta".into(), 1);
        a.insert("alpha".into(), 2);
        a.insert("mu".into(), 3);

        let mut b: BTreeMap<String, u32> = BTreeMap::new();
        b.insert("mu".into(), 3);
        b.insert("zeta".into(), 1);
        b.insert("alpha".into(), 2);

        assert_eq!(to_vec(&a).unwrap(), to_vec(&b).unwrap());
    }

    #[test]
    fn round_trips() {
        let mut m: BTreeMap<String, u32> = BTreeMap::new();
        m.insert("a".into(), 1);
        m.insert("b".into(), 2);
        let bytes = to_vec(&m).unwrap();
        let back: BTreeMap<String, u32> = from_slice(&bytes).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn shorter_keys_sort_before_longer_per_rfc8949() {
        // CBOR encodes text-string length in the head byte, so "z" (1 byte payload) encodes
        // to fewer bytes than "aa" (2 bytes) and must sort first under deterministic encoding.
        let mut m: BTreeMap<String, u32> = BTreeMap::new();
        m.insert("aa".into(), 1);
        m.insert("z".into(), 2);
        let bytes = to_vec(&m).unwrap();
        // First map entry key should be "z".
        let tree: Value = ciborium::de::from_reader(&bytes[..]).unwrap();
        if let Value::Map(entries) = tree {
            if let Value::Text(first_key) = &entries[0].0 {
                assert_eq!(first_key, "z");
            } else {
                panic!("expected text key");
            }
        } else {
            panic!("expected map");
        }
    }
}
