use crate::parser::binary::BinaryReader;
use std::io::{self, ErrorKind};

#[derive(Clone, Debug, Default)]
pub struct AsbHeader {
    pub version: u32,
    pub filename_offset: u32,
    pub command_count: u32,
    pub node_count: u32,
    pub event_count: u32,
    pub slot_count: u32,
    pub x38_count: u32,
    pub local_blackboard_offset: u32,
    pub string_pool_offset: u32,
    pub enum_resolve_array_offset: u32,
    pub x2c_offset: u32,
    pub event_offsets_offset: u32,
    pub slots_offset: u32,
    pub x38_offset: u32,
    pub x38_index_offset: u32,
    pub x40_offset: u32,
    pub x40_count: u32,
    pub bone_group_offset: u32,
    pub bone_group_count: u32,
    pub string_pool_size: u32,
    pub transitions_offset: u32,
    pub tag_list_offset: u32,
    pub as_markings_offset: u32,
    pub exb_offset: u32,
    pub command_groups_offset: u32,
    pub x68_offset: Option<u32>,
}

impl AsbHeader {
    pub const VERSION_417: u32 = 0x417;
    pub const VERSION_40F: u32 = 0x40f;

    pub fn read(reader: &mut BinaryReader<'_>) -> io::Result<Self> {
        if reader.read_bytes(4)? != b"ASB " {
            return Err(io::Error::new(ErrorKind::InvalidData, "invalid ASB magic"));
        }
        let version = reader.read_u32()?;
        if !matches!(version, Self::VERSION_417 | Self::VERSION_40F) {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                format!("unsupported ASB version {version:#x}"),
            ));
        }
        let result = Self {
            version,
            filename_offset: reader.read_u32()?,
            command_count: reader.read_u32()?,
            node_count: reader.read_u32()?,
            event_count: reader.read_u32()?,
            slot_count: reader.read_u32()?,
            x38_count: reader.read_u32()?,
            local_blackboard_offset: reader.read_u32()?,
            string_pool_offset: reader.read_u32()?,
            enum_resolve_array_offset: reader.read_u32()?,
            x2c_offset: reader.read_u32()?,
            event_offsets_offset: reader.read_u32()?,
            slots_offset: reader.read_u32()?,
            x38_offset: reader.read_u32()?,
            x38_index_offset: reader.read_u32()?,
            x40_offset: reader.read_u32()?,
            x40_count: reader.read_u32()?,
            bone_group_offset: reader.read_u32()?,
            bone_group_count: reader.read_u32()?,
            string_pool_size: reader.read_u32()?,
            transitions_offset: reader.read_u32()?,
            tag_list_offset: reader.read_u32()?,
            as_markings_offset: reader.read_u32()?,
            exb_offset: reader.read_u32()?,
            command_groups_offset: reader.read_u32()?,
            x68_offset: if version == Self::VERSION_417 {
                Some(reader.read_u32()?)
            } else {
                None
            },
        };
        let expected = if version == Self::VERSION_417 {
            0x6c
        } else {
            0x68
        };
        if reader.position() != expected {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "invalid ASB header size",
            ));
        }
        Ok(result)
    }

    pub fn validate_offsets(&self, len: usize) -> io::Result<()> {
        let offsets = [
            self.local_blackboard_offset,
            self.string_pool_offset,
            self.enum_resolve_array_offset,
            self.x2c_offset,
            self.event_offsets_offset,
            self.slots_offset,
            self.x38_offset,
            self.x38_index_offset,
            self.x40_offset,
            self.bone_group_offset,
            self.transitions_offset,
            self.tag_list_offset,
            self.as_markings_offset,
            self.exb_offset,
            self.command_groups_offset,
            self.x68_offset.unwrap_or(0),
        ];
        if let Some(offset) = offsets.into_iter().find(|offset| *offset as usize > len) {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                format!("ASB section offset {offset:#x} exceeds file size {len:#x}"),
            ));
        }
        let pool_end = self.string_pool_offset as usize + self.string_pool_size as usize;
        if pool_end > len {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "ASB string pool exceeds file",
            ));
        }
        Ok(())
    }
}
