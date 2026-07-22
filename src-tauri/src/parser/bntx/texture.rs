use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BntxTexture {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub array_length: u32,
    pub mip_count: u16,
    pub format: u32,
    pub image_size: u32,
    pub alignment: u32,
    pub tile_mode: u16,
    pub swizzle: u16,
    pub block_height_log2: u8,
    pub channel_types: [u8; 4],
    pub data_offsets: Vec<u64>,
    pub name_offset: usize,
    pub name_capacity: usize,
    pub format_offset: usize,
}
