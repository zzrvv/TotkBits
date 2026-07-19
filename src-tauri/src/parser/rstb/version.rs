#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RstbVersion {
    Fixed,
    Dynamic(u32),
}
