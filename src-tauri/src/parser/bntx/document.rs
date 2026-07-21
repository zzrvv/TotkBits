use super::{BntxError, BntxTexture};
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
        if data.get(..4) != Some(b"BNTX") {
            return Err(BntxError::new(0, "missing BNTX magic"));
        }
        let file_size = u32_at(data, 0x1c)?;
        let target = String::from_utf8_lossy(slice(data, 0x20, 4)?).to_string();
        let texture_count = u32_at(data, 0x24)? as usize;
        let info_ptrs = u64_at(data, 0x28)? as usize;
        let mut textures = Vec::with_capacity(texture_count);
        for index in 0..texture_count {
            let ptr = u64_at(data, info_ptrs + index * 8)? as usize;
            textures.push(parse_texture(data, ptr)?);
        }
        Ok(Self {
            version: slice(data, 8, 4)?.try_into().unwrap(),
            target,
            file_size,
            textures,
        })
    }
}

fn parse_texture(data: &[u8], p: usize) -> Result<BntxTexture, BntxError> {
    if slice(data, p, 4)? != b"BRTI" {
        return Err(BntxError::new(p, "texture pointer does not reference BRTI"));
    }
    let name_ptr = u64_at(data, p + 0x60)? as usize;
    let name = read_res_string(data, name_ptr)?;
    let mip_count = u16_at(data, p + 0x16)?;
    let array_length = u32_at(data, p + 0x30)?;
    let mip_ptrs = u64_at(data, p + 0x70)? as usize;
    let pointer_count = usize::from(mip_count).saturating_mul(array_length.max(1) as usize);
    let mut data_offsets = Vec::with_capacity(pointer_count);
    for i in 0..pointer_count {
        data_offsets.push(u64_at(data, mip_ptrs + i * 8)?);
    }
    Ok(BntxTexture {
        name,
        width: u32_at(data, p + 0x24)?,
        height: u32_at(data, p + 0x28)?,
        depth: u32_at(data, p + 0x2c)?,
        array_length,
        mip_count,
        format: u32_at(data, p + 0x1c)?,
        image_size: u32_at(data, p + 0x50)?,
        alignment: u32_at(data, p + 0x54)?,
        tile_mode: u16_at(data, p + 0x12)?,
        swizzle: u16_at(data, p + 0x14)?,
        block_height_log2: (u32_at(data, p + 0x34)? & 7) as u8,
        channel_types: slice(data, p + 0x58, 4)?.try_into().unwrap(),
        data_offsets,
    })
}

fn read_res_string(data: &[u8], p: usize) -> Result<String, BntxError> {
    let length = u16_at(data, p)? as usize;
    Ok(String::from_utf8_lossy(slice(data, p + 2, length)?).to_string())
}
fn slice(data: &[u8], p: usize, n: usize) -> Result<&[u8], BntxError> {
    data.get(p..p + n)
        .ok_or_else(|| BntxError::new(p, "unexpected end of file"))
}
fn u16_at(data: &[u8], p: usize) -> Result<u16, BntxError> {
    Ok(u16::from_le_bytes(slice(data, p, 2)?.try_into().unwrap()))
}
fn u32_at(data: &[u8], p: usize) -> Result<u32, BntxError> {
    Ok(u32::from_le_bytes(slice(data, p, 4)?.try_into().unwrap()))
}
fn u64_at(data: &[u8], p: usize) -> Result<u64, BntxError> {
    Ok(u64::from_le_bytes(slice(data, p, 8)?.try_into().unwrap()))
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
