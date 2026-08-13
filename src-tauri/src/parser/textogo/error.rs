use std::fmt;

#[derive(Debug)]
pub struct TexToGoError {
    pub offset: usize,
    pub message: String,
}
impl TexToGoError {
    pub(super) fn new(offset: usize, message: impl Into<String>) -> Self {
        Self {
            offset,
            message: message.into(),
        }
    }
}
impl fmt::Display for TexToGoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TexToGo error at 0x{:X}: {}", self.offset, self.message)
    }
}
impl std::error::Error for TexToGoError {}
