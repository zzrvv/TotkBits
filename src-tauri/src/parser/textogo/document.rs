use super::{TexToGoError, TexToGoHeader, TexToGoSurface};
use serde::Serialize;
use std::io::Cursor;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TexToGoFile {
    pub header: TexToGoHeader,
    pub surfaces: Vec<TexToGoSurface>,
}

impl TexToGoFile {
    pub fn parse(data: &[u8]) -> Result<Self, TexToGoError> {
        if data.get(4..8) != Some(b"6PK0") {
            return Err(TexToGoError::new(4, "missing 6PK0 magic"));
        }
        let header = TexToGoHeader {
            header_size: u16_at(data, 0)?,
            version: u16_at(data, 2)?,
            width: u16_at(data, 8)?,
            height: u16_at(data, 10)?,
            depth: u16_at(data, 12)?,
            mip_count: byte(data, 14)?,
            format_flag: byte(data, 19)?,
            format_setting: u32_at(data, 20)?,
            component_selectors: slice(data, 24, 4)?.try_into().unwrap(),
            hash: slice(data, 28, 32)?.try_into().unwrap(),
            format: u16_at(data, 60)?,
            texture_settings: [
                u32_at(data, 64)?,
                u32_at(data, 68)?,
                u32_at(data, 72)?,
                u32_at(data, 76)?,
            ],
        };
        let count = usize::from(header.depth)
            .checked_mul(usize::from(header.mip_count))
            .ok_or_else(|| TexToGoError::new(12, "surface count overflow"))?;
        let mut surfaces = Vec::with_capacity(count);
        let descriptors = usize::from(header.header_size);
        let sizes = descriptors + count * 4;
        let mut payload = sizes + count * 8;
        for i in 0..count {
            let descriptor = descriptors + i * 4;
            let size_entry = sizes + i * 8;
            let compressed_size = u32_at(data, size_entry)?;
            let compressed = slice(data, payload, compressed_size as usize)?;
            let decoded = zstd::stream::decode_all(Cursor::new(compressed)).map_err(|e| {
                TexToGoError::new(payload, format!("invalid Zstandard surface: {e}"))
            })?;
            surfaces.push(TexToGoSurface {
                array_level: u16_at(data, descriptor)?,
                mip_level: byte(data, descriptor + 2)?,
                surface_count: byte(data, descriptor + 3)?,
                compressed_size,
                compression_type: u32_at(data, size_entry + 4)?,
                data: decoded,
            });
            payload += compressed_size as usize;
        }
        Ok(Self { header, surfaces })
    }
}
fn slice(data: &[u8], p: usize, n: usize) -> Result<&[u8], TexToGoError> {
    data.get(p..p + n)
        .ok_or_else(|| TexToGoError::new(p, "unexpected end of file"))
}
fn byte(data: &[u8], p: usize) -> Result<u8, TexToGoError> {
    Ok(*slice(data, p, 1)?.first().unwrap())
}
fn u16_at(data: &[u8], p: usize) -> Result<u16, TexToGoError> {
    Ok(u16::from_le_bytes(slice(data, p, 2)?.try_into().unwrap()))
}
fn u32_at(data: &[u8], p: usize) -> Result<u32, TexToGoError> {
    Ok(u32::from_le_bytes(slice(data, p, 4)?.try_into().unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_all_sample_surfaces() {
        let data = include_bytes!(r"../../../../tmp/tex/Armor_1006_Lower_Alb.7.txtg");
        let file = TexToGoFile::parse(data).unwrap();
        assert_eq!((file.header.width, file.header.height), (640, 640));
        assert_eq!(file.header.mip_count, 10);
        assert_eq!(file.surfaces.len(), 10);
        assert!(file.surfaces.iter().all(|surface| !surface.data.is_empty()));
    }

    #[test]
    fn parses_textogo_corpus() {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp/tex");
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|value| value.to_str()) == Some("txtg") {
                let file = TexToGoFile::parse(&std::fs::read(&path).unwrap())
                    .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
                assert_eq!(
                    file.surfaces.len(),
                    usize::from(file.header.depth) * usize::from(file.header.mip_count)
                );
            }
        }
    }
}
