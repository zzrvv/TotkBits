use super::{BntxError, BntxTexture};
use crate::parser::binary::BinaryReader;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BntxFile {
    pub version: [u8; 4],
    pub target: String,
    pub file_size: u32,
    pub textures: Vec<BntxTexture>,
}

impl BntxFile {
    pub fn parse(data: &[u8]) -> Result<Self, BntxError> {
        let reader = BinaryReader::new(data);
        if bytes_at(&reader, 0, 4)? != b"BNTX" {
            return Err(BntxError::new(0, "missing BNTX magic"));
        }
        let file_size = u32_at(&reader, 0x1c)?;
        if file_size as usize > data.len() {
            return Err(BntxError::new(0x1c, "declared file size exceeds input"));
        }
        let target = String::from_utf8_lossy(bytes_at(&reader, 0x20, 4)?).to_string();
        let texture_count = u32_at(&reader, 0x24)? as usize;
        let info_ptrs = usize::try_from(u64_at(&reader, 0x28)?)
            .map_err(|_| BntxError::new(0x28, "texture table pointer is too large"))?;
        let table_size = texture_count
            .checked_mul(8)
            .ok_or_else(|| BntxError::new(0x24, "texture table size overflow"))?;
        bytes_at(&reader, info_ptrs, table_size)?;
        let mut textures = Vec::with_capacity(texture_count);
        for index in 0..texture_count {
            let entry = info_ptrs
                .checked_add(index * 8)
                .ok_or_else(|| BntxError::new(info_ptrs, "texture pointer offset overflow"))?;
            let ptr = usize::try_from(u64_at(&reader, entry)?)
                .map_err(|_| BntxError::new(entry, "texture pointer is too large"))?;
            textures.push(parse_texture(&reader, ptr)?);
        }
        Ok(Self {
            version: array_at(&reader, 8)?,
            target,
            file_size,
            textures,
        })
    }
}

fn parse_texture(reader: &BinaryReader<'_>, p: usize) -> Result<BntxTexture, BntxError> {
    if bytes_at(reader, p, 4)? != b"BRTI" {
        return Err(BntxError::new(p, "texture pointer does not reference BRTI"));
    }
    let at = |relative| {
        p.checked_add(relative)
            .ok_or_else(|| BntxError::new(p, "texture field offset overflow"))
    };
    let name_field = at(0x60)?;
    let name_ptr = usize::try_from(u64_at(reader, name_field)?)
        .map_err(|_| BntxError::new(name_field, "name pointer is too large"))?;
    let name = read_res_string(reader, name_ptr)?;
    let name_capacity = usize::from(u16_at(reader, name_ptr)?);
    let mip_count = u16_at(reader, at(0x16)?)?;
    let array_length = u32_at(reader, at(0x30)?)?;
    let mip_field = at(0x70)?;
    let mip_ptrs = usize::try_from(u64_at(reader, mip_field)?)
        .map_err(|_| BntxError::new(mip_field, "mip pointer table is too large"))?;
    let array_count_field = at(0x30)?;
    let pointer_count = usize::from(mip_count)
        .checked_mul(array_length.max(1) as usize)
        .ok_or_else(|| BntxError::new(array_count_field, "mip pointer count overflow"))?;
    let pointer_bytes = pointer_count
        .checked_mul(8)
        .ok_or_else(|| BntxError::new(mip_ptrs, "mip pointer table size overflow"))?;
    bytes_at(reader, mip_ptrs, pointer_bytes)?;
    let mut data_offsets = Vec::with_capacity(pointer_count);
    for i in 0..pointer_count {
        data_offsets.push(u64_at(reader, mip_ptrs + i * 8)?);
    }
    Ok(BntxTexture {
        name,
        width: u32_at(reader, at(0x24)?)?,
        height: u32_at(reader, at(0x28)?)?,
        depth: u32_at(reader, at(0x2c)?)?,
        array_length,
        mip_count,
        format: u32_at(reader, at(0x1c)?)?,
        image_size: u32_at(reader, at(0x50)?)?,
        alignment: u32_at(reader, at(0x54)?)?,
        tile_mode: u16_at(reader, at(0x12)?)?,
        swizzle: u16_at(reader, at(0x14)?)?,
        block_height_log2: (u32_at(reader, at(0x34)?)? & 7) as u8,
        channel_types: array_at(reader, at(0x58)?)?,
        data_offsets,
        name_offset: name_ptr,
        name_capacity,
        format_offset: at(0x1c)?,
    })
}

fn read_res_string(reader: &BinaryReader<'_>, p: usize) -> Result<String, BntxError> {
    let length = u16_at(reader, p)? as usize;
    let text = p
        .checked_add(2)
        .ok_or_else(|| BntxError::new(p, "string offset overflow"))?;
    Ok(String::from_utf8_lossy(bytes_at(reader, text, length)?).to_string())
}
fn bytes_at<'a>(reader: &BinaryReader<'a>, p: usize, n: usize) -> Result<&'a [u8], BntxError> {
    reader
        .read_bytes_at(p, n)
        .map_err(|error| BntxError::new(p, error.to_string()))
}
fn array_at<const N: usize>(reader: &BinaryReader<'_>, p: usize) -> Result<[u8; N], BntxError> {
    reader
        .read_array_at(p)
        .map_err(|error| BntxError::new(p, error.to_string()))
}
fn u16_at(reader: &BinaryReader<'_>, p: usize) -> Result<u16, BntxError> {
    reader
        .read_u16_at(p)
        .map_err(|error| BntxError::new(p, error.to_string()))
}
fn u32_at(reader: &BinaryReader<'_>, p: usize) -> Result<u32, BntxError> {
    reader
        .read_u32_at(p)
        .map_err(|error| BntxError::new(p, error.to_string()))
}
fn u64_at(reader: &BinaryReader<'_>, p: usize) -> Result<u64, BntxError> {
    reader
        .read_u64_at(p)
        .map_err(|error| BntxError::new(p, error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_insect_sample() {
        let data = include_bytes!(r"../../../../tmp/tex/Animal_Insect_A.bntx");
        let file = BntxFile::parse(data).unwrap();
        assert_eq!(file.target, "NX  ");
        assert_eq!(file.textures.len(), 1);
        let texture = &file.textures[0];
        assert_eq!(texture.name, "Animal_Insect_A");
        assert_eq!((texture.width, texture.height), (256, 256));
        assert!(!texture.data_offsets.is_empty());
    }

    #[test]
    fn parses_bntx_corpus() {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp/tex");
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|value| value.to_str()) == Some("bntx") {
                let file = BntxFile::parse(&std::fs::read(&path).unwrap())
                    .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
                assert!(!file.textures.is_empty(), "{}", path.display());
            }
        }
    }
}
