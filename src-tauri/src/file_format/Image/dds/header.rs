use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DdsHeader {
    pub width: u32,
    pub height: u32,
    pub mipmap_count: u32,
}

impl DdsHeader {
    pub fn parse(data: &[u8]) -> io::Result<Self> {
        if data.len() < 128 || !data.starts_with(b"DDS ") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid DDS header",
            ));
        }
        let read = |offset: usize| u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
        Ok(Self {
            height: read(12),
            width: read(16),
            mipmap_count: read(28).max(1),
        })
    }
}
