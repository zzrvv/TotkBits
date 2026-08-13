use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TexToGoHeader {
    pub header_size: u16,
    pub version: u16,
    pub width: u16,
    pub height: u16,
    pub depth: u16,
    pub mip_count: u8,
    pub format_flag: u8,
    pub format_setting: u32,
    pub component_selectors: [u8; 4],
    pub hash: [u8; 32],
    pub format: u16,
    pub texture_settings: [u32; 4],
}
