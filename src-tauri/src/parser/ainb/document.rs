use super::{
    blackboard::AinbBlackboard,
    command::AinbCommand,
    header::{AinbHeader, HEADER_SIZE},
    node::AinbNode,
    section::AinbSection,
};
use crate::parser::binary::BinaryReader;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AinbDocument {
    pub version: u32,
    pub filename: String,
    pub category: String,
    pub blackboard_id: u32,
    pub parent_blackboard_id: u32,
    #[serde(skip)]
    pub header: AinbHeader,
    pub commands: Vec<AinbCommand>,
    pub nodes: Vec<AinbNode>,
    #[serde(skip)]
    pub blackboard_layout: AinbBlackboard,
    pub blackboard: serde_yaml::Value,
    pub expressions: serde_yaml::Value,
    pub replacement_table: serde_yaml::Value,
    pub modules: serde_yaml::Value,
    pub unknown_section_0x58: serde_yaml::Value,
    pub has_section_0x6c: bool,
    #[serde(skip)]
    pub sections: Vec<AinbSection>,
    #[serde(skip)]
    original_data: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_ainb_data() {
        let error = AinbDocument::from_bytes(b"not an ainb").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}

impl AinbDocument {
    pub fn from_bytes(data: &[u8]) -> io::Result<Self> {
        let mut reader = BinaryReader::new(data);
        let header = AinbHeader::read(&mut reader)?;
        let pool = header.string_pool_offset as usize;
        let filename = reader.read_c_string_at(pool + header.filename_offset as usize)?;
        let category = reader.read_c_string_at(pool + header.category_name_offset as usize)?;
        reader.seek(HEADER_SIZE)?;
        let commands = (0..header.command_count)
            .map(|_| AinbCommand::read(&mut reader, pool))
            .collect::<io::Result<Vec<_>>>()?;
        let nodes = (0..header.node_count)
            .map(|_| AinbNode::read(&mut reader, header.version, pool))
            .collect::<io::Result<Vec<_>>>()?;
        let blackboard_layout = AinbBlackboard::read(&mut reader, header.blackboard_offset)?;
        reader.seek(header.blackboard_id_offset as usize)?;
        let blackboard_id = reader.read_u32()?;
        let parent_blackboard_id = reader.read_u32()?;
        let has_section_0x6c = header.section_0x6c_offset != 0;
        let mut offsets = BTreeMap::new();
        for (name, offset) in header.named_offsets() {
            if offset > 0 && (offset as usize) < data.len() {
                offsets.entry(offset as usize).or_insert(name);
            }
        }
        offsets.entry(pool).or_insert("string_pool");
        let entries: Vec<_> = offsets.into_iter().collect();
        let sections = entries
            .iter()
            .enumerate()
            .map(|(index, (offset, name))| {
                let end = entries
                    .get(index + 1)
                    .map(|entry| entry.0)
                    .unwrap_or(data.len());
                AinbSection::new(name, *offset, &data[*offset..end])
            })
            .collect();
        Ok(Self {
            version: header.version,
            filename,
            category,
            blackboard_id,
            parent_blackboard_id,
            header,
            commands,
            nodes,
            blackboard_layout,
            blackboard: serde_yaml::Value::Mapping(Default::default()),
            expressions: serde_yaml::Value::Mapping(Default::default()),
            replacement_table: serde_yaml::Value::Sequence(Vec::new()),
            modules: serde_yaml::Value::Sequence(Vec::new()),
            unknown_section_0x58: serde_yaml::Value::Mapping(Default::default()),
            has_section_0x6c,
            sections,
            original_data: data.to_vec(),
        })
    }

    pub fn to_bytes(&self) -> io::Result<Vec<u8>> {
        if self.original_data.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "AINB binary cannot be rebuilt from YAML until all semantic sections have writers",
            ));
        }
        let bytes = self.original_data.clone();
        let reparsed = Self::from_bytes(&bytes)?;
        if self.filename != reparsed.filename
            || self.category != reparsed.category
            || self.header.version != reparsed.header.version
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "editing top-level AINB identity fields is not supported yet",
            ));
        }
        Ok(bytes)
    }

    pub fn to_yaml(&self) -> io::Result<String> {
        serde_yaml::to_string(self).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
    pub fn from_yaml(text: &str) -> io::Result<Self> {
        serde_yaml::from_str(text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
}
