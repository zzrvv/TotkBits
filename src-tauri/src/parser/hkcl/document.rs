use super::{header::HEADER_SIZE, section::SECTION_HEADER_SIZE, HkclHeader, HkclSection};
use crate::parser::binary::BinaryReader;
use std::io::{self, ErrorKind};

#[derive(Clone, Debug)]
pub struct HkclDocument {
    pub raw: Vec<u8>,
    pub header: HkclHeader,
    pub sections: Vec<HkclSection>,
    pub contents_offset: Option<usize>,
    pub contents_class_name_offset: Option<usize>,
    pub contents_class_name: Option<String>,
}

impl HkclDocument {
    pub fn parse(data: &[u8]) -> io::Result<Self> {
        let header = HkclHeader::read(data)?;
        let sections = (0..header.section_count)
            .map(|index| {
                HkclSection::read(
                    data,
                    HEADER_SIZE + index * SECTION_HEADER_SIZE,
                    header.layout.endian,
                )
            })
            .collect::<io::Result<Vec<_>>>()?;
        let contents_offset = resolve_offset(
            &sections,
            header.contents_section_index,
            header.contents_section_offset,
            "contents",
        )?;
        let contents_class_name_offset = resolve_offset(
            &sections,
            header.contents_class_name_section_index,
            header.contents_class_name_section_offset,
            "contents class name",
        )?;
        let contents_class_name = contents_class_name_offset
            .map(|offset| BinaryReader::new(data).read_c_string_at(offset))
            .transpose()?;
        Ok(Self {
            raw: data.to_vec(),
            header,
            sections,
            contents_offset,
            contents_class_name_offset,
            contents_class_name,
        })
    }

    pub fn section(&self, tag: &str) -> Option<&HkclSection> {
        self.sections.iter().find(|section| section.tag == tag)
    }
}

fn resolve_offset(
    sections: &[HkclSection],
    section_index: Option<usize>,
    relative_offset: u32,
    name: &str,
) -> io::Result<Option<usize>> {
    let Some(section_index) = section_index else {
        return Ok(None);
    };
    let section = &sections[section_index];
    let absolute = section
        .absolute_data_start
        .checked_add(relative_offset as usize)
        .ok_or_else(|| invalid(&format!("HKCL {name} offset overflows")))?;
    if absolute >= section.end {
        return Err(invalid(&format!("HKCL {name} offset exceeds its section")));
    }
    Ok(Some(absolute))
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::binary::{BinaryWriter, Endian};

    fn packfile(endian: Endian) -> Vec<u8> {
        let mut writer = BinaryWriter::with_endian(endian);
        writer.write_bytes(&[0x57, 0xe0, 0xe0, 0x57, 0x10, 0xc0, 0xc0, 0x10]);
        writer.write_u32(0);
        writer.write_u32(11);
        writer.write_bytes(&[
            if endian == Endian::Little { 8 } else { 4 },
            if endian == Endian::Little { 1 } else { 0 },
            1,
            1,
        ]);
        writer.write_i32(1);
        writer.write_i32(0);
        writer.write_u32(0);
        writer.write_i32(0);
        writer.write_u32(5);
        let mut version = [0u8; 16];
        let text = b"hk_2014.1.0-r1";
        version[..text.len()].copy_from_slice(text);
        writer.write_bytes(&version);
        writer.write_u32(0);
        writer.write_u16(0);
        writer.write_u16(0);
        let mut tag = [0u8; 20];
        tag[..9].copy_from_slice(b"__data__\0");
        writer.write_bytes(&tag);
        writer.write_u32(0x70);
        writer.write_u32(0x10);
        writer.write_u32(0x10);
        writer.write_u32(0x10);
        writer.write_u32(0x10);
        writer.write_u32(0x10);
        writer.write_u32(0x10);
        writer.write_bytes(b"ROOT\0hkRoot\0\0\0\0\0");
        let bytes = writer.into_inner();
        assert_eq!(bytes.len(), 0x80);
        bytes
    }

    #[test]
    fn parses_little_and_big_endian_packfile_headers() {
        for endian in [Endian::Little, Endian::Big] {
            let document = HkclDocument::parse(&packfile(endian)).unwrap();
            assert_eq!(document.header.layout.endian, endian);
            assert_eq!(document.header.file_version, 11);
            assert_eq!(document.header.contents_version, "hk_2014.1.0-r1");
            assert_eq!(document.contents_offset, Some(0x70));
            assert_eq!(document.contents_class_name.as_deref(), Some("hkRoot"));
            assert_eq!(document.sections[0].data(), 0x70..0x80);
        }
    }

    #[test]
    fn rejects_non_monotonic_section_ranges() {
        let mut bytes = packfile(Endian::Little);
        bytes[0x58..0x5c].copy_from_slice(&0x18u32.to_le_bytes());
        assert!(HkclDocument::parse(&bytes).is_err());
    }
}
