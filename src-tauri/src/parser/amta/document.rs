use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct AmtaChunk {
    pub magic: String,
    pub offset: usize,
    pub size: usize,
    pub strings: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AmtaFile {
    pub version: u8,
    pub byte_order: String,
    pub file_size: usize,
    pub name: String,
    pub section_offsets: Vec<usize>,
    pub chunks: Vec<AmtaChunk>,
}

impl AmtaFile {
    pub fn parse(data: &[u8]) -> Result<Self, String> {
        if !data.starts_with(b"AMTA") || data.len() < 0x1c {
            return Err("not an AMTA file".into());
        }
        let little = match data.get(4..6) {
            Some(b"\xff\xfe") => true,
            Some(b"\xfe\xff") => false,
            _ => return Err("invalid AMTA byte order".into()),
        };
        let read_u32 = |at: usize| -> Result<u32, String> {
            let value = data.get(at..at + 4).ok_or("truncated AMTA header")?;
            Ok(if little {
                u32::from_le_bytes(value.try_into().map_err(|_| "truncated AMTA value")?)
            } else {
                u32::from_be_bytes(value.try_into().map_err(|_| "truncated AMTA value")?)
            })
        };
        let version = *data.get(7).ok_or("truncated AMTA version")?;
        let declared = read_u32(8)? as usize;
        let file_size = if declared == 0 {
            data.len()
        } else {
            declared.min(data.len())
        };
        let offset_positions: &[usize] = if version == 5 && data.len() >= 0x28 {
            &[0x18, 0x1c, 0x20, 0x24]
        } else {
            &[0x0c, 0x10, 0x14, 0x18]
        };
        let mut section_offsets = Vec::new();
        for &at in offset_positions {
            let offset = read_u32(at)? as usize;
            if offset > 0 && offset < file_size && !section_offsets.contains(&offset) {
                section_offsets.push(offset);
            }
        }
        section_offsets.sort_unstable();
        let mut chunks = Vec::new();
        for (index, &offset) in section_offsets.iter().enumerate() {
            let magic = data
                .get(offset..offset + 4)
                .filter(|value| value.iter().all(u8::is_ascii_graphic))
                .map(|value| String::from_utf8_lossy(value).into_owned())
                .unwrap_or_else(|| "DATA".into());
            let next = section_offsets.get(index + 1).copied().unwrap_or(file_size);
            let declared_chunk = data
                .get(offset + 4..offset + 8)
                .map(|value| {
                    if little {
                        u32::from_le_bytes(value.try_into().unwrap_or([0; 4]))
                    } else {
                        u32::from_be_bytes(value.try_into().unwrap_or([0; 4]))
                    }
                })
                .unwrap_or(0) as usize;
            let end = offset
                .checked_add(declared_chunk)
                .filter(|&end| end > offset && end <= file_size)
                .unwrap_or(next);
            let payload_start = (offset + 8).min(end);
            let strings = strings(&data[payload_start..end]);
            chunks.push(AmtaChunk {
                magic,
                offset,
                size: end - offset,
                strings,
            });
        }
        let explicit_name = if version == 5 {
            read_u32(0x24)
                .ok()
                .and_then(|offset| (offset as usize).checked_add(0x24))
        } else {
            read_u32(0x18)
                .ok()
                .and_then(|offset| (offset as usize).checked_add(8))
        }
        .filter(|&start| start < file_size)
        .and_then(|start| {
            let end = data[start..file_size]
                .iter()
                .position(|&byte| byte == 0)
                .map(|length| start + length)
                .unwrap_or(file_size);
            let value = &data[start..end];
            (!value.is_empty()
                && value
                    .iter()
                    .all(|byte| byte.is_ascii_graphic() || *byte == b' '))
            .then(|| String::from_utf8_lossy(value).into_owned())
        });
        let name = explicit_name
            .or_else(|| {
                chunks
                    .iter()
                    .find(|chunk| chunk.magic == "STRG")
                    .and_then(|chunk| chunk.strings.first())
                    .cloned()
                    .or_else(|| {
                        chunks
                            .iter()
                            .flat_map(|chunk| chunk.strings.iter())
                            .next()
                            .cloned()
                    })
            })
            .unwrap_or_default();
        Ok(Self {
            version,
            byte_order: if little { "Little" } else { "Big" }.into(),
            file_size,
            name,
            section_offsets,
            chunks,
        })
    }
}

fn strings(data: &[u8]) -> Vec<String> {
    data.split(|&byte| byte == 0)
        .filter(|value| {
            value.len() >= 2
                && value
                    .iter()
                    .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
        })
        .map(|value| String::from_utf8_lossy(value).into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::AmtaFile;

    #[test]
    fn parses_amta_entries_from_bars_corpus() {
        use crate::file_format::Archive::{ArchiveCodec, Bars::BarsFile};
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp/bars");
        let mut count = 0;
        for path in std::fs::read_dir(root)
            .unwrap()
            .flatten()
            .map(|item| item.path())
            .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("bars"))
        {
            let archive = BarsFile::from_bytes(&std::fs::read(&path).unwrap()).unwrap();
            for (name, bytes) in archive
                .entries()
                .iter()
                .filter(|(name, _)| name.ends_with(".amta"))
            {
                let parsed = AmtaFile::parse(bytes)
                    .unwrap_or_else(|error| panic!("{}::{name}: {error}", path.display()));
                assert!(!parsed.name.is_empty(), "{}::{name}", path.display());
                count += 1;
            }
        }
        assert!(count > 0);
    }
}
