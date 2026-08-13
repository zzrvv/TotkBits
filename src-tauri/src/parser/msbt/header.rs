use crate::parser::binary::{BinaryReader, BinaryWriter, Endian};
use std::io::{self, ErrorKind};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub endian: Endian,
    pub unknown: u16,
    pub encoding: u8,
    pub version: u8,
    pub section_count: u16,
    pub reserved: u16,
    pub file_size: u32,
    pub padding: [u8; 10],
}
impl Header {
    pub fn read(data: &[u8]) -> io::Result<Self> {
        if data.len() < 32 || &data[..8] != b"MsgStdBn" {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "not a MsgStdBn file",
            ));
        }
        let endian = match &data[8..10] {
            [0xff, 0xfe] => Endian::Little,
            [0xfe, 0xff] => Endian::Big,
            _ => return Err(io::Error::new(ErrorKind::InvalidData, "invalid MSBT BOM")),
        };
        let mut r = BinaryReader::with_endian(data, endian);
        r.seek(10)?;
        let unknown = r.read_u16()?;
        let encoding = r.read_u8()?;
        let version = r.read_u8()?;
        let section_count = r.read_u16()?;
        let reserved = r.read_u16()?;
        let file_size = r.read_u32()?;
        let mut padding = [0; 10];
        padding.copy_from_slice(r.read_bytes(10)?);
        if file_size as usize > data.len() || file_size < 32 {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "invalid MSBT file size",
            ));
        }
        Ok(Self {
            endian,
            unknown,
            encoding,
            version,
            section_count,
            reserved,
            file_size,
            padding,
        })
    }
    pub fn write(&self, w: &mut BinaryWriter, file_size: u32, sections: u16) {
        w.write_bytes(b"MsgStdBn");
        match self.endian {
            Endian::Little => w.write_bytes(&[0xff, 0xfe]),
            Endian::Big => w.write_bytes(&[0xfe, 0xff]),
        };
        w.write_u16(self.unknown);
        w.write_u8(self.encoding);
        w.write_u8(self.version);
        w.write_u16(sections);
        w.write_u16(self.reserved);
        w.write_u32(file_size);
        w.write_bytes(&self.padding);
    }
}
