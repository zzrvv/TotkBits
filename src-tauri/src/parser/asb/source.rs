use serde::{Deserialize, Serialize};
use std::io;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Source {
    Imm,
    ImmStr,
    StaticMem,
    ParamTbl,
    ParamTblStr,
    Output,
    Input,
    Scratch32,
    Scratch64,
    UserOut,
    UserIn,
}
impl Source {
    pub fn from_u8(value: u8) -> io::Result<Self> {
        Ok(match value {
            0 => Self::Imm,
            1 => Self::ImmStr,
            2 => Self::StaticMem,
            3 => Self::ParamTbl,
            4 => Self::ParamTblStr,
            5 => Self::Output,
            6 => Self::Input,
            7 => Self::Scratch32,
            8 => Self::Scratch64,
            9 => Self::UserOut,
            10 => Self::UserIn,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid EXB source {value}"),
                ))
            }
        })
    }
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}
