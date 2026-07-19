use crate::parser::binary::{BinaryReader, BinaryWriter, Endian};
use std::io::{self, ErrorKind};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    pub name: String,
    pub index: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LabelSection {
    pub group_count: u32,
    pub labels: Vec<Label>,
}
pub fn hash_label(s: &str) -> u32 {
    s.as_bytes()
        .iter()
        .fold(0u32, |h, b| h.wrapping_mul(0x492).wrapping_add(*b as u32))
}
impl LabelSection {
    pub fn write(&self, e: Endian) -> io::Result<Vec<u8>> {
        if self.group_count == 0 {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "LBL1 requires at least one group",
            ));
        }
        let groups = self.group_count as usize;
        let mut buckets = vec![Vec::new(); groups];
        for label in &self.labels {
            buckets[(hash_label(&label.name) as usize) % groups].push(label);
        }
        let mut writer = BinaryWriter::with_endian(e);
        writer.write_u32(self.group_count);
        let mut offset = 4 + groups * 8;
        for bucket in &buckets {
            writer.write_u32(bucket.len() as u32);
            writer.write_u32(offset as u32);
            offset += bucket.iter().map(|x| 1 + x.name.len() + 4).sum::<usize>();
        }
        for bucket in buckets {
            for label in bucket {
                if label.name.len() > 255 {
                    return Err(io::Error::new(
                        ErrorKind::InvalidData,
                        "LBL1 label too long",
                    ));
                }
                writer.write_u8(label.name.len() as u8);
                writer.write_bytes(label.name.as_bytes());
                writer.write_u32(label.index);
            }
        }
        Ok(writer.into_inner())
    }
    pub fn read(data: &[u8], e: Endian) -> io::Result<Self> {
        let mut r = BinaryReader::with_endian(data, e);
        let n = r.read_u32()? as usize;
        if n > data.len() / 8 {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "LBL1 group count out of bounds",
            ));
        }
        let mut desc = Vec::with_capacity(n);
        for _ in 0..n {
            desc.push((r.read_u32()? as usize, r.read_u32()? as usize));
        }
        let mut labels = Vec::new();
        for (count, off) in desc {
            let mut q = BinaryReader::with_endian(data, e);
            q.seek(off)?;
            for _ in 0..count {
                let len = q.read_u8()? as usize;
                let name = String::from_utf8(q.read_bytes(len)?.to_vec()).map_err(|_| {
                    io::Error::new(ErrorKind::InvalidData, "LBL1 label is not UTF-8")
                })?;
                labels.push(Label {
                    name,
                    index: q.read_u32()?,
                });
            }
        }
        Ok(Self {
            group_count: n as u32,
            labels,
        })
    }
}
