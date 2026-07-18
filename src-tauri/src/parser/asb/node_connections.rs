use super::x2c::X2cEntry;
use crate::parser::binary::BinaryReader;
use serde::Serialize;
use std::io;

#[derive(Clone, Debug, Default, Serialize)]
pub struct NodeConnections {
    #[serde(skip)]
    pub child_offsets: Vec<u32>,
    #[serde(rename = "State Nodes", skip_serializing_if = "Vec::is_empty")]
    pub state_nodes: Vec<u32>,
    #[serde(rename = "0x2C Connections", skip_serializing_if = "Vec::is_empty")]
    pub x2c: Vec<ResolvedX2c>,
    #[serde(
        rename = "Event Node Connections",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub event_nodes: Vec<u32>,
    #[serde(
        rename = "Frame Node Connections",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub frame_nodes: Vec<u32>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub enum ResolvedX2c {
    Modern {
        #[serde(rename = "0x2C Entry")]
        entry: serde_yaml::Value,
        #[serde(rename = "Node Index")]
        node_index: u32,
    },
    Legacy(u32),
}
impl NodeConnections {
    pub fn read(r: &mut BinaryReader<'_>, version: u32, table: &[X2cEntry]) -> io::Result<Self> {
        let mut counts = [0u8; 6];
        for count in &mut counts {
            *count = r.read_u8()?;
            r.read_u8()?;
        }
        let mut offsets: [Vec<u32>; 6] = Default::default();
        for (i, count) in counts.into_iter().enumerate() {
            for _ in 0..count {
                offsets[i].push(r.read_u32()?);
            }
        }
        let state_nodes = read_at(r, &offsets[0])?;
        let child_offsets = offsets[2].clone();
        let event_nodes = read_at(r, &offsets[4])?;
        let frame_nodes = read_at(r, &offsets[5])?;
        let mut x2c = Vec::new();
        for offset in &offsets[3] {
            r.seek(*offset as usize)?;
            if version == 0x417 {
                let index = r.read_i32()?;
                let entry = if index >= 0 {
                    serde_yaml::to_value(table.get(index as usize).ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "ASB 0x2C index exceeds table")
                    })?)
                    .map_err(io::Error::other)?
                } else {
                    serde_yaml::Value::Mapping(Default::default())
                };
                x2c.push(ResolvedX2c::Modern {
                    entry,
                    node_index: r.read_u32()?,
                });
            } else {
                x2c.push(ResolvedX2c::Legacy(r.read_u32()?));
            }
        }
        Ok(Self {
            child_offsets,
            state_nodes,
            x2c,
            event_nodes,
            frame_nodes,
        })
    }
}
fn read_at(r: &mut BinaryReader<'_>, offsets: &[u32]) -> io::Result<Vec<u32>> {
    let mut out = Vec::with_capacity(offsets.len());
    for offset in offsets {
        r.seek(*offset as usize)?;
        out.push(r.read_u32()?)
    }
    Ok(out)
}
