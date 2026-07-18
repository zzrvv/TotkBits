use crate::parser::binary::BinaryReader;
use std::io;

#[derive(Clone, Copy, Debug)]
pub struct BaevArray {
    pub offset: u64,
    pub count: u32,
    pub element_size: u32,
}

impl BaevArray {
    pub fn read(reader: &mut BinaryReader<'_>) -> io::Result<Self> {
        Ok(Self {
            offset: reader.read_u64()?,
            count: reader.read_u32()?,
            element_size: reader.read_u32()?,
        })
    }

    pub fn offset(&self) -> io::Result<usize> {
        usize::try_from(self.offset)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "BAEV offset exceeds usize"))
    }
}
