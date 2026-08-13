#[derive(Clone, Copy, Debug)]
pub struct FixedHeader {
    pub hash_count: u32,
    pub overflow_count: u32,
}
impl FixedHeader {
    pub const SIZE: usize = 12;
    pub const KEY_SIZE: usize = 128;
}
