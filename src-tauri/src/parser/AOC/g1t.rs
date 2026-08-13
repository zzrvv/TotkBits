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
    pub array_count: u32,
    pub renderable: bool,
    pub data_url: String,
    pub data_urls: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct G1tTextureSurface {
    pub index: usize,
    pub width: u32,
    pub height: u32,
    pub mip_count: u8,
    pub texture_type: u8,
    pub array_count: u32,
    pub data_offset: usize,
    pub layer_stride: usize,
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
        Ok(Self {
            textures: Self::parse_internal(data, false)?,
            version: parse_version(data)?,
            platform: parse_platform(data)?,
        })
    }

    /// Parses only the layers used by the 3D preview (first and last), while
    /// preserving the archive's real array count in the returned metadata.
    pub fn parse_preview(data: &[u8]) -> io::Result<Self> {
        Ok(Self {
            textures: Self::parse_internal(data, true)?,
            version: parse_version(data)?,
            platform: parse_platform(data)?,
        })
    }

    fn parse_internal(data: &[u8], preview_layers_only: bool) -> io::Result<Vec<G1tTexture>> {
        let endian = match data.get(..4) {
            Some(b"GT1G") => Endian::Little,
            Some(b"G1TG") => Endian::Big,
            _ => return Err(invalid("not a G1T texture archive")),
        };
        let mut reader = BinaryReader::with_endian(data, endian);
        reader.skip(4)?;
        let _version = reader.read_array_at::<4>(4)?;
        reader.skip(4)?;
        let total_size = reader.read_u32()? as usize;
        let table_offset = reader.read_u32()? as usize;
        let texture_count = reader.read_u32()? as usize;
        let _platform = reader.read_u32()?;
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
        let mut textures = Vec::with_capacity(texture_count);
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
            if let Ok(texture) = parse_texture(index, entry, endian, preview_layers_only) {
                textures.push(texture);
            }
        }
        Ok(textures)
    }

    pub fn parse_surfaces(data: &[u8]) -> io::Result<Vec<G1tTextureSurface>> {
        let surfaces = parse_surfaces_internal(data)?;
        Ok(surfaces)
    }
}

fn parse_platform(data: &[u8]) -> io::Result<u32> {
    let endian = match data.get(..4) {
        Some(b"GT1G") => Endian::Little,
        Some(b"G1TG") => Endian::Big,
        _ => return Err(invalid("not a G1T texture archive")),
    };
    let mut reader = BinaryReader::with_endian(data, endian);
    reader.skip(16)?;
    let _ = reader.read_u32()?;
    let platform = reader.read_u32()?;
    Ok(platform)
}

fn parse_version(data: &[u8]) -> io::Result<[u8; 4]> {
    match data.get(..4) {
        Some(b"GT1G") | Some(b"G1TG") => Ok(BinaryReader::with_endian(
            data,
            match data.get(..4) {
                Some(b"GT1G") => Endian::Little,
                _ => Endian::Big,
            },
        )
        .read_array_at(4)?),
        _ => Err(invalid("not a G1T texture archive")),
    }
}

fn parse_surfaces_internal(data: &[u8]) -> io::Result<Vec<G1tTextureSurface>> {
    let endian = match data.get(..4) {
        Some(b"GT1G") => Endian::Little,
        Some(b"G1TG") => Endian::Big,
        _ => return Err(invalid("not a G1T texture archive")),
    };
    let mut reader = BinaryReader::with_endian(data, endian);
    reader.skip(4)?;
    reader.skip(4)?;
    let total_size = reader.read_u32()? as usize;
    let table_offset = reader.read_u32()? as usize;
    let texture_count = reader.read_u32()? as usize;
    reader.read_u32()?;
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
    let mut textures = Vec::with_capacity(texture_count);
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
        if let Ok(texture) = parse_surface(index, start, entry, endian, false) {
            textures.push(texture);
        }
    }
    Ok(textures)
}

