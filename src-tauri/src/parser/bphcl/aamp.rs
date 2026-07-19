use std::io::{self, ErrorKind};
#[derive(Clone, Debug)]
pub struct AampSection {
    pub offset: usize,
    pub size: usize,
    pub raw: Vec<u8>,
}
impl AampSection {
    pub fn read(data: &[u8], offset: u32, size: u32) -> io::Result<Option<Self>> {
        if size == 0 {
            return Ok(None);
        }
        let o = offset as usize;
        let s = size as usize;
        let raw = data
            .get(
                o..o.checked_add(s)
                    .ok_or_else(|| io::Error::new(ErrorKind::InvalidData, "AAMP range overflow"))?,
            )
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidData, "AAMP outside BPHCL"))?;
        if !raw.starts_with(b"AAMP") {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "parameter section is not AAMP",
            ));
        }
        Ok(Some(Self {
            offset: o,
            size: s,
            raw: raw.to_vec(),
        }))
    }
}
