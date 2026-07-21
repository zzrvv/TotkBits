use super::{
    crc32::crc32, dynamic_header::DynamicHeader, error::RstbError, fixed_header::FixedHeader,
    header::Header, version::RstbVersion,
};
use crate::parser::binary::{BinaryReader, BinaryWriter, Endian};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct ResourceSizeTable {
    pub version: RstbVersion,
    pub endian: Endian,
    pub key_size: usize,
    pub hash_table: BTreeMap<u32, u32>,
    pub overflow_table: BTreeMap<String, u32>,
    hash_order: Vec<u32>,
    overflow_order: Vec<String>,
}
impl ResourceSizeTable {
    pub fn from_bytes(data: &[u8]) -> Result<Self, RstbError> {
        if !(data.starts_with(b"RSTB") || data.starts_with(b"RESTBL")) {
            return Err(RstbError::InvalidMagic);
        }
        let mut errors = None;
        for endian in [Endian::Little, Endian::Big] {
            match Self::parse(data, endian) {
                Ok(v) => return Ok(v),
                Err(e) => errors = Some(e),
            }
        }
        Err(errors.unwrap_or(RstbError::InvalidMagic))
    }
    fn parse(data: &[u8], endian: Endian) -> Result<Self, RstbError> {
        let dynamic = data.starts_with(b"RESTBL");
        let mut r = BinaryReader::with_endian(data, endian);
        let header = if dynamic {
            r.read_bytes(6)?;
            let version = r.read_u32()?;
            if version != 1 {
                return Err(RstbError::UnsupportedVersion(version));
            }
            let key_size = r.read_u32()?;
            let hash_count = r.read_u32()?;
            let overflow_count = r.read_u32()?;
            if key_size == 0 && overflow_count != 0 {
                return Err(RstbError::InvalidKeySize(key_size));
            }
            Header::Dynamic(DynamicHeader {
                version,
                key_size,
                hash_count,
                overflow_count,
            })
        } else {
            r.read_bytes(4)?;
            Header::Fixed(FixedHeader {
                hash_count: r.read_u32()?,
                overflow_count: r.read_u32()?,
            })
        };
        let (hc, oc) = header.counts();
        let expected = header
            .size()
            .checked_add(
                (hc as usize)
                    .checked_mul(8)
                    .ok_or(RstbError::Overflow("hash table"))?,
            )
            .and_then(|v| v.checked_add((oc as usize).checked_mul(header.key_size() + 4)?))
            .ok_or(RstbError::Overflow("file size"))?;
        if expected != data.len() {
            return Err(RstbError::InvalidLength {
                expected,
                actual: data.len(),
            });
        }
        let mut hash_table = BTreeMap::new();
        let mut hash_order = Vec::with_capacity(hc as usize);
        for _ in 0..hc {
            let hash = r.read_u32()?;
            hash_table.insert(hash, r.read_u32()?);
            hash_order.push(hash);
        }
        let mut overflow_table = BTreeMap::new();
        let mut overflow_order = Vec::with_capacity(oc as usize);
        for _ in 0..oc {
            let raw = r.read_bytes(header.key_size())?;
            let end = raw.iter().position(|v| *v == 0).unwrap_or(raw.len());
            let key = std::str::from_utf8(&raw[..end])
                .map_err(|e| RstbError::InvalidUtf8(e.to_string()))?
                .to_owned();
            overflow_table.insert(key.clone(), r.read_u32()?);
            overflow_order.push(key);
        }
        Ok(Self {
            version: if dynamic {
                RstbVersion::Dynamic(1)
            } else {
                RstbVersion::Fixed
            },
            endian,
            key_size: header.key_size(),
            hash_table,
            overflow_table,
            hash_order,
            overflow_order,
        })
    }
    pub fn to_bytes(&self) -> Result<Vec<u8>, RstbError> {
        let key_size = match self.version {
            RstbVersion::Fixed => {
                if self.overflow_table.keys().any(|k| k.as_bytes().len() > 128) {
                    return Err(RstbError::InvalidKeySize(128));
                }
                128
            }
            RstbVersion::Dynamic(_) => {
                let n = self
                    .overflow_table
                    .keys()
                    .map(|k| k.as_bytes().len() + 1)
                    .max()
                    .unwrap_or(0);
                self.key_size.max(if n == 0 { 0 } else { (n + 1) & !1 })
            }
        };
        let mut w = BinaryWriter::with_endian(self.endian);
        match self.version {
            RstbVersion::Fixed => w.write_bytes(b"RSTB"),
            RstbVersion::Dynamic(v) => {
                w.write_bytes(b"RESTBL");
                w.write_u32(v);
                w.write_u32(key_size as u32)
            }
        }
        w.write_u32(self.hash_table.len() as u32);
        w.write_u32(self.overflow_table.len() as u32);
        for h in &self.hash_order {
            let v = self.hash_table[h];
            w.write_u32(*h);
            w.write_u32(v)
        }
        for k in &self.overflow_order {
            let v = self.overflow_table[k];
            w.write_bytes(k.as_bytes());
            w.write_bytes(&vec![0; key_size - k.len()]);
            w.write_u32(v)
        }
        Ok(w.into_inner())
    }
    pub fn get(&self, key: String) -> Option<&u32> {
        self.overflow_table
            .get(&key)
            .or_else(|| self.hash_table.get(&crc32(key)))
    }
    pub fn set(&mut self, key: String, value: u32) {
        if self.overflow_table.contains_key(&key) {
            self.overflow_table.insert(key, value);
        } else {
            let hash = crc32(&key);
            if !self.hash_table.contains_key(&hash) {
                self.hash_order.push(hash);
            }
            self.hash_table.insert(hash, value);
        }
    }
    pub fn remove(&mut self, key: String) {
        if self.overflow_table.remove(&key).is_none() {
            let hash = crc32(key);
            self.hash_table.remove(&hash);
            self.hash_order.retain(|v| *v != hash);
        } else {
            self.overflow_order.retain(|v| v != &key);
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = (&u32, &u32)> {
        self.hash_table.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dynamic_roundtrip() {
        let mut t = ResourceSizeTable {
            version: RstbVersion::Dynamic(1),
            endian: Endian::Little,
            key_size: 0,
            hash_table: BTreeMap::new(),
            overflow_table: BTreeMap::new(),
            hash_order: Vec::new(),
            overflow_order: Vec::new(),
        };
        t.set("A/B".into(), 7);
        let b = t.to_bytes().unwrap();
        let p = ResourceSizeTable::from_bytes(&b).unwrap();
        assert_eq!(p.to_bytes().unwrap(), b)
    }
    #[test]
    fn truncated_rejected() {
        assert!(ResourceSizeTable::from_bytes(b"RSTB").is_err())
    }
}
