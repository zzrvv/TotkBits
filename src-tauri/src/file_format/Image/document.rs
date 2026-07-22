use base64::Engine;
use serde::Serialize;
use std::{io, path::Path};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderedImage {
    pub data_url: String,
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub mip_count: u32,
    pub dds_type: Option<String>,
    pub entries: Vec<ImageEntry>,
    pub selected_index: usize,
    pub selected_array_index: u32,
    pub selected_mip_index: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageEntry {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub mip_count: u32,
    pub array_count: u32,
    pub format: String,
    pub subimages: Vec<ImageSubimage>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageSubimage {
    pub array_index: u32,
    pub mip_index: u32,
    pub width: u32,
    pub height: u32,
    pub name: String,
}

pub struct ImageDocument;

impl ImageDocument {
    pub fn open(
        path: &Path,
        zstd: &crate::Zstd::TotkZstd<'_>,
    ) -> Option<(
        crate::file_format::BinTextFile::OpenedFile<'static>,
        crate::Open_and_Save::SendData,
    )> {
        let data = std::fs::read(path).ok()?;
        let bntx_dictionary = decode_compressed_bntx(&data, Some(zstd))
            .ok()
            .and_then(|(_, dictionary)| dictionary);
        if !Self::supports(path, &data, Some(zstd)) {
            return None;
        }
        let mut opened = crate::file_format::BinTextFile::OpenedFile::default();
        opened.path = crate::Settings::Pathlib::new(path);
        opened.file_type = if data.starts_with(b"BNTX") || bntx_dictionary.is_some() {
            crate::Zstd::TotkFileType::Bntx
        } else {
            crate::Zstd::TotkFileType::Other
        };

        let mut send = crate::Open_and_Save::SendData::default();
        send.path = crate::Settings::Pathlib::new(path);
        send.file_label = if opened.file_type == crate::Zstd::TotkFileType::Bntx {
            format!("{} [BNTX]", send.path.name)
        } else {
            format!("{} [IMAGE]", send.path.name)
        };
        send.set_file_metadata(opened.file_type, bntx_dictionary);
        send.status_text = format!("Opened image {}", path.display());
        send.tab = "IMAGE".into();
        send.read_only = true;
        Some((opened, send))
    }

    pub fn render_path(path: impl AsRef<Path>) -> io::Result<RenderedImage> {
        Self::render_path_selection(path, 0, 0, 0)
    }

    pub fn render_path_selection(
        path: impl AsRef<Path>,
        texture_index: usize,
        array_index: u32,
        mip_index: u32,
    ) -> io::Result<RenderedImage> {
        Self::render_path_selection_with_zstd(path, texture_index, array_index, mip_index, None)
    }

    pub fn render_path_selection_with_zstd(
        path: impl AsRef<Path>,
        texture_index: usize,
        array_index: u32,
        mip_index: u32,
        zstd: Option<&crate::Zstd::TotkZstd<'_>>,
    ) -> io::Result<RenderedImage> {
        let path = path.as_ref();
        let source = std::fs::read(path)?;
        let data = maybe_zstd(&source, zstd)?;
        let mut entries = Vec::new();
        let (rgba, format, mip_count, dds_type) = if data.starts_with(b"BNTX") {
            let bntx = crate::parser::bntx::BntxFile::parse(&data).map_err(invalid)?;
            entries = bntx
                .textures
                .iter()
                .map(|texture| ImageEntry {
                    name: texture.name.clone(),
                    width: texture.width,
                    height: texture.height,
                    mip_count: u32::from(texture.mip_count),
                    array_count: texture.array_length,
                    format: format!("0x{:X}", texture.format),
                    subimages: subimages(
                        texture.array_length.max(1),
                        u32::from(texture.mip_count),
                        texture.width,
                        texture.height,
                    ),
                })
                .collect();
            let texture = bntx.textures.get(texture_index).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "BNTX texture index is out of range",
                )
            })?;
            let array_count = texture.array_length.max(1);
            if array_index >= array_count || mip_index >= u32::from(texture.mip_count) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "BNTX subimage index is out of range",
                ));
            }
            let surface_index =
                array_index as usize * usize::from(texture.mip_count) + mip_index as usize;
            let offset = *texture.data_offsets.get(surface_index).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "BNTX texture has no image data")
            })? as usize;
            let end = texture
                .data_offsets
                .get(surface_index + 1)
                .copied()
                .map(|value| value as usize)
                .unwrap_or_else(|| offset.saturating_add(texture.image_size as usize))
                .min(data.len());
            let surface_data = data.get(offset..end).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "BNTX image offset is outside the file",
                )
            })?;
            let width = (texture.width >> mip_index).max(1);
            let height = (texture.height >> mip_index).max(1);
            let block_height_log2 = texture.block_height_log2.saturating_sub(mip_index as u8);
            let (mut image, format_name) = if let Some((block_width, block_height)) =
                super::switch_texture::astc_block_from_bntx(texture.format)
            {
                (
                    super::switch_texture::decode_astc(
                        width,
                        height,
                        surface_data,
                        block_width,
                        block_height,
                        block_height_log2,
                    )?,
                    format!("ASTC_{block_width}x{block_height}"),
                )
            } else {
                let image_format = super::switch_texture::format_from_bntx(texture.format)?;
                (
                    super::switch_texture::decode(
                        width,
                        height,
                        image_format,
                        surface_data,
                        block_height_log2,
                        texture.tile_mode == 1,
                    )?,
                    format!("{image_format:?}"),
                )
            };
            super::switch_texture::apply_component_selectors(&mut image, texture.channel_types);
            (
                image,
                "BNTX".into(),
                u32::from(texture.mip_count),
                Some(format_name),
            )
        } else if data.get(4..8) == Some(b"6PK0") {
            let txtg = crate::parser::textogo::TexToGoFile::parse(&data).map_err(invalid)?;
            let image_format = super::switch_texture::format_from_textogo(txtg.header.format);
            entries.push(ImageEntry {
                name: path
                    .file_stem()
                    .and_then(|x| x.to_str())
                    .unwrap_or("TexToGo")
                    .into(),
                width: u32::from(txtg.header.width),
                height: u32::from(txtg.header.height),
                mip_count: u32::from(txtg.header.mip_count),
                array_count: u32::from(txtg.header.depth),
                format: format!("0x{:X}", txtg.header.format),
                subimages: subimages(
                    u32::from(txtg.header.depth).max(1),
                    u32::from(txtg.header.mip_count),
                    u32::from(txtg.header.width),
                    u32::from(txtg.header.height),
                ),
            });
            let surface = txtg
                .surfaces
                .iter()
                .find(|surface| {
                    u32::from(surface.array_level) == array_index
                        && u32::from(surface.mip_level) == mip_index
                })
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "TexToGo has no base surface")
                })?;
            let selected_width = (u32::from(txtg.header.width) >> mip_index).max(1);
            let selected_height = (u32::from(txtg.header.height) >> mip_index).max(1);
            let log2 = super::switch_texture::inferred_block_height_log2(selected_height, 4);
            let (image, format_name) = match txtg.header.format {
                0x101 | 0x109 => (
                    super::switch_texture::decode_astc(
                        selected_width,
                        selected_height,
                        &surface.data,
                        4,
                        4,
                        log2,
                    )?,
                    "ASTC 4x4".to_owned(),
                ),
                0x102 | 0x105 => (
                    super::switch_texture::decode_astc(
                        selected_width,
                        selected_height,
                        &surface.data,
                        8,
                        8,
                        log2,
                    )?,
                    "ASTC 8x8".to_owned(),
                ),
                _ => {
                    let image_format = image_format?;
                    (
                        super::switch_texture::decode(
                            selected_width,
                            selected_height,
                            image_format,
                            &surface.data,
                            log2,
                            false,
                        )?,
                        format!("{image_format:?}"),
                    )
                }
            };
            (
                image,
                "TexToGo".into(),
                u32::from(txtg.header.mip_count),
                Some(format_name),
            )
        } else if data.starts_with(b"DDS ") {
            let header = super::dds::DdsHeader::parse(&data)?;
            let dds = image_dds::ddsfile::Dds::read(&mut std::io::Cursor::new(&data))
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
            let dds_type = image_dds::dds_image_format(&dds)
                .map(|value| format!("{value:?}"))
                .unwrap_or_else(|_| "Unknown".into());
            let surface = image_dds::Surface::from_dds(&dds)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
            if array_index >= surface.layers || mip_index >= surface.mipmaps {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "DDS subimage index is out of range",
                ));
            }
            entries.push(ImageEntry {
                name: path
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or("DDS")
                    .into(),
                width: surface.width,
                height: surface.height,
                mip_count: surface.mipmaps,
                array_count: surface.layers,
                format: dds_type.clone(),
                subimages: subimages(
                    surface.layers,
                    surface.mipmaps,
                    surface.width,
                    surface.height,
                ),
            });
            let selected = image_dds::SurfaceRgba8::decode_layers_mipmaps_dds(
                &dds,
                array_index..array_index + 1,
                mip_index..mip_index + 1,
            )
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
            (
                selected.get_image(0, 0, 0).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "DDS subimage could not be decoded",
                    )
                })?,
                "DDS".to_string(),
                header.mipmap_count,
                Some(dds_type),
            )
        } else {
            let image = super::raster::decode(&data)?;
            let format = path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("image")
                .to_ascii_uppercase();
            (image, format, 1, None)
        };
        let width = rgba.width();
        let height = rgba.height();
        let png = super::png::encode(&rgba)?;
        Ok(RenderedImage {
            data_url: format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(png)
            ),
            width,
            height,
            format,
            mip_count,
            dds_type,
            entries,
            selected_index: texture_index,
            selected_array_index: array_index,
            selected_mip_index: mip_index,
        })
    }

    pub fn replace_dds(
        target: impl AsRef<Path>,
        png: impl AsRef<Path>,
        dds_type: &str,
        mip_count: u32,
    ) -> io::Result<()> {
        super::dds::replace_from_png(target, png, dds_type, mip_count)
    }

    pub fn replace_dds_surface(
        target: impl AsRef<Path>,
        png: impl AsRef<Path>,
        array_index: u32,
        mip_index: u32,
        replacement_format: Option<&str>,
    ) -> io::Result<()> {
        super::dds::replace_surface_from_png(
            target,
            png,
            array_index,
            mip_index,
            replacement_format,
        )
    }

    pub fn rename_bntx_texture(
        path: impl AsRef<Path>,
        texture_index: usize,
        new_name: &str,
        zstd: &crate::Zstd::TotkZstd<'_>,
    ) -> io::Result<()> {
        if new_name.is_empty() || new_name.as_bytes().len() > u16::MAX as usize {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid BNTX texture name",
            ));
        }
        let path = path.as_ref();
        let source = std::fs::read(path)?;
        let (mut data, dictionary) = decode_compressed_bntx(&source, Some(zstd))?;
        let bntx = crate::parser::bntx::BntxFile::parse(&data).map_err(invalid)?;
        let texture = bntx.textures.get(texture_index).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "BNTX texture index is out of range",
            )
        })?;
        if new_name.as_bytes().len() > texture.name_capacity {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "name is too long for this BNTX string slot (maximum {} UTF-8 bytes)",
                    texture.name_capacity
                ),
            ));
        }
        let length = u16::try_from(new_name.as_bytes().len())
            .unwrap()
            .to_le_bytes();
        data[texture.name_offset..texture.name_offset + 2].copy_from_slice(&length);
        let start = texture.name_offset + 2;
        data[start..start + texture.name_capacity].fill(0);
        data[start..start + new_name.as_bytes().len()].copy_from_slice(new_name.as_bytes());
        let output = match dictionary {
            Some(dictionary) => zstd.compress_with_dictionary(&data, dictionary)?,
            None => data,
        };
        std::fs::write(path, output)
    }

    pub fn replace_bntx_surface(
        path: impl AsRef<Path>,
        png: impl AsRef<Path>,
        texture_index: usize,
        array_index: u32,
        mip_index: u32,
        replacement_format: Option<&str>,
        zstd: &crate::Zstd::TotkZstd<'_>,
    ) -> io::Result<()> {
        let path = path.as_ref();
        let source = std::fs::read(path)?;
        let (mut data, dictionary) = decode_compressed_bntx(&source, Some(zstd))?;
        let bntx = crate::parser::bntx::BntxFile::parse(&data).map_err(invalid)?;
        let texture = bntx.textures.get(texture_index).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "BNTX texture index is out of range",
            )
        })?;
        let custom_format = replacement_format.filter(|value| *value != "ORIGINAL");
        if custom_format.is_some() && (texture.array_length.max(1) != 1 || mip_index != 0) {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "custom BNTX format generation requires selecting mip 0 of a non-array texture",
            ));
        }
        if super::switch_texture::astc_block_from_bntx(texture.format).is_some() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "ASTC BNTX replacement is not supported",
            ));
        }
        let surface_index =
            array_index as usize * usize::from(texture.mip_count) + mip_index as usize;
        let offset = *texture.data_offsets.get(surface_index).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "BNTX subimage index is out of range",
            )
        })? as usize;
        let end = texture
            .data_offsets
            .get(surface_index + 1)
            .copied()
            .map(|value| value as usize)
            .unwrap_or_else(|| offset.saturating_add(texture.image_size as usize))
            .min(data.len());
        let width = (texture.width >> mip_index).max(1);
        let height = (texture.height >> mip_index).max(1);
        let image = image::open(png)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?
            .to_rgba8();
        if image.dimensions() != (width, height) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("replacement must be {width} x {height} pixels"),
            ));
        }
        if let Some(name) = custom_format {
            let (format, bntx_format) = super::switch_texture::format_for_bntx_name(name)?;
            let allocation_end =
                texture.data_offsets[0].saturating_add(u64::from(texture.image_size)) as usize;
            for mip in 0..u32::from(texture.mip_count) {
                let mip_width = (texture.width >> mip).max(1);
                let mip_height = (texture.height >> mip).max(1);
                let mip_image = if mip == 0 {
                    image.clone()
                } else {
                    image::imageops::resize(
                        &image,
                        mip_width,
                        mip_height,
                        image::imageops::FilterType::Triangle,
                    )
                };
                let mip_offset = texture.data_offsets[mip as usize] as usize;
                let mip_end = texture
                    .data_offsets
                    .get(mip as usize + 1)
                    .copied()
                    .map(|value| value as usize)
                    .unwrap_or(allocation_end)
                    .min(data.len());
                let encoded = super::switch_texture::encode(
                    &mip_image,
                    format,
                    texture.block_height_log2.saturating_sub(mip as u8),
                    texture.tile_mode == 1,
                )?;
                if encoded.len() > mip_end.saturating_sub(mip_offset) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("{name} mip {mip} does not fit the BNTX data slot"),
                    ));
                }
                data[mip_offset..mip_end].fill(0);
                data[mip_offset..mip_offset + encoded.len()].copy_from_slice(&encoded);
            }
            data[texture.format_offset..texture.format_offset + 4]
                .copy_from_slice(&bntx_format.to_le_bytes());
            let output = match dictionary {
                Some(dictionary) => zstd.compress_with_dictionary(&data, dictionary)?,
                None => data,
            };
            return std::fs::write(path, output);
        }
        let format = super::switch_texture::format_from_bntx(texture.format)?;
        let encoded = super::switch_texture::encode(
            &image,
            format,
            texture.block_height_log2.saturating_sub(mip_index as u8),
            texture.tile_mode == 1,
        )?;
        if encoded.len() > end.saturating_sub(offset) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "encoded BNTX surface does not fit its existing data slot",
            ));
        }
        data[offset..end].fill(0);
        data[offset..offset + encoded.len()].copy_from_slice(&encoded);
        let output = match dictionary {
            Some(dictionary) => zstd.compress_with_dictionary(&data, dictionary)?,
            None => data,
        };
        std::fs::write(path, output)
    }

    pub fn export_png(source: impl AsRef<Path>, output: impl AsRef<Path>) -> io::Result<()> {
        let rendered = Self::render_path(source)?;
        Self::export_rendered_png(&rendered, output)
    }

    pub fn export_rendered_png(
        rendered: &RenderedImage,
        output: impl AsRef<Path>,
    ) -> io::Result<()> {
        let encoded = rendered
            .data_url
            .split_once(',')
            .map(|(_, value)| value)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid rendered image URL")
            })?;
        let png = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
        std::fs::write(output, png)
    }

    fn supports(path: &Path, data: &[u8], zstd: Option<&crate::Zstd::TotkZstd<'_>>) -> bool {
        if data.starts_with(b"DDS ")
            || data.starts_with(b"BNTX")
            || data.get(4..8) == Some(b"6PK0")
            || data.starts_with(b"\x89PNG\r\n\x1a\n")
        {
            return true;
        }
        if data.starts_with(b"\x28\xB5\x2F\xFD") {
            if let Ok(decoded) = maybe_zstd(data, zstd) {
                return decoded.starts_with(b"BNTX");
            }
        }
        matches!(
            path.extension()
                .and_then(|value| value.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some(
                "bmp"
                    | "gif"
                    | "ico"
                    | "jpg"
                    | "jpeg"
                    | "png"
                    | "pnm"
                    | "qoi"
                    | "tga"
                    | "tif"
                    | "tiff"
                    | "webp"
                    | "dds"
                    | "bntx"
                    | "txtg"
            )
        )
    }
}

