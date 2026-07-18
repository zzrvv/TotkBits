use super::{
    container::{read_container, Container},
    radix_tree::read_string_ptr,
};
use crate::parser::binary::BinaryReader;
use serde::{Deserialize, Serialize};
use std::io;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Cut {
    pub start_time: f32,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Container>,
}

impl Cut {
    pub fn read(data: &[u8], offset: u64) -> io::Result<Self> {
        let mut reader = BinaryReader::new(data);
        reader.seek(offset as usize)?;
        let start_time = reader.read_f32()?;
        reader.skip(4)?;
        let name = read_string_ptr(&mut reader, data)?;
        let parameters_offset = reader.read_u64()?;
        let parameters = (parameters_offset != 0)
            .then(|| read_container(data, parameters_offset))
            .transpose()?;
        Ok(Self {
            start_time,
            name,
            parameters,
        })
    }
}
