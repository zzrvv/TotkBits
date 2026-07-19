use crate::parser::binary::{BinaryReader, BinaryWriter, Endian};
use std::io;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyleSection(pub Vec<u32>);
impl StyleSection {
    pub fn read(data: &[u8], endian: Endian) -> io::Result<Self> {
        let mut reader = BinaryReader::with_endian(data, endian);
        let mut values = Vec::with_capacity(data.len() / 4);
        while reader.position() < data.len() {
            values.push(reader.read_u32()?);
        }
        Ok(Self(values))
    }
    pub fn write(&self, endian: Endian) -> Vec<u8> {
        let mut writer = BinaryWriter::with_endian(endian);
        for &value in &self.0 {
            writer.write_u32(value);
        }
        writer.into_inner()
    }
}
