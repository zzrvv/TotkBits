use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DdsHeader {
    pub width: u32,
    pub height: u32,
    pub mipmap_count: u32,
}

impl DdsHeader {
    pub fn parse(data: &[u8]) -> io::Result<Self> {
        if data.len() < 128 || !crate::Settings::Magic::is_dds(data) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid DDS header",
            ));
        }
        let read = |offset: usize| {
            let mut value = [0; 4];
            value.copy_from_slice(&data[offset..offset + 4]);
            u32::from_le_bytes(value)
        };
        Ok(Self {
            height: read(12),
            width: read(16),
            mipmap_count: read(28).max(1),
        })
    }
}
