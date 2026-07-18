use crate::parser::binary::BinaryReader;
use serde::{Deserialize, Serialize};
use std::io;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AinbBlackboard {
    pub offset: u32,
    pub parameter_type_counts: Vec<u16>,
}

impl AinbBlackboard {
    pub fn read(reader: &mut BinaryReader<'_>, offset: u32) -> io::Result<Self> {
        reader.seek(offset as usize)?;
        let mut parameter_type_counts = Vec::with_capacity(6);
        for _ in 0..6 {
            reader.read_u32()?;
            parameter_type_counts.push(reader.read_u16()?);
            reader.read_u16()?;
        }
        Ok(Self {
            offset,
            parameter_type_counts,
        })
    }
}
