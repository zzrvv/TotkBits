use crate::parser::binary::BinaryReader;
use std::io;

pub(crate) const END: u32 = u32::MAX;

#[derive(Clone, Debug)]
pub(crate) struct SectionHeader {
    pub signature: String,
    pub subsection_offset: u32,
    pub next_section_offset: u32,
    pub section_offset: u32,
}

impl SectionHeader {
    pub fn read(data: &[u8], offset: usize) -> io::Result<Self> {
        let mut reader = BinaryReader::new(data);
        reader.seek(offset)?;
        let signature = std::str::from_utf8(reader.read_bytes(4)?)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
            .to_string();
        let _size = reader.read_u32()?;
        let subsection_offset = reader.read_u32()?;
        let next_section_offset = reader.read_u32()?;
        let _next_subsection_offset = reader.read_u32()?;
        let section_offset = reader.read_u32()?;
        let _unknown = reader.read_u32()?;
        let _subsection_count = reader.read_u32()?;
        Ok(Self {
            signature,
            subsection_offset,
            next_section_offset,
            section_offset,
        })
    }
}

pub(crate) fn relative(base: usize, offset: u32, what: &str) -> io::Result<usize> {
    base.checked_add(offset as usize).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{what} offset overflow"),
        )
    })
}
