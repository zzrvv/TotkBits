use crate::parser::binary::BinaryWriter;
use std::{collections::HashMap, io};

#[derive(Default)]
pub struct AinbWriter {
    binary: BinaryWriter,
    strings: Vec<String>,
    string_offsets: HashMap<String, u32>,
    string_pool_len: u32,
}

impl AinbWriter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_strings(strings: &[String]) -> Self {
        let mut writer = Self::new();
        for string in strings {
            writer.add_string(string);
        }
        writer
    }

    pub fn position(&self) -> usize {
        self.binary.position()
    }

    pub fn into_inner(self) -> Vec<u8> {
        self.binary.into_inner()
    }

    pub fn write_bytes(&mut self, value: &[u8]) {
        self.binary.write_bytes(value);
    }

    pub fn truncate(&mut self, position: usize) {
        self.binary.truncate(position);
    }

    pub fn write_u8(&mut self, value: u8) {
        self.binary.write_u8(value);
    }

    pub fn write_u16(&mut self, value: u16) {
        self.binary.write_u16(value);
    }

    pub fn write_i16(&mut self, value: i16) {
        self.binary.write_i16(value);
    }

    pub fn write_u32(&mut self, value: u32) {
        self.binary.write_u32(value);
    }

    pub fn write_i32(&mut self, value: i32) {
        self.binary.write_i32(value);
    }

    pub fn write_f32(&mut self, value: f32) {
        self.binary.write_f32(value);
    }

    pub fn write_vec3(&mut self, value: [f32; 3]) {
        for component in value {
            self.write_f32(component);
        }
    }

    pub fn add_string(&mut self, value: &str) -> u32 {
        if let Some(offset) = self.string_offsets.get(value) {
            return *offset;
        }
        let offset = self.string_pool_len;
        self.string_pool_len += value.len() as u32 + 1;
        self.string_offsets.insert(value.to_owned(), offset);
        self.strings.push(value.to_owned());
        offset
    }

    pub fn write_string_offset(&mut self, value: &str) {
        let offset = self.add_string(value);
        self.write_u32(offset);
    }

    pub fn string_offset(&self, value: &str) -> io::Result<u32> {
        self.string_offsets.get(value).copied().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("string was not added to pool: {value}"),
            )
        })
    }

    pub fn write_string_pool(&mut self) {
        for value in std::mem::take(&mut self.strings) {
            self.binary.write_c_string(&value);
        }
    }

    pub fn write_guid(&mut self, value: &str) -> io::Result<()> {
        let mut groups = value.split('-');
        let a = parse_hex::<u32>(groups.next(), "GUID field 1")?;
        let b = parse_hex::<u16>(groups.next(), "GUID field 2")?;
        let c = parse_hex::<u16>(groups.next(), "GUID field 3")?;
        let d = groups.next().ok_or_else(|| invalid_guid(value))?;
        let e = groups.next().ok_or_else(|| invalid_guid(value))?;
        if groups.next().is_some() || d.len() != 4 || e.len() != 12 {
            return Err(invalid_guid(value));
        }
        self.write_u32(a);
        self.write_u16(b);
        self.write_u16(c);
        for pair in d
            .as_bytes()
            .chunks_exact(2)
            .chain(e.as_bytes().chunks_exact(2))
        {
            let pair = std::str::from_utf8(pair).map_err(|_| invalid_guid(value))?;
            self.write_u8(u8::from_str_radix(pair, 16).map_err(|_| invalid_guid(value))?);
        }
        Ok(())
    }
}

fn parse_hex<T>(value: Option<&str>, label: &str) -> io::Result<T>
where
    T: TryFrom<u64>,
{
    let value = value.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, label))?;
    let parsed = u64::from_str_radix(value, 16)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, label))?;
    T::try_from(parsed).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, label))
}

fn invalid_guid(value: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("invalid AINB GUID {value}"),
    )
}

pub fn murmur3_32(value: &str) -> u32 {
    let bytes = value.as_bytes();
    let mut hash = 0u32;
    let mut chunks = bytes.chunks_exact(4);
    for chunk in &mut chunks {
        let mut k = u32::from_le_bytes(chunk.try_into().expect("four-byte chunk"));
        k = k.wrapping_mul(0xcc9e_2d51);
        k = k.rotate_left(15);
        k = k.wrapping_mul(0x1b87_3593);
        hash ^= k;
        hash = hash.rotate_left(13);
        hash = hash.wrapping_mul(5).wrapping_add(0xe654_6b64);
    }
    let tail = chunks.remainder();
    let mut k = 0u32;
    if tail.len() >= 3 {
        k ^= (tail[2] as u32) << 16;
    }
    if tail.len() >= 2 {
        k ^= (tail[1] as u32) << 8;
    }
    if let Some(first) = tail.first() {
        k ^= *first as u32;
        k = k.wrapping_mul(0xcc9e_2d51);
        k = k.rotate_left(15);
        k = k.wrapping_mul(0x1b87_3593);
        hash ^= k;
    }
    hash ^= bytes.len() as u32;
    hash ^= hash >> 16;
    hash = hash.wrapping_mul(0x85eb_ca6b);
    hash ^= hash >> 13;
    hash = hash.wrapping_mul(0xc2b2_ae35);
    hash ^ (hash >> 16)
}

#[cfg(test)]
mod tests {
    use super::murmur3_32;

    #[test]
    fn murmur3_matches_known_vectors() {
        assert_eq!(murmur3_32(""), 0);
        assert_eq!(murmur3_32("foo"), 0xf6a5_c420);
        assert_eq!(murmur3_32("hello"), 0x248b_fa47);
    }
}
