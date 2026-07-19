#[derive(Clone, Copy, Debug)]
pub struct DynamicHeader {
    pub version: u32,
    pub key_size: u32,
    pub hash_count: u32,
    pub overflow_count: u32,
}
impl DynamicHeader {
    pub const SIZE: usize = 22;
}
