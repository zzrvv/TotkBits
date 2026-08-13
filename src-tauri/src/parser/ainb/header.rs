use crate::parser::binary::BinaryReader;
use serde::{Deserialize, Serialize};
use std::io;

pub const HEADER_SIZE: usize = 0x74;
pub const SUPPORTED_VERSIONS: [u32; 3] = [0x404, 0x407, 0x408];

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AinbHeader {
    pub version: u32,
    pub filename_offset: u32,
    pub command_count: u32,
    pub node_count: u32,
    pub query_count: u32,
    pub attachment_count: u32,
    pub output_count: u32,
    pub blackboard_offset: u32,
    pub string_pool_offset: u32,
    pub enum_resolve_offset: u32,
    pub property_offset: u32,
    pub transition_offset: u32,
    pub io_param_offset: u32,
    pub multi_param_offset: u32,
    pub attachment_offset: u32,
    pub attachment_index_offset: u32,
    pub expression_offset: u32,
    pub replacement_offset: u32,
    pub query_offset: u32,
    pub section_0x50_offset: u32,
    pub section_0x54_value: u32,
    pub section_0x58_offset: u32,
    pub module_offset: u32,
    pub category_name_offset: u32,
    pub category: u32,
    pub action_offset: u32,
    pub section_0x6c_offset: u32,
    pub blackboard_id_offset: u32,
}

impl AinbHeader {
    pub fn read(reader: &mut BinaryReader<'_>) -> io::Result<Self> {
        if reader.read_bytes(4)? != b"AIB " {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid AINB magic",
            ));
        }
        let version = reader.read_u32()?;
        if !SUPPORTED_VERSIONS.contains(&version) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported AINB version {version:#x}"),
            ));
        }
        let mut values = [0u32; 27];
        for value in &mut values {
            *value = reader.read_u32()?;
        }
        Ok(Self {
            version,
            filename_offset: values[0],
            command_count: values[1],
            node_count: values[2],
            query_count: values[3],
            attachment_count: values[4],
            output_count: values[5],
            blackboard_offset: values[6],
            string_pool_offset: values[7],
            enum_resolve_offset: values[8],
            property_offset: values[9],
            transition_offset: values[10],
            io_param_offset: values[11],
            multi_param_offset: values[12],
            attachment_offset: values[13],
            attachment_index_offset: values[14],
            expression_offset: values[15],
            replacement_offset: values[16],
            query_offset: values[17],
            section_0x50_offset: values[18],
            section_0x54_value: values[19],
            section_0x58_offset: values[20],
            module_offset: values[21],
            category_name_offset: values[22],
            category: values[23],
            action_offset: values[24],
            section_0x6c_offset: values[25],
            blackboard_id_offset: values[26],
        })
    }

    pub fn named_offsets(&self) -> [(&'static str, u32); 17] {
        [
            ("blackboard", self.blackboard_offset),
            ("properties", self.property_offset),
            ("transitions", self.transition_offset),
            ("io_parameters", self.io_param_offset),
            ("multi_parameters", self.multi_param_offset),
            ("attachments", self.attachment_offset),
            ("attachment_indices", self.attachment_index_offset),
            ("expressions", self.expression_offset),
            ("replacements", self.replacement_offset),
            ("queries", self.query_offset),
            ("section_0x50", self.section_0x50_offset),
            ("section_0x58", self.section_0x58_offset),
            ("modules", self.module_offset),
            ("actions", self.action_offset),
            ("section_0x6c", self.section_0x6c_offset),
            ("blackboard_ids", self.blackboard_id_offset),
            ("enum_resolve", self.enum_resolve_offset),
        ]
    }
}
