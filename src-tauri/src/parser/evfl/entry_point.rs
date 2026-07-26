use super::{container::ContainerItem, radix_tree::read_keys};
use crate::parser::binary::BinaryReader;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::io::{self, ErrorKind};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct VariableDef {
    #[serde(flatten)]
    pub value: ContainerItem,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct EntryPoint {
    pub sub_flow_event_indices: Vec<i16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variables: Option<IndexMap<String, VariableDef>>,
    pub event_index: i16,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub register_null_variable_dictionary: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub register_null_variable_definitions: bool,
}

impl EntryPoint {
    pub fn read(data: &[u8], offset: u64) -> io::Result<Self> {
        let mut reader = BinaryReader::new(data);
        reader.seek(offset as usize)?;
        let subflows_offset = reader.read_u64()?;
        let names_offset = reader.read_u64()?;
        let definitions_offset = reader.read_u64()?;
        let subflow_count = reader.read_u16()? as usize;
        let definition_count = reader.read_u16()? as usize;
        let event_index = reader.read_i16()?;

        let mut subflow_reader = BinaryReader::new(data);
        if subflow_count != 0 {
            subflow_reader.seek(subflows_offset as usize)?;
        }
        let sub_flow_event_indices = (0..subflow_count)
            .map(|_| subflow_reader.read_i16())
            .collect::<io::Result<_>>()?;

        let variables = if definition_count == 0 || names_offset == 0 {
            None
        } else {
            let keys = read_keys(data, names_offset)?;
            if keys.len() != definition_count {
                return Err(io::Error::new(
                    ErrorKind::InvalidData,
                    "variable definition dictionary length mismatch",
                ));
            }
            let mut definitions = BinaryReader::new(data);
            definitions.seek(definitions_offset as usize)?;
            let mut values = IndexMap::new();
            for key in keys {
                let raw = definitions.read_u64()?;
                let count = definitions.read_u16()? as usize;
                let kind = definitions.read_u8()?;
                definitions.align(8)?;
                let mut item = ContainerItem::default();
                match kind {
                    2 => item.int = Some(raw as i32),
                    3 => item.bool = Some(raw as i64 != 0),
                    // This intentionally mirrors BfevLibrary's numeric cast.
                    4 => item.float = Some(raw as i64 as f32),
                    7 | 8 | 9 => {
                        let mut array = BinaryReader::new(data);
                        array.seek(raw as usize)?;
                        match kind {
                            7 => {
                                item.int_array = Some(
                                    (0..count)
                                        .map(|_| array.read_i32())
                                        .collect::<io::Result<_>>()?,
                                )
                            }
                            8 => {
                                item.bool_array = Some(
                                    (0..count)
                                        .map(|_| Ok(array.read_i32()? != 0))
                                        .collect::<io::Result<_>>()?,
                                )
                            }
                            9 => {
                                item.float_array = Some(
                                    (0..count)
                                        .map(|_| array.read_f32())
                                        .collect::<io::Result<_>>()?,
                                )
                            }
                            _ => {
                                return Err(io::Error::new(
                                    ErrorKind::InvalidData,
                                    format!("unsupported variable array type {kind}"),
                                ))
                            }
                        }
                    }
                    _ => {
                        return Err(io::Error::new(
                            ErrorKind::InvalidData,
                            format!("unsupported variable type {kind}"),
                        ))
                    }
                }
                values.insert(key, VariableDef { value: item });
            }
            Some(values)
        };
        Ok(Self {
            sub_flow_event_indices,
            variables,
            event_index,
            register_null_variable_dictionary: names_offset == 0
                && is_relocated_pointer(data, offset as usize + 8)?,
            register_null_variable_definitions: definitions_offset == 0
                && is_relocated_pointer(data, offset as usize + 16)?,
        })
    }
}

fn is_relocated_pointer(data: &[u8], address: usize) -> io::Result<bool> {
    let mut header = BinaryReader::new(data);
    header.seek(24)?;
    let relocation = header.read_u32()? as usize;
    let mut count_reader = BinaryReader::new(data);
    count_reader.seek(relocation + 36)?;
    let count = count_reader.read_u32()? as usize;
    let entries = relocation + 40;
    let mut low = 0usize;
    let mut high = count;
    while low < high {
        let middle = (low + high) / 2;
        let mut entry = BinaryReader::new(data);
        entry.seek(entries + middle * 8)?;
        if entry.read_u32()? as usize <= address {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    for index in low.saturating_sub(2)..low {
        let mut entry = BinaryReader::new(data);
        entry.seek(entries + index * 8)?;
        let base = entry.read_u32()? as usize;
        let flags = entry.read_u32()?;
        if address >= base && address < base + 256 && (address - base) % 8 == 0 {
            return Ok(flags & (1u32 << ((address - base) / 8)) != 0);
        }
    }
    Ok(false)
}
