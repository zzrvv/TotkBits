use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TexToGoSurface {
    pub array_level: u16,
    pub mip_level: u8,
    pub surface_count: u8,
    pub compressed_size: u32,
    pub compression_type: u32,
    #[serde(skip)]
    pub data: Vec<u8>,
}
