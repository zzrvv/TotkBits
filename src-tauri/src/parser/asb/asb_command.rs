use super::parameter::{read_parameter, ParameterType};
use crate::parser::binary::BinaryReader;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::io;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AsbCommand {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Tags", skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(rename = "Unknown 1")]
    pub unknown_1: Value,
    #[serde(rename = "Unknown 2")]
    pub unknown_2: Value,
    #[serde(rename = "Unknown 3")]
    pub unknown_3: u32,
    #[serde(rename = "GUID")]
    pub guid: String,
    #[serde(rename = "Left Node Index")]
    pub left_node_index: u16,
    #[serde(rename = "Right Node Index")]
    pub right_node_index: i32,
}

impl AsbCommand {
    pub fn read(
        reader: &mut BinaryReader<'_>,
        pool: &BinaryReader<'_>,
        version: u32,
    ) -> io::Result<Self> {
        let name = pool.read_c_string_at(reader.read_u32()? as usize)?;
        let tags = if version == 0x417 {
            let offset = reader.read_u32()? as usize;
            if offset != 0 {
                let pos = reader.position();
                reader.seek(offset)?;
                let count = reader.read_u32()?;
                let mut tags = Vec::new();
                for _ in 0..count {
                    tags.push(pool.read_c_string_at(reader.read_u32()? as usize)?);
                }
                reader.seek(pos)?;
                Some(tags)
            } else {
                None
            }
        } else {
            None
        };
        let unknown_1 = read_parameter(reader, pool, ParameterType::Float)?;
        let unknown_2 = read_parameter(reader, pool, ParameterType::Int)?;
        let unknown_3 = reader.read_u32()?;
        let guid = format!(
            "{:x}-{:x}-{:x}-{:x}-{}",
            reader.read_u32()?,
            reader.read_u16()?,
            reader.read_u16()?,
            reader.read_u16()?,
            hex_bytes(reader.read_bytes(6)?)
        );
        Ok(Self {
            name,
            tags,
            unknown_1,
            unknown_2,
            unknown_3,
            guid,
            left_node_index: reader.read_u16()?,
            right_node_index: i32::from(reader.read_u16()?) - 1,
        })
    }
}
fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
