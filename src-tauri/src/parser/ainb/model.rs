use super::{
    common::{murmur3_32, AinbWriter},
    parameter::{ParamType, PropertySet},
};
use crate::parser::binary::BinaryReader;
use serde::{Deserialize, Serialize};
use std::io;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Attachment {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Debug")]
    pub debug: u32,
    #[serde(rename = "Properties", default)]
    pub properties: PropertySet,
}

impl Attachment {
    pub fn read(
        reader: &mut BinaryReader<'_>,
        pool: usize,
        version: u32,
        properties: &PropertySet,
    ) -> io::Result<Self> {
        let name_offset = reader.read_u32()? as usize;
        let name = reader.read_c_string_at(pool + name_offset)?;
        let parameter_offset = reader.read_u32()? as usize;
        reader.read_u16()?;
        reader.read_u16()?;
        if version >= 0x407 {
            reader.read_u32()?;
        }
        let position = reader.position();
        reader.seek(parameter_offset)?;
        let debug = reader.read_u32()?;
        let mut selected = PropertySet::new();
        for kind in ParamType::ALL {
            let base = reader.read_u32()? as usize;
            let count = reader.read_u32()? as usize;
            let values = properties
                .get(kind.name())
                .map(Vec::as_slice)
                .unwrap_or(&[])
                .get(base..base + count)
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "attachment property range exceeds table",
                    )
                })?
                .to_vec();
            if !values.is_empty() {
                selected.insert(kind.name().to_owned(), values);
            }
        }
        reader.seek(position)?;
        Ok(Self {
            name,
            debug,
            properties: selected,
        })
    }

    pub fn write_header(
        &self,
        writer: &mut AinbWriter,
        parameter_offset: u32,
        expression_count: u16,
        expression_size: u16,
        version: u32,
    ) {
        writer.write_string_offset(&self.name);
        writer.write_u32(parameter_offset);
        writer.write_u16(expression_count);
        writer.write_u16(expression_size);
        if version >= 0x407 {
            writer.write_u32(murmur3_32(&self.name));
        }
    }

    pub fn write_parameters(&self, writer: &mut AinbWriter, property_indices: &mut [u32; 6]) {
        writer.write_u32(self.debug);
        for (index, kind) in ParamType::ALL.into_iter().enumerate() {
            let count = self.properties.get(kind.name()).map_or(0, Vec::len) as u32;
            writer.write_u32(property_indices[index]);
            writer.write_u32(count);
            property_indices[index] += count;
        }
        let offset = writer.position() as u32 + 0x30;
        for _ in 0..6 {
            writer.write_u32(0);
            writer.write_u32(offset);
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Action {
    #[serde(rename = "Action Slot")]
    pub action_slot: String,
    #[serde(rename = "Action")]
    pub action: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Module {
    #[serde(rename = "Path")]
    pub path: String,
    #[serde(rename = "Category")]
    pub category: String,
    #[serde(rename = "Instance Count")]
    pub instance_count: u32,
}

impl Module {
    pub fn write(&self, writer: &mut AinbWriter) {
        writer.write_string_offset(&self.path);
        writer.write_string_offset(&self.category);
        writer.write_u32(self.instance_count);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplacementType {
    RemoveChild,
    ReplaceChild,
    RemoveAttachment,
}

impl ReplacementType {
    pub fn from_raw(value: u8) -> io::Result<Self> {
        match value {
            0 => Ok(Self::RemoveChild),
            1 => Ok(Self::ReplaceChild),
            2 => Ok(Self::RemoveAttachment),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unknown replacement type {value}"),
            )),
        }
    }

    pub fn to_raw(self) -> u8 {
        match self {
            Self::RemoveChild => 0,
            Self::ReplaceChild => 1,
            Self::RemoveAttachment => 2,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Replacement {
    #[serde(rename = "Type")]
    pub replacement_type: String,
    #[serde(rename = "Node Index")]
    pub node_index: i16,
    #[serde(
        rename = "Child Plug Index",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub child_plug_index: Option<i16>,
    #[serde(
        rename = "Attachment Index",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub attachment_index: Option<i16>,
    #[serde(
        rename = "Replacement Node Index",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub replacement_node_index: Option<i16>,
}

impl Replacement {
    pub fn kind(&self) -> io::Result<ReplacementType> {
        match self.replacement_type.as_str() {
            "RemoveChild" => Ok(ReplacementType::RemoveChild),
            "ReplaceChild" => Ok(ReplacementType::ReplaceChild),
            "RemoveAttachment" => Ok(ReplacementType::RemoveAttachment),
            other => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unknown replacement type {other}"),
            )),
        }
    }

    pub fn write(&self, writer: &mut AinbWriter) -> io::Result<()> {
        self.write_with_raw_new_index(writer, None)
    }

    pub fn write_with_raw_new_index(
        &self,
        writer: &mut AinbWriter,
        raw_new_index: Option<i16>,
    ) -> io::Result<()> {
        let kind = self.kind()?;
        writer.write_u8(kind.to_raw());
        writer.write_u8(0);
        writer.write_i16(self.node_index);
        writer.write_i16(
            match kind {
                ReplacementType::RemoveAttachment => self.attachment_index,
                _ => self.child_plug_index,
            }
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "missing replacement index")
            })?,
        );
        writer.write_i16(self.replacement_node_index.or(raw_new_index).unwrap_or(-1));
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UnknownSection58 {
    #[serde(rename = "Description")]
    pub description: String,
    #[serde(rename = "Unknown04")]
    pub unknown04: u32,
    #[serde(rename = "Unknown08")]
    pub unknown08: u32,
    #[serde(rename = "Unknown0C")]
    pub unknown0c: u32,
}
