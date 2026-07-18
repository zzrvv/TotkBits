use super::{
    event::AsbEvent,
    node_body,
    node_tables::{guid, BoneGroup, X38Entry, X40Entry},
    node_type::NodeType,
    x2c::X2cEntry,
};
use crate::parser::binary::BinaryReader;
use serde::{Deserialize, Serialize};
use std::io;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AsbNode {
    #[serde(rename = "Node Type")]
    pub node_type: String,
    #[serde(rename = "Unknown")]
    pub unknown: u8,
    #[serde(rename = "Tags", skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(rename = "GUID")]
    pub guid: String,
    #[serde(rename = "0x38 Entries")]
    pub x38_entries: Vec<X38Entry>,
    #[serde(rename = "0x40 Entries")]
    pub x40_entries: Vec<X40Entry>,
    #[serde(rename = "ASMarkings", skip_serializing_if = "Option::is_none")]
    pub as_markings: Option<Vec<String>>,
    #[serde(skip)]
    pub body_offset: u32,
    #[serde(rename = "Body", skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_yaml::Value>,
}
impl AsbNode {
    pub fn read(
        r: &mut BinaryReader<'_>,
        p: &BinaryReader<'_>,
        x38: &[X38Entry],
        x40: &[X40Entry],
        markings: &[Vec<String>],
        x38_index_offset: u32,
        version: u32,
        x2c: &[X2cEntry],
        events: &[AsbEvent],
        bone_groups: &[BoneGroup],
    ) -> io::Result<Self> {
        let node_type = NodeType::from_u16(r.read_u16()?)?.name().to_string();
        let x38_count = r.read_u8()?;
        let unknown = r.read_u8()?;
        let tag_offset = r.read_u32()?;
        let tags = if tag_offset != 0 {
            let ret = r.position();
            r.seek(tag_offset as usize)?;
            let n = r.read_u32()?;
            let mut v = Vec::new();
            for _ in 0..n {
                v.push(p.read_c_string_at(r.read_u32()? as usize)?)
            }
            r.seek(ret)?;
            Some(v)
        } else {
            None
        };
        let body_offset = r.read_u32()?;
        let x40_index = r.read_u16()? as usize;
        let x40_count = r.read_u16()? as usize;
        let x38_index = r.read_u16()? as usize;
        let marking_index = i32::from(r.read_u16()?) - 1;
        let guid = guid(r)?;
        let ret = r.position();
        let x40_entries = x40
            .get(x40_index..x40_index + x40_count)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "ASB node 0x40 range exceeds table",
                )
            })?
            .to_vec();
        let mut x38_entries = Vec::new();
        if x38_count != 0 {
            r.seek(x38_index_offset as usize + 4 * x38_index)?;
            for _ in 0..x38_count {
                let i = r.read_u32()? as usize;
                x38_entries.push(
                    x38.get(i)
                        .ok_or_else(|| {
                            io::Error::new(
                                io::ErrorKind::InvalidData,
                                "ASB node 0x38 index exceeds table",
                            )
                        })?
                        .clone(),
                )
            }
        }
        r.seek(body_offset as usize)?;
        let body = node_body::read(&node_type, r, p, version, x2c, events, bone_groups)?;
        r.seek(ret)?;
        let as_markings = if marking_index >= 0 {
            Some(
                markings
                    .get(marking_index as usize)
                    .ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            "ASB marking index exceeds table",
                        )
                    })?
                    .clone(),
            )
        } else {
            None
        };
        Ok(Self {
            node_type,
            unknown,
            tags,
            guid,
            x38_entries,
            x40_entries,
            as_markings,
            body_offset,
            body,
        })
    }
}
