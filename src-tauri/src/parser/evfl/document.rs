use super::{
    flowchart::Flowchart,
    radix_tree::{read_keys, read_offset_array, read_string},
    timeline::Timeline,
};
use crate::parser::binary::BinaryReader;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{self, ErrorKind},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct BfevDocument {
    pub file_name: String,
    pub version: String,
    pub flowcharts: BTreeMap<String, Flowchart>,
    pub timelines: BTreeMap<String, Timeline>,
}

impl BfevDocument {
    pub fn from_binary(data: &[u8]) -> io::Result<Self> {
        let mut reader = BinaryReader::new(data);
        if reader.read_bytes(6)? != b"BFEVFL" {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "invalid BFEVFL magic",
            ));
        }
        reader.skip(2)?;
        let version = reader
            .read_bytes(4)?
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(".");
        reader.skip(4)?;
        let file_name_pointer = reader.read_u32()?;
        let file_name_offset = file_name_pointer
            .checked_sub(2)
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidData, "invalid file name pointer"))?;
        let file_name = read_string(data, file_name_offset as u64)?;
        reader.skip(12)?;
        let flowchart_count = reader.read_u16()? as usize;
        let timeline_count = reader.read_u16()? as usize;
        reader.skip(4)?;
        let flowchart_offsets_pointer = reader.read_u64()?;
        let flowchart_dictionary_pointer = reader.read_u64()?;
        let timeline_offsets_pointer = reader.read_u64()?;
        let timeline_dictionary_pointer = reader.read_u64()?;
        let names = read_keys(data, flowchart_dictionary_pointer)?;
        let offsets = read_offset_array(data, flowchart_offsets_pointer, flowchart_count)?;
        if names.len() != flowchart_count || offsets.len() != flowchart_count {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "flowchart table length mismatch",
            ));
        }
        let mut flowcharts = BTreeMap::new();
        for (name, offset) in names.into_iter().zip(offsets) {
            flowcharts.insert(name, Flowchart::read(data, offset)?);
        }
        let timeline_names = read_keys(data, timeline_dictionary_pointer)?;
        let timeline_offsets = read_offset_array(data, timeline_offsets_pointer, timeline_count)?;
        if timeline_names.len() != timeline_count || timeline_offsets.len() != timeline_count {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "timeline table length mismatch",
            ));
        }
        let mut timelines = BTreeMap::new();
        for (name, offset) in timeline_names.into_iter().zip(timeline_offsets) {
            timelines.insert(name, Timeline::read(data, offset)?);
        }
        Ok(Self {
            file_name,
            version,
            flowcharts,
            timelines,
        })
    }

    pub fn to_json(&self) -> io::Result<String> {
        serde_json::to_string_pretty(self)
            .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))
    }
}
