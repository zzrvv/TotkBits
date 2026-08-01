use base64::Engine;
use serde::Serialize;
use std::io;

use crate::parser::binary::{BinaryReader, BinaryWriter, Endian};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct G1tTexture {
    pub index: usize,
    pub width: u32,
    pub height: u32,
    pub mip_count: u8,
    pub texture_type: u8,
    pub data_url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct G1tFile {
    pub version: [u8; 4],
    pub platform: u32,
    pub textures: Vec<G1tTexture>,
}

#[derive(Clone, Copy)]
enum TextureFormat {
    Rgba8,
    Rgba32Float,
    Bc1,
    Bc3,
    Bc4,
    Bc5,
    Bc6,
    Bc7,
}

impl TextureFormat {
    fn block(self) -> (u32, u32) {
        match self {
            Self::Rgba8 => (1, 4),
            Self::Rgba32Float => (1, 16),
            Self::Bc1 | Self::Bc4 => (4, 8),
            Self::Bc3 | Self::Bc5 | Self::Bc6 | Self::Bc7 => (4, 16),
        }
    }

    fn dxgi(self, srgb: bool) -> Option<u32> {
        match self {
            Self::Rgba32Float => Some(2),
            Self::Bc5 => Some(83),
            Self::Bc6 => Some(95),
            Self::Bc7 => Some(if srgb { 99 } else { 98 }),
            _ => None,
        }
    }

    fn fourcc(self) -> Option<[u8; 4]> {
        match self {
            Self::Bc1 => Some(*b"DXT1"),
            Self::Bc3 => Some(*b"DXT5"),
            Self::Bc4 => Some(*b"ATI1"),
            _ => None,
        }
    }
}

impl G1tFile {
    pub fn parse(data: &[u8]) -> io::Result<Self> {
        let endian = match data.get(..4) {
            Some(b"GT1G") => Endian::Little,
            Some(b"G1TG") => Endian::Big,
            _ => return Err(invalid("not a G1T texture archive")),
        };
        let mut reader = BinaryReader::with_endian(data, endian);
        reader.skip(4)?;
        let version = reader.read_array_at::<4>(4)?;
        reader.skip(4)?;
        let total_size = reader.read_u32()? as usize;
        let table_offset = reader.read_u32()? as usize;
        let texture_count = reader.read_u32()? as usize;
        let platform = reader.read_u32()?;
        let extra_size = reader.read_u32()? as usize;
        if total_size > data.len() || table_offset > total_size || texture_count > 65_536 {
            return Err(invalid("invalid G1T header bounds"));
        }
        let offsets_offset = table_offset;
        if offsets_offset
            .checked_add(texture_count.saturating_mul(4))
            .is_none_or(|end| end > total_size)
        {
            return Err(invalid("invalid G1T texture offset table"));
        }
        let table = BinaryReader::with_endian(data, endian);
        let mut textures = Vec::new();
        for index in 0..texture_count {
            let relative = table.read_u32_at(offsets_offset + index * 4)? as usize;
            let next_relative = if index + 1 < texture_count {
                table.read_u32_at(offsets_offset + (index + 1) * 4)? as usize
            } else {
                total_size.saturating_sub(table_offset)
            };
            if relative < texture_count * 4 + extra_size || next_relative < relative {
                continue;
            }
            let start = table_offset
                .checked_add(relative)
                .ok_or_else(|| invalid("G1T texture offset overflow"))?;
            let end = table_offset
                .checked_add(next_relative)
                .unwrap_or(data.len())
                .min(data.len());
            let entry = table.slice(start, end)?;
            if let Ok(texture) = parse_texture(index, entry, endian) {
                textures.push(texture);
            }
        }
        Ok(Self {
            version,
            platform,
            textures,
        })
    }
}

fn parse_texture(index: usize, entry: &[u8], endian: Endian) -> io::Result<G1tTexture> {
    let mut reader = BinaryReader::with_endian(entry, endian);
    let mip_byte = reader.read_u8()?;
    let texture_type = reader.read_u8()?;
    let dimensions = reader.read_u8()?;
    let extra_header_version = reader.read_array_at::<5>(3)?[4];
    reader.skip(5)?;
    let (mip_count, dx, dy) = match endian {
        Endian::Little => (mip_byte >> 4, dimensions & 0x0f, dimensions >> 4),
        Endian::Big => (mip_byte & 0x0f, dimensions >> 4, dimensions & 0x0f),
    };
    if mip_count == 0 || dx > 30 || dy > 30 {
        return Err(invalid("invalid G1T dimensions or mip count"));
    }
    let mut width = 1u32 << dx;
    let mut height = 1u32 << dy;
    if extra_header_version > 0 {
        let size = reader.read_u32()? as usize;
        if !matches!(size, 12 | 16 | 20) || size > entry.len().saturating_sub(8) {
            return Err(invalid("unsupported G1T extended header"));
        }
        if size >= 16 {
            width = BinaryReader::with_endian(entry, endian).read_u32_at(8 + 12)?;
        }
        if size >= 20 {
            height = BinaryReader::with_endian(entry, endian).read_u32_at(8 + 16)?;
        }
        reader.seek(8 + size)?;
    }
    let format = match texture_type {
        0x00 | 0x01 | 0x02 | 0x21 => TextureFormat::Rgba8,
        0x04 => TextureFormat::Rgba32Float,
        0x06 | 0x59 => TextureFormat::Bc1,
        0x08 | 0x5b => TextureFormat::Bc3,
        0x5c => TextureFormat::Bc4,
        0x5d => TextureFormat::Bc5,
        0x5e => TextureFormat::Bc6,
        0x5f | 0x72 => TextureFormat::Bc7,
        _ => {
            return Err(invalid(&format!(
                "unsupported G1T texture type 0x{texture_type:02X}"
            )))
        }
    };
    let payload = &entry[reader.position()..];
    // Some G1T archives repeat the final 1x1 mip. DDS consumers reject a mip
    // count beyond log2(max dimension) + 1, so decode only the valid chain.
    let decode_mips = mip_count.min(32 - width.max(height).leading_zeros() as u8);
    let expected = mip_chain_size(format, width, height, decode_mips);
    if payload.len() < expected {
        return Err(invalid("truncated G1T texture payload"));
    }
    let dds = make_dds(
        format,
        width,
        height,
        decode_mips,
        false,
        &payload[..expected],
    );
    let image = crate::file_format::Image::dds::decode(&dds)?;
    let png = crate::file_format::Image::png::encode(&image)?;
    Ok(G1tTexture {
        index,
        width,
        height,
        mip_count,
        texture_type,
        data_url: format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(png)
        ),
    })
}

