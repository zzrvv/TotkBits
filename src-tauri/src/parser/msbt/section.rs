#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub magic: [u8; 4],
    pub reserved: [u8; 8],
    pub data: Vec<u8>,
    pub padding: Vec<u8>,
}
impl Section {
    pub fn name(&self) -> String {
        String::from_utf8_lossy(&self.magic).into_owned()
    }
}
