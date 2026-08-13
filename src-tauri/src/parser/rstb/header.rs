use super::{dynamic_header::DynamicHeader, fixed_header::FixedHeader};
#[derive(Clone, Copy, Debug)]
pub enum Header {
    Fixed(FixedHeader),
    Dynamic(DynamicHeader),
}
impl Header {
    pub fn counts(self) -> (u32, u32) {
        match self {
            Self::Fixed(h) => (h.hash_count, h.overflow_count),
            Self::Dynamic(h) => (h.hash_count, h.overflow_count),
        }
    }
    pub fn key_size(self) -> usize {
        match self {
            Self::Fixed(_) => FixedHeader::KEY_SIZE,
            Self::Dynamic(h) => h.key_size as usize,
        }
    }
    pub fn size(self) -> usize {
        match self {
            Self::Fixed(_) => FixedHeader::SIZE,
            Self::Dynamic(_) => DynamicHeader::SIZE,
        }
    }
}