fn subimages(array_count: u32, mip_count: u32, width: u32, height: u32) -> Vec<ImageSubimage> {
    (0..array_count)
        .flat_map(|array_index| {
            (0..mip_count).map(move |mip_index| ImageSubimage {
                array_index,
                mip_index,
                width: (width >> mip_index).max(1),
                height: (height >> mip_index).max(1),
                name: format!("Layer {array_index} / Mip {mip_index}"),
            })
        })
        .collect()
}

fn maybe_zstd(data: &[u8], zstd: Option<&crate::Zstd::TotkZstd<'_>>) -> io::Result<Vec<u8>> {
    if data.starts_with(b"\x28\xB5\x2F\xFD") {
        if let Some(zstd) = zstd {
            decode_compressed_bntx(data, Some(zstd)).map(|(decoded, _)| decoded)
        } else {
            crate::Zstd::TotkZstd::decompress_empty(data)
        }
    } else {
        Ok(data.to_vec())
    }
}

fn decode_compressed_bntx(
    data: &[u8],
    zstd: Option<&crate::Zstd::TotkZstd<'_>>,
) -> io::Result<(Vec<u8>, Option<crate::Zstd::ZstdDictionary>)> {
    if data.starts_with(b"BNTX") {
        return Ok((data.to_vec(), None));
    }
    if !data.starts_with(b"\x28\xB5\x2F\xFD") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "data is neither BNTX nor Zstandard",
        ));
    }
    let Some(zstd) = zstd else {
        let decoded = crate::Zstd::TotkZstd::decompress_empty(data)?;
        return decoded
            .starts_with(b"BNTX")
            .then_some((decoded, None))
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "Zstandard payload is not BNTX")
            });
    };
    for dictionary in [
        crate::Zstd::ZstdDictionary::Zs,
        crate::Zstd::ZstdDictionary::Pack,
        crate::Zstd::ZstdDictionary::Empty,
        crate::Zstd::ZstdDictionary::Bcett,
    ] {
        if let Ok(decoded) = zstd.try_decompress_using(data, dictionary) {
            if decoded.starts_with(b"BNTX") {
                return Ok((decoded, Some(dictionary)));
            }
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "no configured Zstandard dictionary produced a BNTX payload",
    ))
}
fn invalid(error: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compressed_non_image_is_not_accepted_by_extension() {
        assert!(!ImageDocument::supports(
            Path::new("Event/Test.bfevfl.zs"),
            b"not an image",
            None,
        ));
    }

    #[test]
    fn renders_bntx_sample() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp");
        let path = root.join("tex/Animal_Insect_A.bntx");
        let rendered = ImageDocument::render_path(path).unwrap();
        assert_eq!(
            (rendered.width, rendered.height, rendered.format.as_str()),
            (256, 256, "BNTX")
        );
        assert_eq!(rendered.dds_type.as_deref(), Some("ASTC_4x4"));

        let encoded = rendered.data_url.split_once(',').unwrap().1;
        let actual_png = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        let actual = image::load_from_memory(&actual_png).unwrap().to_rgba8();
        let expected = image::open(root.join("_ss/Animal_Insect_A.png"))
            .unwrap()
            .to_rgba8();
        assert_eq!(actual.dimensions(), expected.dimensions());
        let total_error: u64 = actual
            .as_raw()
            .iter()
            .zip(expected.as_raw())
            .map(|(actual, expected)| u64::from(actual.abs_diff(*expected)))
            .sum();
        let mean_error = total_error as f64 / actual.as_raw().len() as f64;
        assert!(
            mean_error < 2.0,
            "rendered BNTX differs from the reference PNG (mean channel error {mean_error:.3})"
        );
    }

    #[test]
    fn renders_textogo_sample() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp/tex/Armor_1006_Lower_Alb.7.txtg");
        let rendered = ImageDocument::render_path(path).unwrap();
        assert_eq!(
            (rendered.width, rendered.height, rendered.format.as_str()),
            (640, 640, "TexToGo")
        );
    }
}