fn parse_texture(
    index: usize,
    entry: &[u8],
    endian: Endian,
    preview_layers_only: bool,
) -> io::Result<G1tTexture> {
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
    let format = format_for_type(texture_type)?;
    let payload = &entry[reader.position()..];
    // Some G1T archives repeat the final 1x1 mip. DDS consumers reject a mip
    // count beyond log2(max dimension) + 1, so decode only the valid chain.
    let decode_mips = mip_count.min(32 - width.max(height).leading_zeros() as u8);
    let expected = mip_chain_size(format, width, height, decode_mips);
    if payload.len() < expected {
        return Err(invalid("truncated G1T texture payload"));
    }
    let decode_mips = mip_count.min(32 - width.max(height).leading_zeros() as u8);
    let full_stride = mip_chain_size(format, width, height, mip_count);
    let decode_stride = mip_chain_size(format, width, height, decode_mips);
    let layer_payload = if full_stride > 0 && payload.len() % full_stride == 0 {
        full_stride
    } else if decode_stride > 0 && payload.len() % decode_stride == 0 {
        decode_stride
    } else {
        return Err(invalid("truncated G1T texture payload"));
    };
    let array_count = (payload.len() / layer_payload).clamp(1, 256);
    if layer_payload < expected {
        return Err(invalid("truncated G1T texture payload"));
    }
    // The viewer only needs the highest-resolution image. Do not pass the
    // remaining mip chain through DDS decoding or retain it in the PNG data.
    let base_mip_size = mip_chain_size(format, width, height, 1);
    let layers: Vec<_> = if preview_layers_only && array_count > 2 {
        vec![0, array_count - 1]
    } else {
        (0..array_count).collect()
    };
    let mut data_urls = Vec::with_capacity(layers.len());
    let mut renderable = false;
    for layer in layers {
        let start = layer * layer_payload;
        let dds = make_dds(
            format,
            width,
            height,
            1,
            false,
            &payload[start..start + base_mip_size],
        );
        let mut image = crate::file_format::Image::dds::decode(&dds)?;
        if preview_layers_only && image.width().max(image.height()) > 384 {
            let scale = 384.0 / f64::from(image.width().max(image.height()));
            let width = (f64::from(image.width()) * scale).round().max(1.0) as u32;
            let height = (f64::from(image.height()) * scale).round().max(1.0) as u32;
            image = image::imageops::resize(
                &image,
                width,
                height,
                image::imageops::FilterType::Triangle,
            );
        }
        if layer == 0 {
            renderable = is_renderable_texture(&image);
        }
        let png = crate::file_format::Image::png::encode(&image)?;
        data_urls.push(format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(png)
        ));
    }
    Ok(G1tTexture {
        index,
        width,
        height,
        mip_count,
        texture_type,
        array_count: array_count as u32,
        renderable,
        data_url: data_urls.first().cloned().unwrap_or_default(),
        data_urls,
    })
}

fn parse_surface(
    index: usize,
    entry_offset: usize,
    entry: &[u8],
    endian: Endian,
    _preview_layers_only: bool,
) -> io::Result<G1tTextureSurface> {
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
    let payload = &entry[reader.position()..];
    let format = format_for_type(texture_type)?;
    let decode_mips = mip_count.min(32 - width.max(height).leading_zeros() as u8);
    let decode_size = mip_chain_size(format, width, height, decode_mips);
    let stride = {
        let full_size = mip_chain_size(format, width, height, mip_count);
        if full_size > 0 && payload.len() % full_size == 0 {
            full_size
        } else if decode_size > 0 && payload.len() % decode_size == 0 {
            decode_size
        } else if payload.len() >= decode_size {
            decode_size
        } else {
            return Err(invalid("truncated G1T texture payload"));
        }
    };
    let array_count = (payload.len() / stride).clamp(1, 256);
    Ok(G1tTextureSurface {
        index,
        width,
        height,
        mip_count,
        texture_type,
        array_count: array_count as u32,
        data_offset: entry_offset + reader.position(),
        layer_stride: stride,
    })
}

