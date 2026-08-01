use crate::parser::binary::{BinaryReader, Endian};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct AmtaHeaderField {
    pub name: String,
    pub offset: usize,
    pub raw_hex: String,
    pub value: u32,
    pub target: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AmtaString {
    pub offset: usize,
    pub value: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct AmtaWord {
    pub offset: usize,
    pub raw_hex: String,
    pub unsigned: u32,
    pub signed: i32,
    pub float: Option<f32>,
    pub ascii: Option<String>,
    pub target: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AmtaChunk {
    pub magic: String,
    pub source: String,
    pub offset: usize,
    pub end_offset: usize,
    pub size: usize,
    pub alignment: usize,
    pub zero_bytes: usize,
    pub nonzero_bytes: usize,
    pub preview_hex: String,
    pub strings: Vec<String>,
    pub string_entries: Vec<AmtaString>,
    pub words: Vec<AmtaWord>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AmtaFile {
    pub magic: String,
    pub version: u8,
    pub byte_order: String,
    pub byte_order_mark: String,
    pub declared_file_size: usize,
    pub file_size: usize,
    pub trailing_bytes: usize,
    pub header_size: usize,
    pub name: String,
    pub section_offsets: Vec<usize>,
    pub header_fields: Vec<AmtaHeaderField>,
    pub strings: Vec<AmtaString>,
    pub total_strings: usize,
    pub total_words: usize,
    pub total_zero_bytes: usize,
    pub chunks: Vec<AmtaChunk>,
    pub diagnostics: Vec<String>,
}

impl AmtaFile {
    pub fn parse(data: &[u8]) -> Result<Self, String> {
        if !data.starts_with(b"AMTA") || data.len() < 0x1c {
            return Err("not an AMTA file".into());
        }
        let (little, byte_order_mark) = match data.get(4..6) {
            Some(b"\xff\xfe") => (true, "FF FE"),
            Some(b"\xfe\xff") => (false, "FE FF"),
            _ => return Err("invalid AMTA byte order".into()),
        };
        let reader =
            BinaryReader::with_endian(data, if little { Endian::Little } else { Endian::Big });
        let read_u32 = |at: usize| {
            reader
                .read_u32_at(at)
                .map_err(|error| format!("invalid AMTA value: {error}"))
        };
        let version = reader
            .read_u8_at(7)
            .map_err(|error| format!("invalid AMTA version: {error}"))?;
        let declared_file_size = read_u32(8)? as usize;
        let file_size = if declared_file_size == 0 {
            data.len()
        } else {
            declared_file_size.min(data.len())
        };
        let header_size = if version == 5 { 0x28 } else { 0x1c };
        if file_size < header_size {
            return Err(format!(
                "AMTA v{version} header exceeds declared file size {file_size:#x}"
            ));
        }

        let field_layout: &[(&str, usize, bool)] = if version == 5 {
            &[
                ("unknown_0c", 0x0c, false),
                ("data_offset", 0x10, true),
                ("section_1_offset", 0x14, true),
                ("minf_offset", 0x18, true),
                ("section_3_offset", 0x1c, true),
                ("unknown_20", 0x20, false),
                ("string_table_offset", 0x24, true),
            ]
        } else {
            &[
                ("section_0_offset", 0x0c, true),
                ("section_1_offset", 0x10, true),
                ("section_2_offset", 0x14, true),
                ("string_table_offset", 0x18, true),
            ]
        };
        let mut diagnostics = Vec::new();
        if declared_file_size != 0 && declared_file_size != data.len() {
            diagnostics.push(format!(
                "Declared size {declared_file_size:#x} differs from available size {:#x}",
                data.len()
            ));
        }
        let mut header_fields = Vec::new();
        let mut sections = Vec::<(usize, String)>::new();
        for &(name, at, is_offset) in field_layout {
            let value = read_u32(at)?;
            let target =
                (is_offset && value != 0 && (value as usize) < file_size).then_some(value as usize);
            if is_offset && value != 0 && target.is_none() {
                diagnostics.push(format!("{name} points outside the file: {value:#x}"));
            }
            if let Some(target) = target {
                sections.push((target, name.trim_end_matches("_offset").to_owned()));
            }
            header_fields.push(AmtaHeaderField {
                name: name.to_owned(),
                offset: at,
                raw_hex: format!("{value:08X}"),
                value,
                target,
            });
        }
        sections.sort_by_key(|value| value.0);
        sections.dedup_by(|left, right| {
            if left.0 == right.0 {
                left.1 = format!("{}/{}", left.1, right.1);
                true
            } else {
                false
            }
        });
        let section_offsets = sections.iter().map(|value| value.0).collect::<Vec<_>>();
        let all_strings = strings(data, 0, file_size);
        let string_table_offset = header_fields
            .iter()
            .find(|field| field.name == "string_table_offset")
            .and_then(|field| field.target);
        let name = explicit_name(data, file_size, version, string_table_offset)
            .or_else(|| {
                all_strings
                    .iter()
                    .rev()
                    .map(|value| value.value.clone())
                    .next()
            })
            .unwrap_or_default();

        let mut chunks = Vec::new();
        for (index, (offset, source)) in sections.iter().enumerate() {
            let end = sections
                .get(index + 1)
                .map(|value| value.0)
                .unwrap_or(file_size);
            if end <= *offset {
                continue;
            }
            let bytes = &data[*offset..end];
            let magic = bytes
                .get(..4)
                .filter(|value| value.iter().all(u8::is_ascii_graphic))
                .map(|value| String::from_utf8_lossy(value).into_owned())
                .unwrap_or_else(|| source.to_ascii_uppercase());
            let string_entries = strings(data, *offset, end);
            let strings = string_entries
                .iter()
                .map(|value| value.value.clone())
                .collect();
            let words = words(&reader, data, *offset, end, file_size);
            let zero_bytes = bytes.iter().filter(|&&byte| byte == 0).count();
            chunks.push(AmtaChunk {
                magic,
                source: source.clone(),
                offset: *offset,
                end_offset: end,
                size: end - offset,
                alignment: alignment(*offset),
                zero_bytes,
                nonzero_bytes: bytes.len() - zero_bytes,
                preview_hex: hex_preview(bytes, 64),
                strings,
                string_entries,
                words,
            });
        }
        let total_words = chunks.iter().map(|chunk| chunk.words.len()).sum();
        let total_zero_bytes = data[..file_size].iter().filter(|&&byte| byte == 0).count();
        Ok(Self {
            magic: "AMTA".into(),
            version,
            byte_order: if little { "Little" } else { "Big" }.into(),
            byte_order_mark: byte_order_mark.into(),
            declared_file_size,
            file_size,
            trailing_bytes: data.len().saturating_sub(file_size),
            header_size,
            name,
            section_offsets,
            header_fields,
            total_strings: all_strings.len(),
            strings: all_strings,
            total_words,
            total_zero_bytes,
            chunks,
            diagnostics,
        })
    }
}

fn explicit_name(
    data: &[u8],
    file_size: usize,
    version: u8,
    string_table_offset: Option<usize>,
) -> Option<String> {
    let start = if version == 5 {
        string_table_offset?.checked_add(0x24)?
    } else {
        string_table_offset?.checked_add(8)?
    };
    printable_string_at(data, start, file_size)
}

fn printable_string_at(data: &[u8], start: usize, end: usize) -> Option<String> {
    let tail = data.get(start..end)?;
    let length = tail
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(tail.len());
    let value = &tail[..length];
    (!value.is_empty()
        && value
            .iter()
            .all(|byte| byte.is_ascii_graphic() || *byte == b' '))
    .then(|| String::from_utf8_lossy(value).into_owned())
}

fn strings(data: &[u8], start: usize, end: usize) -> Vec<AmtaString> {
    let mut result = Vec::new();
    let mut at = start;
    while at < end {
        while at < end && data[at] == 0 {
            at += 1;
        }
        let value_start = at;
        while at < end && data[at] != 0 {
            at += 1;
        }
        let value = &data[value_start..at];
        if value.len() >= 2
            && value
                .iter()
                .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
        {
            result.push(AmtaString {
                offset: value_start,
                value: String::from_utf8_lossy(value).into_owned(),
            });
        }
        at += usize::from(at < end);
    }
    result
}

fn words(
    reader: &BinaryReader<'_>,
    data: &[u8],
    start: usize,
    end: usize,
    file_size: usize,
) -> Vec<AmtaWord> {
    let mut result = Vec::new();
    let mut at = start;
    while at.checked_add(4).is_some_and(|next| next <= end) {
        let unsigned = reader.read_u32_at(at).unwrap_or(0);
        let float_value = f32::from_bits(unsigned);
        let float = (float_value.is_finite()
            && float_value != 0.0
            && (float_value.abs() >= 1.0e-20 && float_value.abs() <= 1.0e20))
            .then_some(float_value);
        let bytes = &data[at..at + 4];
        let ascii = bytes
            .iter()
            .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
            .then(|| String::from_utf8_lossy(bytes).into_owned());
        let target =
            (unsigned != 0 && (unsigned as usize) < file_size).then_some(unsigned as usize);
        result.push(AmtaWord {
            offset: at,
            raw_hex: format!("{unsigned:08X}"),
            unsigned,
            signed: unsigned as i32,
            float,
            ascii,
            target,
        });
        at += 4;
    }
    result
}

fn alignment(offset: usize) -> usize {
    if offset == 0 {
        return 0;
    }
    1usize << offset.trailing_zeros().min(12)
}

fn hex_preview(data: &[u8], limit: usize) -> String {
    data.iter()
        .take(limit)
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
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
                assert!(
                    !parsed.header_fields.is_empty(),
                    "{}::{name}",
                    path.display()
                );
                assert!(parsed.total_words > 0, "{}::{name}", path.display());
                count += 1;
            }
        }
        assert!(count > 0);
    }
}
