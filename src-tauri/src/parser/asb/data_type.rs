use serde::{Deserialize, Serialize};
use std::io;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum DataType {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "immediate_or_user")]
    ImmediateOrUser,
    #[serde(rename = "bool")]
    Bool,
    #[serde(rename = "s32")]
    S32,
    #[serde(rename = "f32")]
    F32,
    #[serde(rename = "string")]
    String,
    #[serde(rename = "vec3f")]
    Vec3f,
}
impl DataType {
    pub fn from_u16(value: u16) -> io::Result<Self> {
        Ok(match value {
            0 => Self::None,
            1 => Self::ImmediateOrUser,
            2 => Self::Bool,
            3 => Self::S32,
            4 => Self::F32,
            5 => Self::String,
            6 => Self::Vec3f,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid EXB data type {value}"),
                ))
            }
        })
    }
    pub fn as_u16(self) -> u16 {
        self as u16
    }
    pub fn byte_size(self) -> u32 {
        if self == Self::Vec3f {
            12
        } else {
            4
        }
    }
}
