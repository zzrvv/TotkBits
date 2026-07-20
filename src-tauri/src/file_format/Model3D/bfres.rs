//! Read-only parser for Nintendo BFRES resource containers.
//!
//! BFRES stores its object graph as relocated pointers.  This module parses the
//! container header and inventories the typed resource sections without tying
//! the result to TotkBits' document/YAML representation.

use std::{fmt, fs, path::Path};

const SECTION_SIGNATURES: &[&[u8; 4]] = &[
    b"FMDL", b"FSKL", b"FVTX", b"FSHP", b"FMAT", b"FSKA", b"FSHU", b"FSHA", b"FSCN", b"FTXP",
    b"FVIS", b"FMAA", b"FREL",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endian {
    Little,
    Big,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BfresHeader {
    pub version: [u8; 4],
    pub endian: Endian,
    pub alignment_exponent: u8,
    pub target_address_size: u8,
    pub name_offset: u32,
    pub flags: u16,
    pub block_offset: u16,
    pub relocation_table_offset: u32,
    /// Size used by BFRES itself. Files are commonly padded beyond this value.
    pub file_size: u32,
    pub string_pool_size: u32,
    pub string_pool_offset: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BfresSection {
    pub signature: [u8; 4],
    pub offset: u64,
    pub name: Option<String>,
}

impl BfresSection {
    pub fn signature_str(&self) -> &str {
        std::str::from_utf8(&self.signature).unwrap_or("????")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BfresFile {
    pub header: BfresHeader,
    pub name: Option<String>,
    pub sections: Vec<BfresSection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BfresError {
    pub offset: usize,
    pub message: String,
}

impl BfresError {
    fn new(offset: usize, message: impl Into<String>) -> Self {
        Self {
            offset,
            message: message.into(),
        }
    }
}

impl fmt::Display for BfresError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BFRES error at 0x{:X}: {}", self.offset, self.message)
    }
}

impl std::error::Error for BfresError {}

impl BfresFile {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, BfresError> {
        let data = fs::read(path.as_ref())
            .map_err(|error| BfresError::new(0, format!("failed to read file: {error}")))?;
        Self::from_bytes(&data)
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self, BfresError> {
        if data.len() < 0x30 {
            return Err(BfresError::new(0, "header is truncated"));
        }
        if &data[..4] != b"FRES" {
            return Err(BfresError::new(0, "invalid FRES signature"));
        }

        let endian = match &data[0x0C..0x0E] {
            [0xFF, 0xFE] => Endian::Little,
            [0xFE, 0xFF] => Endian::Big,
            _ => return Err(BfresError::new(0x0C, "invalid byte-order mark")),
        };
        let read_u16 = |offset| u16_at(data, offset, endian);
        let read_u32 = |offset| u32_at(data, offset, endian);

        let header = BfresHeader {
            version: data[8..12].try_into().unwrap(),
            endian,
            alignment_exponent: data[0x0E],
            target_address_size: data[0x0F],
            name_offset: read_u32(0x10)?,
            flags: read_u16(0x14)?,
            block_offset: read_u16(0x16)?,
            relocation_table_offset: read_u32(0x18)?,
            file_size: read_u32(0x1C)?,
            string_pool_size: read_u32(0x20)?,
            string_pool_offset: read_u32(0x24)?,
        };

        if header.file_size as usize > data.len() {
            return Err(BfresError::new(0x1C, "declared file size exceeds input"));
        }
        if header.target_address_size != 0
            && header.target_address_size != 4
            && header.target_address_size != 8
        {
            return Err(BfresError::new(0x0F, "unsupported target address size"));
        }

        let name = read_string(data, header.name_offset as u64);
        let mut sections = Vec::new();
        for offset in (0..data.len().saturating_sub(3)).filter(|offset| offset % 4 == 0) {
            let signature: [u8; 4] = data[offset..offset + 4].try_into().unwrap();
            if !SECTION_SIGNATURES
                .iter()
                .any(|candidate| **candidate == signature)
            {
                continue;
            }
            // All resource sections begin with their signature, four reserved
            // bytes, then an absolute pointer to their ResString name. FREL is
            // the sole unnamed container-level section.
            let section_name = if &signature == b"FREL" {
                None
            } else if header.target_address_size != 4 {
                u64_at(data, offset + 8, endian)
                    .ok()
                    .and_then(|pointer| read_string(data, pointer))
            } else {
                u32_at(data, offset + 8, endian)
                    .ok()
                    .and_then(|pointer| read_string(data, pointer as u64))
            };
            sections.push(BfresSection {
                signature,
                offset: offset as u64,
                name: section_name,
            });
        }

        if sections.is_empty() {
            return Err(BfresError::new(0, "no BFRES resource sections found"));
        }
        Ok(Self {
            header,
            name,
            sections,
        })
    }

    pub fn sections_with_signature<'a>(
        &'a self,
        signature: &'a [u8; 4],
    ) -> impl Iterator<Item = &'a BfresSection> + 'a {
        self.sections
            .iter()
            .filter(move |section| &section.signature == signature)
    }
}

fn u16_at(data: &[u8], offset: usize, endian: Endian) -> Result<u16, BfresError> {
    let bytes: [u8; 2] = data
        .get(offset..offset + 2)
        .ok_or_else(|| BfresError::new(offset, "truncated u16"))?
        .try_into()
        .unwrap();
    Ok(match endian {
        Endian::Little => u16::from_le_bytes(bytes),
        Endian::Big => u16::from_be_bytes(bytes),
    })
}

fn u32_at(data: &[u8], offset: usize, endian: Endian) -> Result<u32, BfresError> {
    let bytes: [u8; 4] = data
        .get(offset..offset + 4)
        .ok_or_else(|| BfresError::new(offset, "truncated u32"))?
        .try_into()
        .unwrap();
    Ok(match endian {
        Endian::Little => u32::from_le_bytes(bytes),
        Endian::Big => u32::from_be_bytes(bytes),
    })
}

fn u64_at(data: &[u8], offset: usize, endian: Endian) -> Result<u64, BfresError> {
    let bytes: [u8; 8] = data
        .get(offset..offset + 8)
        .ok_or_else(|| BfresError::new(offset, "truncated u64"))?
        .try_into()
        .unwrap();
    Ok(match endian {
        Endian::Little => u64::from_le_bytes(bytes),
        Endian::Big => u64::from_be_bytes(bytes),
    })
}

fn read_string(data: &[u8], offset: u64) -> Option<String> {
    let offset = usize::try_from(offset).ok()?;
    if offset == 0 || offset >= data.len() {
        return None;
    }
    let tail = &data[offset..];
    let end = tail.iter().position(|byte| *byte == 0)?;
    let bytes = &tail[..end];
    if bytes.is_empty()
        || bytes.len() > 0x1000
        || bytes.iter().any(|byte| *byte < 0x20 && *byte != b'\t')
    {
        return None;
    }
    std::str::from_utf8(bytes).ok().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_bfres_data() {
        assert!(BfresFile::from_bytes(b"not a bfres file").is_err());
    }

    #[test]
    fn parses_bfres_corpus() {
        let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp/bfres");
        if !corpus.is_dir() {
            return;
        }
        let mut parsed = 0;
        for entry in fs::read_dir(corpus).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|value| value.to_str()) != Some("bfres") {
                continue;
            }
            let bfres = BfresFile::from_path(&path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert_eq!(bfres.header.endian, Endian::Little);
            assert!(bfres.sections_with_signature(b"FMDL").next().is_some());
            assert!(bfres.sections_with_signature(b"FSKL").next().is_some());
            assert!(bfres.sections_with_signature(b"FVTX").next().is_some());
            assert!(bfres.sections_with_signature(b"FSHP").next().is_some());
            assert!(bfres.sections_with_signature(b"FMAT").next().is_some());
            parsed += 1;
        }
        assert!(parsed > 0, "BFRES corpus is empty");
    }
}
