use super::container::{read_container, Container};
use crate::parser::binary::BinaryReader;
use serde::{Deserialize, Serialize};
use std::io;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Clip {
    pub start_time: f32,
    pub duration: f32,
    pub actor_index: i16,
    pub actor_action_index: i16,
    pub unknown: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Container>,
}

impl Clip {
    pub fn read(data: &[u8], offset: u64) -> io::Result<Self> {
        let mut reader = BinaryReader::new(data);
        reader.seek(offset as usize)?;
        let start_time = reader.read_f32()?;
        let duration = reader.read_f32()?;
        let actor_index = reader.read_i16()?;
        let actor_action_index = reader.read_i16()?;
        let unknown = reader.read_u8()?;
        reader.skip(3)?;
        let parameters_offset = reader.read_u64()?;
        let parameters = (parameters_offset != 0)
            .then(|| read_container(data, parameters_offset))
            .transpose()?;
        Ok(Self {
            start_time,
            duration,
            actor_index,
            actor_action_index,
            unknown,
            parameters,
        })
    }
}
