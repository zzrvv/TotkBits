use std::{fmt, io};
#[derive(Debug)]
pub enum RstbError {
    InvalidMagic,
    UnsupportedVersion(u32),
    InvalidLength { expected: usize, actual: usize },
    InvalidKeySize(u32),
    InvalidUtf8(String),
    Overflow(&'static str),
    Io(io::Error),
}
impl fmt::Display for RstbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMagic => write!(f, "invalid RSTB/RESTBL magic"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported RESTBL version {v}"),
            Self::InvalidLength { expected, actual } => {
                write!(f, "invalid length: expected {expected} bytes, got {actual}")
            }
            Self::InvalidKeySize(v) => write!(f, "invalid RESTBL key slot size {v}"),
            Self::InvalidUtf8(v) => write!(f, "invalid overflow key UTF-8: {v}"),
            Self::Overflow(v) => write!(f, "integer overflow calculating {v}"),
            Self::Io(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for RstbError {}
impl From<io::Error> for RstbError {
    fn from(v: io::Error) -> Self {
        Self::Io(v)
    }
}
