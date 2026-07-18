use serde::{Deserialize, Serialize};
use std::io;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Opcode {
    Terminator,
    Store,
    Negate,
    NegateBool,
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulus,
    Increment,
    Decrement,
    ScalarMultiplyVec3f,
    ScalarDivideVec3f,
    LeftShift,
    RightShift,
    LessThan,
    LessThanEqual,
    GreaterThan,
    GreaterThanEqual,
    Equal,
    NotEqual,
    And,
    Xor,
    Or,
    LogicalAnd,
    LogicalOr,
    UserFunction,
    JumpIfLhsZero,
    Jump,
}

impl Opcode {
    pub fn from_u8(value: u8) -> io::Result<Self> {
        use Opcode::*;
        Ok(match value {
            1 => Terminator,
            2 => Store,
            3 => Negate,
            4 => NegateBool,
            5 => Add,
            6 => Subtract,
            7 => Multiply,
            8 => Divide,
            9 => Modulus,
            10 => Increment,
            11 => Decrement,
            12 => ScalarMultiplyVec3f,
            13 => ScalarDivideVec3f,
            14 => LeftShift,
            15 => RightShift,
            16 => LessThan,
            17 => LessThanEqual,
            18 => GreaterThan,
            19 => GreaterThanEqual,
            20 => Equal,
            21 => NotEqual,
            22 => And,
            23 => Xor,
            24 => Or,
            25 => LogicalAnd,
            26 => LogicalOr,
            27 => UserFunction,
            28 => JumpIfLhsZero,
            29 => Jump,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid EXB opcode {value}"),
                ))
            }
        })
    }
    pub fn as_u8(self) -> u8 {
        self as u8 + 1
    }
}
