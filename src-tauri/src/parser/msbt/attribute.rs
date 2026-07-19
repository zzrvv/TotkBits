use crate::parser::binary::{BinaryReader, BinaryWriter, Endian};
use std::io::{self, ErrorKind};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttributeSection {
    pub item_size: u32,
    pub records: Vec<Vec<u8>>,
    pub string_pool: Vec<u8>,
}
impl AttributeSection {
    pub fn read(data: &[u8], endian: Endian) -> io::Result<Self> {
        let mut reader = BinaryReader::with_endian(data, endian);
        let count = reader.read_u32()? as usize;
        let item_size = reader.read_u32()?;
        let records_size = count
            .checked_mul(item_size as usize)
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidData, "ATR1 size overflow"))?;
        if reader.position() + records_size > data.len() {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "ATR1 records out of bounds",
            ));
        }
        let mut records = Vec::with_capacity(count);
        for _ in 0..count {
            records.push(reader.read_bytes(item_size as usize)?.to_vec());
        }
        let string_pool = reader.read_bytes(data.len() - reader.position())?.to_vec();
        Ok(Self {
            item_size,
            records,
            string_pool,
        })
    }
    pub fn write(&self, endian: Endian) -> io::Result<Vec<u8>> {
        if self
            .records
            .iter()
            .any(|r| r.len() != self.item_size as usize)
        {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "ATR1 record size mismatch",
            ));
        }
        let mut writer = BinaryWriter::with_endian(endian);
        writer.write_u32(self.records.len() as u32);
        writer.write_u32(self.item_size);
        for record in &self.records {
            writer.write_bytes(record);
        }
        writer.write_bytes(&self.string_pool);
        Ok(writer.into_inner())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttributeOffsets(pub Vec<u32>);
impl AttributeOffsets {
    pub fn read(data: &[u8], endian: Endian) -> io::Result<Self> {
        if data.len() % 4 != 0 {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "ATO1 size is not divisible by four",
            ));
        }
        let mut reader = BinaryReader::with_endian(data, endian);
        let mut values = Vec::with_capacity(data.len() / 4);
        while reader.position() < data.len() {
            values.push(reader.read_u32()?);
        }
        Ok(Self(values))
    }
    pub fn write(&self, endian: Endian) -> Vec<u8> {
        let mut w = BinaryWriter::with_endian(endian);
        for &v in &self.0 {
            w.write_u32(v)
        }
        w.into_inner()
    }
}
