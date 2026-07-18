use super::{
    flowchart::Flowchart,
    radix_tree::{read_keys, read_offset_array, read_string},
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
    // Timeline support is represented explicitly rather than silently dropping
    // data. Parsing currently rejects files containing one.
    pub timelines: BTreeMap<String, serde_json::Value>,
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
        let _timeline_offsets_pointer = reader.read_u64()?;
        let _timeline_dictionary_pointer = reader.read_u64()?;

        if timeline_count != 0 {
            return Err(io::Error::new(
                ErrorKind::Unsupported,
                "BFEV timeline blocks are not supported yet",
            ));
        }
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
        Ok(Self {
            file_name,
            version,
            flowcharts,
            timelines: BTreeMap::new(),
        })
    }

    pub fn to_json(&self) -> io::Result<String> {
        serde_json::to_string_pretty(self)
            .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))
    }
}
