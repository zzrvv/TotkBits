use crate::parser::binary::BinaryReader;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::{collections::BTreeMap, io};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileReference {
    #[serde(rename = "Filename")]
    pub filename: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlackboardParameter {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Init Value")]
    pub init_value: Value,
    #[serde(rename = "File Reference", skip_serializing_if = "Option::is_none")]
    pub file_reference: Option<FileReference>,
    #[serde(skip)]
    reference_index: Option<usize>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LocalBlackboard(pub BTreeMap<String, Vec<BlackboardParameter>>);

#[derive(Clone, Copy)]
struct Header {
    count: u16,
    offset: u16,
}

impl LocalBlackboard {
    pub fn read(reader: &mut BinaryReader<'_>, pool: &BinaryReader<'_>) -> io::Result<Self> {
        const TYPES: [&str; 6] = ["string", "int", "float", "bool", "vec3f", "userdefined"];
        let mut headers = Vec::new();
        for _ in TYPES {
            let count = reader.read_u16()?;
            reader.read_u16()?;
            let offset = reader.read_u16()?;
            reader.read_u16()?;
            headers.push(Header { count, offset });
        }
        let mut groups: Vec<Vec<BlackboardParameter>> = Vec::new();
        let mut max_index = 0usize;
        for header in &headers {
            let mut group = Vec::new();
            for _ in 0..header.count {
                let bits = reader.read_u32()?;
                let reference_index = if bits >> 31 != 0 {
                    let i = ((bits >> 24) & 0x7f) as usize;
                    max_index = max_index.max(i);
                    Some(i)
                } else {
                    None
                };
                group.push(BlackboardParameter {
                    name: pool.read_c_string_at((bits & 0x3f_ffff) as usize)?,
                    init_value: Value::Null,
                    file_reference: None,
                    reference_index,
                });
            }
            groups.push(group);
        }
        let values_base = reader.position();
        for (kind, (header, group)) in TYPES.iter().zip(headers.iter().zip(groups.iter_mut())) {
            reader.seek(values_base + header.offset as usize)?;
            for entry in group {
                entry.init_value = read_initial(reader, pool, kind)?;
            }
        }
        let mut refs = Vec::new();
        for _ in 0..=max_index {
            refs.push(FileReference {
                filename: pool.read_c_string_at(reader.read_u32()? as usize)?,
            });
            reader.skip(12)?;
        }
        let mut result = BTreeMap::new();
        for (kind, mut group) in TYPES.into_iter().zip(groups) {
            for entry in &mut group {
                if let Some(index) = entry.reference_index {
                    entry.file_reference = Some(
                        refs.get(index)
                            .ok_or_else(|| {
                                io::Error::new(
                                    io::ErrorKind::InvalidData,
                                    "ASB blackboard reference index exceeds table",
                                )
                            })?
                            .clone(),
                    );
                }
            }
            if !group.is_empty() {
                result.insert(kind.into(), group);
            }
        }
        Ok(Self(result))
    }
}

fn read_initial(
    reader: &mut BinaryReader<'_>,
    pool: &BinaryReader<'_>,
    kind: &str,
) -> io::Result<Value> {
    Ok(match kind {
        "string" => Value::String(pool.read_c_string_at(reader.read_u32()? as usize)?),
        "int" => serde_yaml::to_value(reader.read_u32()?).map_err(io::Error::other)?,
        "float" => serde_yaml::to_value(reader.read_f32()?).map_err(io::Error::other)?,
        "bool" => Value::Bool(reader.read_u32()? != 0),
        "vec3f" => {
            serde_yaml::to_value([reader.read_f32()?, reader.read_f32()?, reader.read_f32()?])
                .map_err(io::Error::other)?
        }
        "userdefined" => Value::Null,
        _ => return Err(super::parameter::invalid_type(kind)),
    })
}
