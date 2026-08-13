use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AinbSection {
    pub name: String,
    pub offset: u32,
    pub size: u32,
}

impl AinbSection {
    pub fn new(name: &str, offset: usize, data: &[u8]) -> Self {
        Self {
            name: name.into(),
            offset: offset as u32,
            size: data.len() as u32,
        }
    }
}
