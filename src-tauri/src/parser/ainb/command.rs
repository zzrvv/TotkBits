use crate::parser::binary::BinaryReader;
use serde::{Deserialize, Serialize};
use std::io;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AinbCommand {
    pub name: String,
    pub guid: String,
    pub root_node_index: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondary_root_node_index: Option<u16>,
}

impl AinbCommand {
    pub fn read(reader: &mut BinaryReader<'_>, string_pool_offset: usize) -> io::Result<Self> {
        let name_offset = reader.read_u32()? as usize;
        let a = reader.read_u32()?;
        let b = reader.read_u16()?;
        let c = reader.read_u16()?;
        let d = reader.read_bytes(8)?;
        let guid = format!(
            "{a:08x}-{b:04x}-{c:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]
        );
        let root_node_index = reader.read_u16()?;
        let secondary = reader.read_u16()?;
        Ok(Self {
            name: reader.read_c_string_at(string_pool_offset + name_offset)?,
            guid,
            root_node_index,
            secondary_root_node_index: secondary.checked_sub(1),
        })
    }
}