fn mip_chain_size(format: TextureFormat, width: u32, height: u32, mip_count: u8) -> usize {
    let (block, bytes) = format.block();
    (0..mip_count)
        .map(|level| {
            let width = (width >> level).max(1).div_ceil(block);
            let height = (height >> level).max(1).div_ceil(block);
            (width * height * bytes) as usize
        })
        .sum()
}

fn make_dds(
    format: TextureFormat,
    width: u32,
    height: u32,
    mips: u8,
    srgb: bool,
    payload: &[u8],
) -> Vec<u8> {
    let mut writer = BinaryWriter::new();
    writer.write_bytes(b"DDS ");
    writer.write_u32(124);
    writer.write_u32(0x000a_1007);
    writer.write_u32(height);
    writer.write_u32(width);
    let (block, bytes) = format.block();
    writer.write_u32(width.div_ceil(block) * height.div_ceil(block) * bytes);
    writer.write_u32(0);
    writer.write_u32(u32::from(mips));
    for _ in 0..11 {
        writer.write_u32(0);
    }
    writer.write_u32(32);
    if let Some(fourcc) = format.fourcc() {
        writer.write_u32(4);
        writer.write_bytes(&fourcc);
        for _ in 0..5 {
            writer.write_u32(0);
        }
    } else if let Some(dxgi) = format.dxgi(srgb) {
        writer.write_u32(4);
        writer.write_bytes(b"DX10");
        for _ in 0..5 {
            writer.write_u32(0);
        }
        writer.write_u32(0x0040_0008);
        for _ in 0..4 {
            writer.write_u32(0);
        }
        writer.write_u32(dxgi);
        writer.write_u32(3);
        writer.write_u32(0);
        writer.write_u32(1);
        writer.write_u32(0);
        writer.write_bytes(payload);
        return writer.into_inner();
    } else {
        writer.write_u32(0x41);
        writer.write_u32(0);
        writer.write_u32(32);
        writer.write_u32(0x00ff_0000);
        writer.write_u32(0x0000_ff00);
        writer.write_u32(0x0000_00ff);
        writer.write_u32(0xff00_0000);
    }
    writer.write_u32(0x0040_0008);
    for _ in 0..4 {
        writer.write_u32(0);
    }
    writer.write_bytes(payload);
    writer.into_inner()
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dds_header_has_expected_size() {
        assert_eq!(
            make_dds(TextureFormat::Bc1, 4, 4, 1, false, &[0; 8]).len(),
            136
        );
        assert_eq!(
            &make_dds(TextureFormat::Bc7, 4, 4, 1, false, &[0; 16])[..4],
            b"DDS "
        );
    }

    #[test]
    fn parses_aoc_texture_corpus() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp/g1m");
        let mut parsed = 0;
        for path in std::fs::read_dir(root)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("g1t"))
        {
            let archive = G1tFile::parse(&std::fs::read(&path).unwrap())
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert!(!archive.textures.is_empty(), "{}", path.display());
            parsed += 1;
        }
        assert!(parsed >= 80);
    }
}
