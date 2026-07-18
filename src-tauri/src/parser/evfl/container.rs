use super::radix_tree::{read_keys, read_offset_array, read_string_ptr};
use crate::parser::binary::BinaryReader;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{self, ErrorKind},
};

pub type Container = BTreeMap<String, ContainerItem>;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ActorIdentifier {
    pub item1: String,
    pub item2: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ContainerItem {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub argument: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Container>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub int: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bool: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub float: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub string: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub w_string: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub int_array: Option<Vec<i32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bool_array: Option<Vec<bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub float_array: Option<Vec<f32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub string_array: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub w_string_array: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor_identifier: Option<ActorIdentifier>,
}

pub fn read_container(data: &[u8], offset: u64) -> io::Result<Container> {
    let root = read_item(data, offset, true)?;
    root.items
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidData, "container root is not a container"))
}

fn read_item(data: &[u8], offset: u64, root: bool) -> io::Result<ContainerItem> {
    let mut reader = BinaryReader::new(data);
    reader.seek(offset as usize)?;
    let kind = reader.read_u8()?;
    reader.skip(1)?;
    let count = reader.read_u16()? as usize;
    reader.skip(4)?;
    let dictionary_offset = reader.read_u64()?;
    let mut item = ContainerItem::default();

    if kind == 1 {
        let keys = read_keys(data, dictionary_offset)?;
        let offsets = if root {
            // Root containers store their item pointer array inline. Nested
            // containers store a pointer to that array in the same position.
            read_offset_array(data, offset + 16, count)?
        } else {
            let offsets_ptr = reader.read_u64()?;
            read_offset_array(data, offsets_ptr, count)?
        };
        if keys.len() != offsets.len() {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "container dictionary length mismatch",
            ));
        }
        let mut items = Container::new();
        for (key, child_offset) in keys.into_iter().zip(offsets) {
            items.insert(key, read_item(data, child_offset, false)?);
        }
        item.items = Some(items);
        return Ok(item);
    }
    if root {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "invalid container root type",
        ));
    }

    match kind {
        0 => item.argument = Some(read_string_ptr(&mut reader, data)?),
        2 => item.int = Some(reader.read_i32()?),
        3 => item.bool = Some(reader.read_u32()? == 0x8000_0001),
        4 => item.float = Some(reader.read_f32()?),
        5 => item.string = Some(read_string_ptr(&mut reader, data)?),
        6 => item.w_string = Some(read_string_ptr(&mut reader, data)?),
        7 => {
            item.int_array = Some(
                (0..count)
                    .map(|_| reader.read_i32())
                    .collect::<io::Result<_>>()?,
            )
        }
        8 => {
            item.bool_array = Some(
                (0..count)
                    .map(|_| Ok(reader.read_u32()? == 0x8000_0001))
                    .collect::<io::Result<_>>()?,
            )
        }
        9 => {
            item.float_array = Some(
                (0..count)
                    .map(|_| reader.read_f32())
                    .collect::<io::Result<_>>()?,
            )
        }
        10 => {
            item.string_array = Some(
                (0..count)
                    .map(|_| read_string_ptr(&mut reader, data))
                    .collect::<io::Result<_>>()?,
            )
        }
        11 => {
            item.w_string_array = Some(
                (0..count)
                    .map(|_| read_string_ptr(&mut reader, data))
                    .collect::<io::Result<_>>()?,
            )
        }
        12 => {
            item.actor_identifier = Some(ActorIdentifier {
                item1: read_string_ptr(&mut reader, data)?,
                item2: read_string_ptr(&mut reader, data)?,
            })
        }
        _ => {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                format!("unsupported container type {kind}"),
            ))
        }
    }
    Ok(item)
}
