use super::radix_tree::read_string;
use serde::{Deserialize, Serialize};
use std::io;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct SubTimeline {
    pub name: String,
}

impl SubTimeline {
    pub fn read(data: &[u8], offset: u64) -> io::Result<Self> {
        let mut bytes = [0; 8];
        bytes.copy_from_slice(
            data.get(offset as usize..offset as usize + 8)
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "sub-timeline pointer exceeds input",
                    )
                })?,
        );
        Ok(Self {
            name: read_string(data, u64::from_le_bytes(bytes))?,
        })
    }
}
