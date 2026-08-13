use super::common::{murmur3_32, AinbWriter};
use crate::parser::binary::BinaryReader;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::{collections::BTreeMap, io, path::Path};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub enum BlackboardType {
    String,
    S32,
    U32,
    F32,
    Bool,
    Vec3f,
    VoidPtr,
}

impl BlackboardType {
    const ALL: [Self; 7] = [
        Self::String,
        Self::S32,
        Self::U32,
        Self::F32,
        Self::Bool,
        Self::Vec3f,
        Self::VoidPtr,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::String => "String",
            Self::S32 => "S32",
            Self::U32 => "U32",
            Self::F32 => "F32",
            Self::Bool => "Bool",
            Self::Vec3f => "Vec3f",
            Self::VoidPtr => "VoidPtr",
        }
    }

    fn supported(self, version: u32) -> bool {
        version >= 0x408 || self != Self::U32
    }

    fn value_size(self) -> usize {
        match self {
            Self::Vec3f => 12,
            Self::VoidPtr => 0,
            _ => 4,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlackboardParam {
    #[serde(rename = "Blackboard Index")]
    pub index: u32,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Notes")]
    pub notes: String,
    #[serde(
        rename = "Source File",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub source_file: Option<String>,
    #[serde(rename = "Inherit Mode")]
    pub inherit_mode: String,
    #[serde(rename = "Default Value")]
    pub default_value: Value,
}

pub type Blackboard = BTreeMap<String, Vec<BlackboardParam>>;

pub fn read_blackboard(
    reader: &mut BinaryReader<'_>,
    offset: usize,
    pool: usize,
    version: u32,
) -> io::Result<Blackboard> {
    reader.seek(offset)?;
    let mut headers = [(0usize, 0usize, 0usize); 7];
    for (index, kind) in BlackboardType::ALL.into_iter().enumerate() {
        if kind.supported(version) {
            headers[index] = (
                reader.read_u16()? as usize,
                reader.read_u16()? as usize,
                reader.read_u16()? as usize,
            );
            reader.read_u16()?;
        }
    }
    let mut info = vec![Vec::new(); 7];
    for (index, kind) in BlackboardType::ALL.into_iter().enumerate() {
        if !kind.supported(version) {
            continue;
        }
        for _ in 0..headers[index].0 {
            let flags = reader.read_u32()?;
            let source_index = (flags >> 31 != 0).then_some(((flags >> 24) & 0x7f) as usize);
            let name = reader.read_c_string_at(pool + (flags & 0x3f_ffff) as usize)?;
            let notes = read_string_offset(reader, pool)?;
            let inherit = match (flags >> 22) & 3 {
                0 => "InheritFromRoot",
                1 => "InheritFromParent",
                2 => "DontInherit",
                value => return Err(invalid(format!("unknown inherit mode {value}"))),
            };
            info[index].push((source_index, name, notes, inherit.to_owned()));
        }
    }
    let base = reader.position();
    let vec_header = headers[BlackboardType::Vec3f as usize];
    let file_refs = base + vec_header.2 + vec_header.0 * 12;
    let mut output = Blackboard::new();
    for (index, kind) in BlackboardType::ALL.into_iter().enumerate() {
        if !kind.supported(version) {
            continue;
        }
        reader.seek(base + headers[index].2)?;
        let mut params = Vec::new();
        for (param_index, (source_index, name, notes, inherit_mode)) in
            info[index].iter().enumerate()
        {
            let default_value = read_value(reader, pool, kind)?;
            let position = reader.position();
            let source_file = source_index
                .map(|source_index| {
                    reader.seek(file_refs + source_index * 0x10)?;
                    read_string_offset(reader, pool)
                })
                .transpose()?;
            reader.seek(position)?;
            params.push(BlackboardParam {
                index: param_index as u32,
                name: name.clone(),
                notes: notes.clone(),
                source_file,
                inherit_mode: inherit_mode.clone(),
                default_value,
            });
        }
        if !params.is_empty() {
            output.insert(kind.name().to_owned(), params);
        }
    }
    Ok(output)
}

pub fn binary_size(blackboard: &Blackboard, version: u32) -> usize {
    let mut references = Vec::<&str>::new();
    BlackboardType::ALL
        .into_iter()
        .filter(|kind| kind.supported(version))
        .flat_map(|kind| blackboard.get(kind.name()).into_iter().flatten())
        .map(|parameter| {
            let mut size = 8 + if parameter.source_file.is_some() {
                0x10
            } else {
                0
            };
            size += match parameter.default_value {
                Value::Sequence(_) => 12,
                Value::Null => 0,
                _ => 4,
            };
            if let Some(reference) = parameter.source_file.as_deref() {
                if references.contains(&reference) {
                    size -= 0x10;
                } else {
                    references.push(reference);
                }
            }
            size
        })
        .sum()
}

pub fn write_blackboard(
    writer: &mut AinbWriter,
    blackboard: &Blackboard,
    version: u32,
) -> io::Result<()> {
    let mut base_index = 0u16;
    let mut value_offset = 0u16;
    for kind in BlackboardType::ALL {
        if !kind.supported(version) {
            continue;
        }
        let count = blackboard.get(kind.name()).map_or(0, Vec::len) as u16;
        writer.write_u16(count);
        writer.write_u16(base_index);
        writer.write_u16(value_offset);
        writer.write_u16(0);
        base_index += count;
        value_offset += count * kind.value_size() as u16;
    }
    let mut file_references = Vec::<String>::new();
    for kind in BlackboardType::ALL {
        if !kind.supported(version) {
            continue;
        }
        for parameter in blackboard.get(kind.name()).into_iter().flatten() {
            let mut flags = writer.add_string(&parameter.name);
            let inherit = match parameter.inherit_mode.as_str() {
                "InheritFromRoot" => 0,
                "InheritFromParent" => 1,
                "DontInherit" => 2,
                other => return Err(invalid(format!("unknown inherit mode {other}"))),
            };
            flags |= inherit << 22;
            if let Some(reference) = &parameter.source_file {
                let reference_index =
                    match file_references.iter().position(|item| item == reference) {
                        Some(index) => index,
                        None => {
                            file_references.push(reference.clone());
                            file_references.len() - 1
                        }
                    };
                flags |= 1 << 31 | (reference_index as u32) << 24;
            }
            writer.write_u32(flags);
            writer.write_string_offset(&parameter.notes);
        }
    }
    for kind in BlackboardType::ALL {
        if !kind.supported(version) {
            continue;
        }
        for parameter in blackboard.get(kind.name()).into_iter().flatten() {
            write_value(writer, kind, &parameter.default_value)?;
        }
    }
    for reference in file_references {
        writer.write_string_offset(&reference);
        writer.write_u32(murmur3_32(&reference));
        let path = Path::new(&reference);
        writer.write_u32(murmur3_32(
            path.file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or(""),
        ));
        writer.write_u32(murmur3_32(
            path.extension()
                .and_then(|value| value.to_str())
                .unwrap_or(""),
        ));
    }
    Ok(())
}

fn read_value(
    reader: &mut BinaryReader<'_>,
    pool: usize,
    kind: BlackboardType,
) -> io::Result<Value> {
    Ok(match kind {
        BlackboardType::String => Value::from(read_string_offset(reader, pool)?),
        BlackboardType::S32 => Value::from(reader.read_i32()?),
        BlackboardType::U32 => Value::from(reader.read_u32()?),
        BlackboardType::F32 => Value::from(reader.read_f32()? as f64),
        BlackboardType::Bool => Value::from(reader.read_u32()? != 0),
        BlackboardType::Vec3f => Value::Sequence(vec![
            Value::from(reader.read_f32()? as f64),
            Value::from(reader.read_f32()? as f64),
            Value::from(reader.read_f32()? as f64),
        ]),
        BlackboardType::VoidPtr => Value::Null,
    })
}

fn write_value(writer: &mut AinbWriter, kind: BlackboardType, value: &Value) -> io::Result<()> {
    match kind {
        BlackboardType::String => writer.write_string_offset(
            value
                .as_str()
                .ok_or_else(|| invalid("expected blackboard string"))?,
        ),
        BlackboardType::S32 => writer.write_i32(
            value
                .as_i64()
                .and_then(|value| i32::try_from(value).ok())
                .ok_or_else(|| invalid("expected blackboard i32"))?,
        ),
        BlackboardType::U32 => writer.write_u32(
            value
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| invalid("expected blackboard u32"))?,
        ),
        BlackboardType::F32 => writer.write_f32(
            value
                .as_f64()
                .ok_or_else(|| invalid("expected blackboard f32"))? as f32,
        ),
        BlackboardType::Bool => writer.write_u32(
            value
                .as_bool()
                .ok_or_else(|| invalid("expected blackboard bool"))? as u32,
        ),
        BlackboardType::Vec3f => {
            let values = value
                .as_sequence()
                .filter(|value| value.len() == 3)
                .ok_or_else(|| invalid("expected blackboard vec3f"))?;
            for value in values {
                writer.write_f32(
                    value
                        .as_f64()
                        .ok_or_else(|| invalid("expected vec3f component"))?
                        as f32,
                );
            }
        }
        BlackboardType::VoidPtr if !value.is_null() => {
            return Err(invalid("VoidPtr default must be null"));
        }
        BlackboardType::VoidPtr => {}
    }
    Ok(())
}

fn read_string_offset(reader: &mut BinaryReader<'_>, pool: usize) -> io::Result<String> {
    let offset = reader.read_u32()? as usize;
    reader.read_c_string_at(pool + offset)
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
