use super::{
    container::{read_container, Container},
    radix_tree::{read_offset_array, read_string, read_string_ptr},
};
use crate::parser::binary::BinaryReader;
use serde::{Deserialize, Serialize};
use std::io;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Actor {
    pub name: String,
    pub secondary_name: String,
    pub argument_name: String,
    pub entry_point_index: i16,
    pub cut_number: u8,
    pub actions: Vec<String>,
    pub queries: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Container>,
}

impl Actor {
    pub fn read(data: &[u8], offset: u64) -> io::Result<Self> {
        let mut reader = BinaryReader::new(data);
        reader.seek(offset as usize)?;
        let name = read_string_ptr(&mut reader, data)?;
        let secondary_name = read_string_ptr(&mut reader, data)?;
        let argument_name = read_string_ptr(&mut reader, data)?;
        let actions_offset = reader.read_u64()?;
        let queries_offset = reader.read_u64()?;
        let parameters_offset = reader.read_u64()?;
        let action_count = reader.read_u16()? as usize;
        let query_count = reader.read_u16()? as usize;
        let entry_point_index = reader.read_i16()?;
        let cut_number = reader.read_u8()?;
        let actions = read_offset_array(data, actions_offset, action_count)?
            .into_iter()
            .map(|offset| read_string(data, offset))
            .collect::<io::Result<_>>()?;
        let queries = read_offset_array(data, queries_offset, query_count)?
            .into_iter()
            .map(|offset| read_string(data, offset))
            .collect::<io::Result<_>>()?;
        let parameters = if parameters_offset == 0 {
            None
        } else {
            Some(read_container(data, parameters_offset)?)
        };
        Ok(Self {
            name,
            secondary_name,
            argument_name,
            entry_point_index,
            cut_number,
            actions,
            queries,
            parameters,
        })
    }
}