fn is_renderable_texture(image: &image::RgbaImage) -> bool {
    image.width().saturating_mul(image.height()) > 1
        && image
            .pixels()
            .any(|pixel| pixel[3] != 0 && (pixel[0] != 0 || pixel[1] != 0 || pixel[2] != 0))
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

pub(crate) fn format_for_type(texture_type: u8) -> io::Result<TextureFormat> {
    match texture_type {
        0x00 | 0x01 | 0x02 | 0x21 => Ok(TextureFormat::Rgba8),
        0x04 => Ok(TextureFormat::Rgba32Float),
        0x06 | 0x59 => Ok(TextureFormat::Bc1),
        0x08 | 0x5b => Ok(TextureFormat::Bc3),
        0x5c => Ok(TextureFormat::Bc4),
        0x5d => Ok(TextureFormat::Bc5),
        0x5e => Ok(TextureFormat::Bc6),
        0x5f | 0x72 => Ok(TextureFormat::Bc7),
        _ => Err(invalid(&format!(
            "unsupported G1T texture type 0x{texture_type:02X}"
        ))),
    }
}

pub(crate) fn format_to_image_dds(
    texture_format: TextureFormat,
) -> io::Result<image_dds::ImageFormat> {
    use image_dds::ImageFormat;
    match texture_format {
        TextureFormat::Rgba8 => Ok(ImageFormat::Rgba8Unorm),
        TextureFormat::Rgba32Float => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "G1T replacement for Rgba32Float is not currently implemented",
        )),
        TextureFormat::Bc1 => Ok(ImageFormat::BC1RgbaUnorm),
        TextureFormat::Bc3 => Ok(ImageFormat::BC3RgbaUnorm),
        TextureFormat::Bc4 => Ok(ImageFormat::BC4RUnorm),
        TextureFormat::Bc5 => Ok(ImageFormat::BC5RgUnorm),
        TextureFormat::Bc6 => Ok(ImageFormat::BC6hRgbUfloat),
        TextureFormat::Bc7 => Ok(ImageFormat::BC7RgbaUnorm),
    }
}

pub(crate) fn image_dds_format_for_type(texture_type: u8) -> io::Result<image_dds::ImageFormat> {
    format_to_image_dds(format_for_type(texture_type)?)
}

pub(crate) fn texture_mip_size(
    texture_type: u8,
    width: u32,
    height: u32,
    mip_level: u8,
) -> io::Result<usize> {
    let format = format_for_type(texture_type)?;
    let width = (width >> mip_level).max(1);
    let height = (height >> mip_level).max(1);
    let (block, bytes) = format.block();
    Ok((width.div_ceil(block) * height.div_ceil(block) * bytes) as usize)
}

pub(crate) fn texture_chain_size(
    texture_type: u8,
    width: u32,
    height: u32,
    mip_count: u8,
) -> io::Result<usize> {
    let mut total = 0usize;
    for mip in 0..mip_count {
        total = total.saturating_add(texture_mip_size(texture_type, width, height, mip)?);
    }
    Ok(total)
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
    fn rejects_placeholder_emission_images() {
        let one_pixel = image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 255, 255, 255]));
        let black = image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 0, 255]));
        let transparent = image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 80, 20, 0]));
        let mut visible = black.clone();
        visible.put_pixel(1, 1, image::Rgba([1, 0, 0, 255]));

        assert!(!is_renderable_texture(&one_pixel));
        assert!(!is_renderable_texture(&black));
        assert!(!is_renderable_texture(&transparent));
        assert!(is_renderable_texture(&visible));
    }

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

    #[test]
    fn parses_emission_texture_arrays() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp/g1m/00648a45.g1t");
        let archive = G1tFile::parse(&std::fs::read(path).unwrap()).unwrap();
        let texture = archive.textures.first().unwrap();
        assert_eq!(texture.array_count, 2);
        assert_eq!(texture.data_urls.len(), texture.array_count as usize);
        assert_ne!(texture.data_urls.first(), texture.data_urls.last());
    }

    #[test]
    fn first_emission_layer_matches_legacy_extraction() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp/g1m_importer/_ss/data");
        let g1t_path = root.join("0202a4c9.g1t");
        let legacy_path = root.join("49c9ef33_textures/emm_0_0202a4c9_17.dds");
        if !g1t_path.is_file() || !legacy_path.is_file() {
            return;
        }
        let archive = G1tFile::parse(&std::fs::read(g1t_path).unwrap()).unwrap();
        let texture = archive.textures.first().unwrap();
        let encoded = texture
            .data_urls
            .first()
            .unwrap()
            .split_once(',')
            .unwrap()
            .1;
        let png = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        let decoded = crate::file_format::Image::raster::decode(&png).unwrap();
        let legacy =
            crate::file_format::Image::dds::decode(&std::fs::read(legacy_path).unwrap()).unwrap();
        assert_eq!(decoded, legacy);
    }
}
