use crate::parser::binary::BinaryReader;
use serde::{Deserialize, Serialize};
use std::io;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Trigger {
    pub clip_index: i16,
    #[serde(rename = "Type")]
    pub trigger_type: u8,
}

impl Trigger {
    pub fn read(data: &[u8], offset: u64) -> io::Result<Self> {
        let mut reader = BinaryReader::new(data);
        reader.seek(offset as usize)?;
        Ok(Self {
            clip_index: reader.read_i16()?,
            trigger_type: reader.read_u8()?,
        })
    }
}
