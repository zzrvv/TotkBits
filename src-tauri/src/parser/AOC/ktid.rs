use std::{collections::HashMap, io};

use crate::parser::binary::{BinaryReader, Endian};

#[derive(Debug, Clone, Default)]
pub struct KtidFile {
    entries: HashMap<u32, u32>,
}

impl KtidFile {
    pub fn parse(data: &[u8]) -> io::Result<Self> {
        if data.len() < 8 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "KTID contains no complete entries",
            ));
        }
        let reader = BinaryReader::with_endian(data, Endian::Little);
        let mut entries = HashMap::with_capacity(data.len() / 8);
        // Match the Python importer: consume every complete pair and tolerate
        // an incomplete trailing record.
        for offset in (0..data.len().saturating_sub(7)).step_by(8) {
            entries.insert(reader.read_u32_at(offset)?, reader.read_u32_at(offset + 4)?);
        }
        Ok(Self { entries })
    }

    pub fn get(&self, index: u32) -> Option<u32> {
        self.entries.get(&index).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_complete_pairs_and_ignores_trailing_bytes() {
        let mut data = Vec::new();
        data.extend_from_slice(&3_u32.to_le_bytes());
        data.extend_from_slice(&0xaabb_ccdd_u32.to_le_bytes());
        data.extend_from_slice(&[1, 2, 3]);
        let ktid = KtidFile::parse(&data).unwrap();
        assert_eq!(ktid.get(3), Some(0xaabb_ccdd));
    }
}
