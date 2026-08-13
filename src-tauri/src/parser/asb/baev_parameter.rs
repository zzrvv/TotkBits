use crate::parser::binary::BinaryReader;
use serde::{Deserialize, Serialize};
use std::io;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BaevParameter {
    Integer(u32),
    Float(f64),
    Vec3f([f64; 3]),
    String(String),
}

impl BaevParameter {
    pub fn read_at(reader: &mut BinaryReader<'_>, offset: usize) -> io::Result<Self> {
        let return_position = reader.position();
        reader.seek(offset)?;
        let parameter_type = reader.read_u32()?;
        reader.read_u32()?;
        let parameter = match parameter_type {
            0 => Self::Integer(reader.read_u32()?),
            1 => Self::Float(f64::from(reader.read_f32()?)),
            3 => Self::Vec3f([
                f64::from(reader.read_f32()?),
                f64::from(reader.read_f32()?),
                f64::from(reader.read_f32()?),
            ]),
            5 => {
                let string_offset = usize::try_from(reader.read_u64()?).map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "BAEV string offset exceeds usize",
                    )
                })?;
                Self::String(reader.read_c_string_at(string_offset)?)
            }
            value => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unsupported BAEV parameter type {value}"),
                ))
            }
        };
        reader.seek(return_position)?;
        Ok(parameter)
    }
}
