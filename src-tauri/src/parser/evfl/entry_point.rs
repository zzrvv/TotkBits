use super::{container::ContainerItem, radix_tree::read_keys};
use crate::parser::binary::BinaryReader;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{self, ErrorKind},
};

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
    pub variables: Option<BTreeMap<String, VariableDef>>,
    pub event_index: i16,
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
            let mut values = BTreeMap::new();
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
        })
    }
}
