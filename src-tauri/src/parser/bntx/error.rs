use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BntxError {
    pub offset: usize,
    pub message: String,
}

impl BntxError {
    pub(super) fn new(offset: usize, message: impl Into<String>) -> Self {
        Self {
            offset,
            message: message.into(),
        }
    }
}

impl fmt::Display for BntxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BNTX error at 0x{:X}: {}", self.offset, self.message)
    }
}

impl std::error::Error for BntxError {}
