use crate::parser::binary::BinaryReader;
use serde_yaml::{Mapping, Value};
use std::io::{self, ErrorKind};

#[derive(Clone, Copy, Debug)]
pub enum ParameterType {
    String,
    Int,
    Float,
    Bool,
    Vec3f,
}

impl ParameterType {
    fn name(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Int => "int",
            Self::Float => "float",
            Self::Bool => "bool",
            Self::Vec3f => "vec3f",
        }
    }
}

pub fn read_parameter(
    reader: &mut BinaryReader<'_>,
    pool: &BinaryReader<'_>,
    ty: ParameterType,
) -> io::Result<Value> {
    let flags = reader.read_i32()?;
    if flags >= 0 {
        return read_value(reader, pool, ty);
    }
    let bits = flags as u32;
    let index = bits & 0xffff;
    let flag = (bits & 0xffff_0000) >> 16;
    let mut map = Mapping::new();
    if ((bits ^ u32::MAX) & 0x8100_0000) == 0 {
        insert(&mut map, "EXB Index", index);
    } else if !matches!(ty, ParameterType::Float | ParameterType::Vec3f) {
        insert(&mut map, "Flags", format!("{flag:#x}"));
        insert(&mut map, "Type", ty.name());
        insert(&mut map, "Local Blackboard Index", index);
    } else if (flag >> 0xe) < 3 || ((flag >> 8) & 1) != 0 {
        insert(&mut map, "Flags", format!("{flag:#x}"));
        if ((flag >> 9) & 1) == 0 {
            insert(&mut map, "Type", ty.name());
            insert(&mut map, "Local Blackboard Index", index);
        } else {
            insert(&mut map, "Index", index);
        }
    } else {
        insert(&mut map, "Flags", format!("{flag:#x}"));
        insert(&mut map, "Index", index);
    }
    let default = read_value(reader, pool, ty)?;
    if truthy(&default) {
        map.insert(Value::String("Default Value".into()), default);
    }
    Ok(Value::Mapping(map))
}

fn read_value(
    reader: &mut BinaryReader<'_>,
    pool: &BinaryReader<'_>,
    ty: ParameterType,
) -> io::Result<Value> {
    Ok(match ty {
        ParameterType::String => {
            let offset = reader.read_u32()? as usize;
            // The reference parser treats selector padding/unused string
            // defaults that point beyond the pool as an empty string.
            Value::String(if offset >= pool.len() {
                String::new()
            } else {
                pool.read_c_string_at(offset)?
            })
        }
        ParameterType::Int => serde_yaml::to_value(reader.read_i32()?).map_err(io::Error::other)?,
        ParameterType::Float => {
            serde_yaml::to_value(reader.read_f32()?).map_err(io::Error::other)?
        }
        ParameterType::Bool => Value::Bool(reader.read_u32()? != 0),
        ParameterType::Vec3f => {
            serde_yaml::to_value([reader.read_f32()?, reader.read_f32()?, reader.read_f32()?])
                .map_err(io::Error::other)?
        }
    })
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(v) => *v,
        Value::Number(v) => v.as_f64().unwrap_or(0.0) != 0.0,
        Value::String(v) => !v.is_empty(),
        Value::Sequence(v) => !v.is_empty(),
        Value::Mapping(v) => !v.is_empty(),
        _ => true,
    }
}
fn insert<T: serde::Serialize>(map: &mut Mapping, key: &str, value: T) {
    map.insert(
        Value::String(key.into()),
        serde_yaml::to_value(value).unwrap_or(Value::Null),
    );
}

pub fn invalid_type(name: &str) -> io::Error {
    io::Error::new(
        ErrorKind::InvalidData,
        format!("invalid ASB parameter type {name}"),
    )
}
