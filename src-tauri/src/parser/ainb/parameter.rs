use super::common::AinbWriter;
use crate::parser::binary::BinaryReader;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::{collections::BTreeMap, io};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub enum ParamType {
    Int,
    Bool,
    Float,
    String,
    Vector3F,
    Pointer,
}

impl ParamType {
    pub const ALL: [Self; 6] = [
        Self::Int,
        Self::Bool,
        Self::Float,
        Self::String,
        Self::Vector3F,
        Self::Pointer,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Int => "Int",
            Self::Bool => "Bool",
            Self::Float => "Float",
            Self::String => "String",
            Self::Vector3F => "Vector3F",
            Self::Pointer => "Pointer",
        }
    }

    pub fn property_size(self) -> usize {
        if self == Self::Vector3F {
            0x14
        } else {
            0x0c
        }
    }

    pub fn input_size(self) -> usize {
        match self {
            Self::Vector3F => 0x18,
            Self::Pointer => 0x14,
            _ => 0x10,
        }
    }

    pub fn output_size(self) -> usize {
        if self == Self::Pointer {
            8
        } else {
            4
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ParamFlags {
    #[serde(rename = "Flags", default)]
    pub names: Vec<String>,
    #[serde(
        rename = "Expression Index",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub expression_index: Option<u16>,
    #[serde(
        rename = "Blackboard Index",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub blackboard_index: Option<u16>,
    #[serde(
        rename = "Vector Component",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub vector_component: Option<String>,
}

impl ParamFlags {
    pub fn from_raw(raw: u32) -> Self {
        let mut names = Vec::new();
        if raw & 0x0080_0000 != 0 {
            names.push("Uses Default".to_owned());
        }
        if raw & 0x0100_0000 != 0 {
            names.push("Is Output".to_owned());
        }
        let expression = raw & 0xc200_0000 == 0xc200_0000;
        let blackboard = !expression && raw & 0xc200_0000 != 0;
        let component = match (raw >> 26) & 3 {
            1 => Some("X".to_owned()),
            2 => Some("Y".to_owned()),
            3 => Some("Z".to_owned()),
            _ => None,
        };
        Self {
            names,
            expression_index: expression.then_some(raw as u16),
            blackboard_index: blackboard.then_some(raw as u16),
            vector_component: blackboard.then_some(component).flatten(),
        }
    }

    pub fn to_raw(&self) -> io::Result<u32> {
        let mut raw = 0;
        for name in &self.names {
            raw |= match name.as_str() {
                "Uses Default" => 0x0080_0000,
                "Is Output" => 0x0100_0000,
                other => return Err(invalid(format!("unknown parameter flag {other}"))),
            };
        }
        if let Some(index) = self.expression_index {
            raw |= 0xc200_0000 | index as u32;
        } else if let Some(index) = self.blackboard_index {
            raw |= 0x8000_0000 | index as u32;
            raw |= match self.vector_component.as_deref() {
                None => 0,
                Some("X") => 1 << 26,
                Some("Y") => 2 << 26,
                Some("Z") => 3 << 26,
                Some(other) => return Err(invalid(format!("unknown vector component {other}"))),
            };
        }
        Ok(raw)
    }

    pub fn is_expression(&self) -> bool {
        self.expression_index.is_some()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Property {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Classname", default, skip_serializing_if = "Option::is_none")]
    pub classname: Option<String>,
    #[serde(rename = "Default Value")]
    pub default_value: Value,
    #[serde(flatten)]
    pub flags: ParamFlags,
}

pub type PropertySet = BTreeMap<String, Vec<Property>>;

impl Property {
    fn read(reader: &mut BinaryReader<'_>, pool: usize, kind: ParamType) -> io::Result<Self> {
        let name = read_string_offset(reader, pool)?;
        let classname = if kind == ParamType::Pointer {
            Some(read_string_offset(reader, pool)?)
        } else {
            None
        };
        let flags = ParamFlags::from_raw(reader.read_u32()?);
        let default_value = read_value(reader, pool, kind, true)?;
        Ok(Self {
            name,
            classname,
            default_value,
            flags,
        })
    }

    pub fn write(&self, writer: &mut AinbWriter, kind: ParamType) -> io::Result<()> {
        writer.write_string_offset(&self.name);
        if kind == ParamType::Pointer {
            writer.write_string_offset(self.classname.as_deref().ok_or_else(|| {
                invalid(format!("pointer property {} has no Classname", self.name))
            })?);
        }
        writer.write_u32(self.flags.to_raw()?);
        write_value(writer, kind, &self.default_value, true)
    }
}

pub fn read_property_set(
    reader: &mut BinaryReader<'_>,
    pool: usize,
    end_offset: usize,
) -> io::Result<PropertySet> {
    let mut offsets = [0usize; 6];
    for offset in &mut offsets {
        *offset = reader.read_u32()? as usize;
    }
    let mut properties = PropertySet::new();
    for (index, kind) in ParamType::ALL.into_iter().enumerate() {
        let end = offsets.get(index + 1).copied().unwrap_or(end_offset);
        if offsets[index] > end {
            return Err(invalid("property offsets are not ordered"));
        }
        reader.seek(offsets[index])?;
        let count = (end - offsets[index]) / kind.property_size();
        let values = (0..count)
            .map(|_| Property::read(reader, pool, kind))
            .collect::<io::Result<Vec<_>>>()?;
        if !values.is_empty() {
            properties.insert(kind.name().to_owned(), values);
        }
    }
    Ok(properties)
}

pub fn write_property_set(writer: &mut AinbWriter, properties: &PropertySet) -> io::Result<()> {
    let mut offset = writer.position() + 0x18;
    for kind in ParamType::ALL {
        writer.write_u32(offset as u32);
        offset += properties.get(kind.name()).map_or(0, Vec::len) * kind.property_size();
    }
    for kind in ParamType::ALL {
        if let Some(values) = properties.get(kind.name()) {
            for value in values {
                value.write(writer, kind)?;
            }
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ParamSource {
    #[serde(rename = "Node Index")]
    pub node_index: i16,
    #[serde(rename = "Output Index")]
    pub output_index: i16,
    #[serde(flatten)]
    pub flags: ParamFlags,
}

impl ParamSource {
    fn read(reader: &mut BinaryReader<'_>) -> io::Result<Self> {
        Ok(Self {
            node_index: reader.read_i16()?,
            output_index: reader.read_i16()?,
            flags: ParamFlags::from_raw(reader.read_u32()?),
        })
    }

    pub fn write(&self, writer: &mut AinbWriter, set_blackboard: bool) -> io::Result<()> {
        writer.write_i16(self.node_index);
        if set_blackboard {
            writer.write_u16(self.output_index as u16 | 0x8000);
        } else {
            writer.write_i16(self.output_index);
        }
        writer.write_u32(self.flags.to_raw()?);
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InputParam {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Classname", default, skip_serializing_if = "Option::is_none")]
    pub classname: Option<String>,
    #[serde(rename = "Default Value")]
    pub default_value: Value,
    #[serde(rename = "Sources", default, skip_serializing_if = "Option::is_none")]
    pub sources: Option<Vec<ParamSource>>,
    #[serde(flatten)]
    pub source: Option<ParamSource>,
    #[serde(
        rename = "Is Set Blackboard",
        default,
        skip_serializing_if = "is_false"
    )]
    pub is_set_blackboard: bool,
}

impl InputParam {
    fn read(
        reader: &mut BinaryReader<'_>,
        pool: usize,
        kind: ParamType,
        multi: &[ParamSource],
    ) -> io::Result<Self> {
        let name = read_string_offset(reader, pool)?;
        let classname = if kind == ParamType::Pointer {
            Some(read_string_offset(reader, pool)?)
        } else {
            None
        };
        let mut source = ParamSource::read(reader)?;
        let default_value = read_value(reader, pool, kind, false)?;
        let (source, sources, is_set_blackboard) = if source.node_index <= -100 {
            let index = (-100i32 - source.node_index as i32) as usize;
            let count = source.output_index as usize;
            let values = multi
                .get(index..index + count)
                .ok_or_else(|| invalid("multi-source range exceeds table"))?
                .to_vec();
            (None, Some(values), false)
        } else {
            let is_set_blackboard = source.output_index < 0;
            if is_set_blackboard {
                source.output_index = (source.output_index as u16 & 0x7fff) as i16;
            }
            (Some(source), None, is_set_blackboard)
        };
        Ok(Self {
            name,
            classname,
            default_value,
            sources,
            source,
            is_set_blackboard,
        })
    }

    pub fn write(
        &self,
        writer: &mut AinbWriter,
        kind: ParamType,
        multi: &[ParamSource],
    ) -> io::Result<()> {
        writer.write_string_offset(&self.name);
        if kind == ParamType::Pointer {
            writer.write_string_offset(
                self.classname.as_deref().ok_or_else(|| {
                    invalid(format!("pointer input {} has no Classname", self.name))
                })?,
            );
        }
        if let Some(sources) = &self.sources {
            let index = multi
                .windows(sources.len())
                .position(|window| window == sources)
                .ok_or_else(|| invalid(format!("missing multi-source window for {}", self.name)))?;
            ParamSource {
                node_index: -100 - index as i16,
                output_index: sources.len() as i16,
                flags: ParamFlags::default(),
            }
            .write(writer, false)?;
        } else {
            self.source
                .as_ref()
                .ok_or_else(|| invalid(format!("input {} has no source", self.name)))?
                .write(writer, self.is_set_blackboard)?;
        }
        write_value(writer, kind, &self.default_value, false)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutputParam {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Classname", default, skip_serializing_if = "Option::is_none")]
    pub classname: Option<String>,
    #[serde(rename = "Is Output")]
    pub is_output: bool,
}

impl OutputParam {
    fn read(reader: &mut BinaryReader<'_>, pool: usize, kind: ParamType) -> io::Result<Self> {
        let flags = reader.read_u32()?;
        let name = reader.read_c_string_at(pool + (flags & 0x3fff_ffff) as usize)?;
        let classname = if kind == ParamType::Pointer {
            Some(read_string_offset(reader, pool)?)
        } else {
            None
        };
        Ok(Self {
            name,
            classname,
            is_output: flags >> 31 != 0,
        })
    }

    pub fn write(&self, writer: &mut AinbWriter, kind: ParamType) -> io::Result<()> {
        let mut offset = writer.add_string(&self.name);
        if self.is_output {
            offset |= 0x8000_0000;
        }
        writer.write_u32(offset);
        if kind == ParamType::Pointer {
            writer.write_string_offset(self.classname.as_deref().ok_or_else(|| {
                invalid(format!("pointer output {} has no Classname", self.name))
            })?);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ParamSet {
    #[serde(rename = "Inputs", default)]
    pub inputs: BTreeMap<String, Vec<InputParam>>,
    #[serde(rename = "Outputs", default)]
    pub outputs: BTreeMap<String, Vec<OutputParam>>,
}

impl ParamSet {
    pub fn write(&self, writer: &mut AinbWriter, multi: &[ParamSource]) -> io::Result<()> {
        let mut offset = writer.position() + 0x30;
        for kind in ParamType::ALL {
            writer.write_u32(offset as u32);
            offset += self.inputs.get(kind.name()).map_or(0, Vec::len) * kind.input_size();
            writer.write_u32(offset as u32);
            offset += self.outputs.get(kind.name()).map_or(0, Vec::len) * kind.output_size();
        }
        for kind in ParamType::ALL {
            if let Some(inputs) = self.inputs.get(kind.name()) {
                for input in inputs {
                    input.write(writer, kind, multi)?;
                }
            }
            if let Some(outputs) = self.outputs.get(kind.name()) {
                for output in outputs {
                    output.write(writer, kind)?;
                }
            }
        }
        Ok(())
    }
}

pub fn read_multi_sources(
    reader: &mut BinaryReader<'_>,
    offset: usize,
    end_offset: usize,
) -> io::Result<Vec<ParamSource>> {
    if offset > end_offset || (end_offset - offset) % 8 != 0 {
        return Err(invalid("invalid multi-source section bounds"));
    }
    reader.seek(offset)?;
    (0..(end_offset - offset) / 8)
        .map(|_| ParamSource::read(reader))
        .collect()
}

pub fn read_param_set(
    reader: &mut BinaryReader<'_>,
    pool: usize,
    end_offset: usize,
    multi: &[ParamSource],
) -> io::Result<ParamSet> {
    let mut offsets = [(0usize, 0usize); 6];
    for pair in &mut offsets {
        *pair = (reader.read_u32()? as usize, reader.read_u32()? as usize);
    }
    let mut output = ParamSet::default();
    for (index, kind) in ParamType::ALL.into_iter().enumerate() {
        let (input_offset, output_offset) = offsets[index];
        let output_end = offsets
            .get(index + 1)
            .map(|pair| pair.0)
            .unwrap_or(end_offset);
        if input_offset > output_offset || output_offset > output_end {
            return Err(invalid("I/O parameter offsets are not ordered"));
        }
        reader.seek(input_offset)?;
        let inputs = (0..(output_offset - input_offset) / kind.input_size())
            .map(|_| InputParam::read(reader, pool, kind, multi))
            .collect::<io::Result<Vec<_>>>()?;
        reader.seek(output_offset)?;
        let outputs = (0..(output_end - output_offset) / kind.output_size())
            .map(|_| OutputParam::read(reader, pool, kind))
            .collect::<io::Result<Vec<_>>>()?;
        if !inputs.is_empty() {
            output.inputs.insert(kind.name().to_owned(), inputs);
        }
        if !outputs.is_empty() {
            output.outputs.insert(kind.name().to_owned(), outputs);
        }
    }
    Ok(output)
}

fn write_value(
    writer: &mut AinbWriter,
    kind: ParamType,
    value: &Value,
    pointer_has_no_storage: bool,
) -> io::Result<()> {
    match kind {
        ParamType::Int => writer.write_i32(
            value
                .as_i64()
                .and_then(|value| i32::try_from(value).ok())
                .ok_or_else(|| invalid("expected i32 value"))?,
        ),
        ParamType::Bool => writer.write_u32(
            value
                .as_bool()
                .ok_or_else(|| invalid("expected bool value"))? as u32,
        ),
        ParamType::Float => writer.write_f32(
            value
                .as_f64()
                .ok_or_else(|| invalid("expected f32 value"))? as f32,
        ),
        ParamType::String => writer.write_string_offset(
            value
                .as_str()
                .ok_or_else(|| invalid("expected string value"))?,
        ),
        ParamType::Vector3F => {
            let sequence = value
                .as_sequence()
                .filter(|value| value.len() == 3)
                .ok_or_else(|| invalid("expected three-component vector"))?;
            let mut vector = [0.0; 3];
            for (output, input) in vector.iter_mut().zip(sequence) {
                *output = input
                    .as_f64()
                    .ok_or_else(|| invalid("expected vector f32"))?
                    as f32;
            }
            writer.write_vec3(vector);
        }
        ParamType::Pointer => {
            if !value.is_null() {
                return Err(invalid("pointer default value must be null"));
            }
            if !pointer_has_no_storage {
                writer.write_u32(0);
            }
        }
    }
    Ok(())
}

fn read_value(
    reader: &mut BinaryReader<'_>,
    pool: usize,
    kind: ParamType,
    pointer_has_no_storage: bool,
) -> io::Result<Value> {
    Ok(match kind {
        ParamType::Int => Value::from(reader.read_i32()?),
        ParamType::Bool => Value::from(reader.read_u32()? != 0),
        ParamType::Float => Value::from(reader.read_f32()? as f64),
        ParamType::String => Value::from(read_string_offset(reader, pool)?),
        ParamType::Vector3F => Value::Sequence(vec![
            Value::from(reader.read_f32()? as f64),
            Value::from(reader.read_f32()? as f64),
            Value::from(reader.read_f32()? as f64),
        ]),
        ParamType::Pointer => {
            if !pointer_has_no_storage && reader.read_u32()? != 0 {
                return Err(invalid("pointer input default is non-zero"));
            }
            Value::Null
        }
    })
}

fn read_string_offset(reader: &mut BinaryReader<'_>, pool: usize) -> io::Result<String> {
    let offset = reader.read_u32()? as usize;
    reader.read_c_string_at(pool + offset)
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
